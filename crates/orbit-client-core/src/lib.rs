//! Embedded Multi-Host Rust Client Core for Flutter (iOS / Android) and `orbit` CLI.
//!
//! Provides:
//! - Concurrent Multi-Host fleet management (Devbox, Linux workstation, Mac mini)
//! - Happy-Eyeballs smart routing: LAN/Tailscale direct WebSocket first, seamless fallback to Noise NK E2EE Relay
//! - Zstd wire compression + zero-copy binary terminal framing
//! - `0ms` instant-open local SQLite WAL replica cache with monotonic `since_seq` catch-up

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use orbit_crypto::{ClientHandshake, PairingOffer, SecureChannel, decode_key_32};
use orbit_protocol::{PROTOCOL_VERSION, TerminalBinaryFrame, WireMessage};
use orbit_store::{OrbitStore, StoreError};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

/// Envelope discriminator inside an E2EE Noise tunnel:
/// `0x00` = Zstd-compressed `WireMessage`, `0x01` = raw `TerminalBinaryFrame`.
pub const E2EE_FRAME_WIRE: u8 = 0x00;
pub const E2EE_FRAME_TERMINAL: u8 = 0x01;

/// Raw direct WS frame discriminator when sending binary over unencrypted LAN/TLS WebSocket:
/// `0x00` = Zstd-compressed `WireMessage`, `0x01..=0x04` = `TerminalBinaryFrame` opcodes!
pub const DIRECT_BIN_ZSTD_WIRE: u8 = 0x00;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostConnectionState {
    Disconnected,
    ConnectingDirect,
    ConnectingRelay,
    OnlineDirect,
    OnlineRelay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedHostProfile {
    pub host_id: String,
    pub display_name: String,
    pub daemon_pubkey_b64: String,
    pub direct_endpoints: Vec<String>,
    pub relay_url: Option<String>,
    pub state: HostConnectionState,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ClientInboundEvent {
    Wire(WireMessage),
    Terminal(TerminalBinaryFrame),
    Disconnected,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum OutboundPayload {
    Wire(WireMessage),
    Terminal(TerminalBinaryFrame),
}

#[derive(Debug, thiserror::Error)]
pub enum ClientCoreError {
    #[error("crypto error: {0}")]
    Crypto(#[from] orbit_crypto::CryptoError),
    #[error("protocol error: {0}")]
    Protocol(#[from] orbit_protocol::ProtocolError),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("websocket error: {0}")]
    Ws(Box<tokio_tungstenite::tungstenite::Error>),
    #[error("connection handshake failed: {0}")]
    Handshake(String),
    #[error("no reachable direct endpoint or relay configured")]
    Unreachable,
    #[error("channel closed")]
    ChannelClosed,
    #[error("lock poisoned")]
    LockPoisoned,
}

impl From<tokio_tungstenite::tungstenite::Error> for ClientCoreError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::Ws(Box::new(e))
    }
}

/// Live bidirectional connection to one Orbit Host (either direct or Noise E2EE over Relay).
#[derive(Clone)]
pub struct HostSessionHandle {
    pub mode: HostConnectionState,
    out_tx: mpsc::Sender<OutboundPayload>,
    events_tx: broadcast::Sender<ClientInboundEvent>,
}

impl HostSessionHandle {
    pub fn subscribe(&self) -> broadcast::Receiver<ClientInboundEvent> {
        self.events_tx.subscribe()
    }

    pub async fn send_wire(&self, msg: WireMessage) -> Result<(), ClientCoreError> {
        self.out_tx
            .send(OutboundPayload::Wire(msg))
            .await
            .map_err(|_| ClientCoreError::ChannelClosed)
    }

    pub async fn send_terminal(&self, frame: TerminalBinaryFrame) -> Result<(), ClientCoreError> {
        self.out_tx
            .send(OutboundPayload::Terminal(frame))
            .await
            .map_err(|_| ClientCoreError::ChannelClosed)
    }

    /// Perform a correlated request/response RPC call with timeout.
    pub async fn request<F>(
        &self,
        msg: WireMessage,
        timeout: Duration,
        mut matcher: F,
    ) -> Result<WireMessage, ClientCoreError>
    where
        F: FnMut(&WireMessage) -> bool,
    {
        let mut rx = self.subscribe();
        self.send_wire(msg).await?;

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(ClientCoreError::Handshake("rpc request timed out".into()));
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(ClientInboundEvent::Wire(w))) => {
                    if matcher(&w) {
                        return Ok(w);
                    }
                    if let WireMessage::RpcError { code, message, .. } = &w {
                        return Err(ClientCoreError::Handshake(format!("[{code}] {message}")));
                    }
                }
                Ok(Ok(ClientInboundEvent::Disconnected)) => {
                    return Err(ClientCoreError::ChannelClosed);
                }
                Err(_) => {
                    return Err(ClientCoreError::Handshake("rpc request timed out".into()));
                }
                _ => continue,
            }
        }
    }
}

/// Connect directly to an Orbit Daemon WebSocket endpoint with Zstd compression.
pub async fn connect_direct(
    ws_url: &str,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    let (ws_stream, _) = connect_async(ws_url).await?;
    let (mut sink, mut stream) = ws_stream.split();

    let hello = WireMessage::Hello {
        client_id: Uuid::new_v4().to_string(),
        client_name: "orbit-client".into(),
        protocol_version: PROTOCOL_VERSION,
        supports_zstd: true,
    };
    let hello_json = serde_json::to_string(&hello).map_err(orbit_protocol::ProtocolError::from)?;
    sink.send(Message::Text(hello_json.into())).await?;

    let (out_tx, mut out_rx) = mpsc::channel::<OutboundPayload>(256);
    let (events_tx, _) = broadcast::channel::<ClientInboundEvent>(512);

    tokio::spawn(async move {
        while let Some(item) = out_rx.recv().await {
            let ws_msg = match item {
                OutboundPayload::Wire(w) => match w.encode_zstd(3) {
                    Ok(z) => {
                        let mut framed = Vec::with_capacity(1 + z.len());
                        framed.push(DIRECT_BIN_ZSTD_WIRE);
                        framed.extend_from_slice(&z);
                        Message::Binary(framed.into())
                    }
                    Err(_) => continue,
                },
                OutboundPayload::Terminal(t) => Message::Binary(t.encode().into()),
            };
            if sink.send(ws_msg).await.is_err() {
                break;
            }
        }
    });

    let events_in = events_tx.clone();
    tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(txt) => {
                    if let Ok(wire) = serde_json::from_str::<WireMessage>(&txt) {
                        sync_to_replica(replica.as_deref(), &wire);
                        let _ = events_in.send(ClientInboundEvent::Wire(wire));
                    }
                }
                Message::Binary(bin) if !bin.is_empty() => {
                    if bin[0] == DIRECT_BIN_ZSTD_WIRE {
                        if let Ok(wire) = WireMessage::decode_zstd(&bin[1..]) {
                            sync_to_replica(replica.as_deref(), &wire);
                            let _ = events_in.send(ClientInboundEvent::Wire(wire));
                        }
                    } else if let Ok(term) = TerminalBinaryFrame::decode(&bin) {
                        let _ = events_in.send(ClientInboundEvent::Terminal(term));
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        let _ = events_in.send(ClientInboundEvent::Disconnected);
    });

    Ok(HostSessionHandle {
        mode: HostConnectionState::OnlineDirect,
        out_tx,
        events_tx,
    })
}

/// Connect through an untrusted/self-hosted Relay using `Noise_NK_25519_ChaChaPoly_SHA256` E2EE.
pub async fn connect_via_relay(
    relay_base_url: &str,
    daemon_pubkey_b64: &str,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    let pubkey_bytes = decode_key_32(daemon_pubkey_b64)?;
    let url = format!(
        "{}/v1/client/{}",
        relay_base_url.trim_end_matches('/'),
        daemon_pubkey_b64
    );

    let (ws_stream, _) = connect_async(&url).await?;
    let (mut sink, mut stream) = ws_stream.split();

    // Noise NK Message 1: -> e, es
    let (hs, msg1) = ClientHandshake::new(&pubkey_bytes)?;
    sink.send(Message::Binary(msg1.into())).await?;

    // Noise NK Message 2: <- e, ee
    let msg2 = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(frame)) = stream.next().await {
            if let Message::Binary(b) = frame {
                return Some(b);
            }
        }
        None
    })
    .await
    .map_err(|_| ClientCoreError::Handshake("relay E2EE handshake timed out".into()))?
    .ok_or_else(|| ClientCoreError::Handshake("relay closed during E2EE handshake".into()))?;

    let write_chan: SecureChannel = hs.finalize(&msg2)?;
    let shared_chan = Arc::new(tokio::sync::Mutex::new(write_chan));

    // Send encrypted Hello inside the established Noise tunnel
    {
        let hello = WireMessage::Hello {
            client_id: Uuid::new_v4().to_string(),
            client_name: "orbit-mobile-e2ee".into(),
            protocol_version: PROTOCOL_VERSION,
            supports_zstd: true,
        };
        let z = hello.encode_zstd(3)?;
        let mut plain = Vec::with_capacity(1 + z.len());
        plain.push(E2EE_FRAME_WIRE);
        plain.extend_from_slice(&z);
        let ct = shared_chan.lock().await.seal_record(&plain)?;
        sink.send(Message::Binary(ct.into())).await?;
    }

    let (out_tx, mut out_rx) = mpsc::channel::<OutboundPayload>(256);
    let (events_tx, _) = broadcast::channel::<ClientInboundEvent>(512);

    let chan_writer = shared_chan.clone();
    tokio::spawn(async move {
        while let Some(item) = out_rx.recv().await {
            let plain = match item {
                OutboundPayload::Wire(w) => match w.encode_zstd(3) {
                    Ok(z) => {
                        let mut p = Vec::with_capacity(1 + z.len());
                        p.push(E2EE_FRAME_WIRE);
                        p.extend_from_slice(&z);
                        p
                    }
                    Err(_) => continue,
                },
                OutboundPayload::Terminal(t) => {
                    let enc = t.encode();
                    let mut p = Vec::with_capacity(1 + enc.len());
                    p.push(E2EE_FRAME_TERMINAL);
                    p.extend_from_slice(&enc);
                    p
                }
            };
            let Ok(ct) = chan_writer.lock().await.seal_record(&plain) else {
                break;
            };
            if sink.send(Message::Binary(ct.into())).await.is_err() {
                break;
            }
        }
    });

    let events_in = events_tx.clone();
    let chan_reader = shared_chan;
    tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Binary(ct) => {
                    let Ok(plain) = chan_reader.lock().await.open_record(&ct) else {
                        // Tampered or replayed record: immediately drop channel!
                        break;
                    };
                    if plain.is_empty() {
                        continue;
                    }
                    match plain[0] {
                        E2EE_FRAME_WIRE => {
                            if let Ok(wire) = WireMessage::decode_zstd(&plain[1..]) {
                                sync_to_replica(replica.as_deref(), &wire);
                                let _ = events_in.send(ClientInboundEvent::Wire(wire));
                            }
                        }
                        E2EE_FRAME_TERMINAL => {
                            if let Ok(term) = TerminalBinaryFrame::decode(&plain[1..]) {
                                let _ = events_in.send(ClientInboundEvent::Terminal(term));
                            }
                        }
                        _ => {}
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        let _ = events_in.send(ClientInboundEvent::Disconnected);
    });

    Ok(HostSessionHandle {
        mode: HostConnectionState::OnlineRelay,
        out_tx,
        events_tx,
    })
}

/// Happy-Eyeballs smart connector: tries LAN/Tailscale direct endpoints with a fast 1.2s probe first,
/// and seamlessly falls back to the private Noise E2EE Relay when away from home/office.
pub async fn connect_smart(
    profile: &SavedHostProfile,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    for ep in &profile.direct_endpoints {
        if let Ok(Ok(handle)) = tokio::time::timeout(
            Duration::from_millis(1200),
            connect_direct(ep, replica.clone()),
        )
        .await
        {
            return Ok(handle);
        }
    }

    if let Some(ref relay_url) = profile.relay_url {
        return connect_via_relay(relay_url, &profile.daemon_pubkey_b64, replica).await;
    }

    Err(ClientCoreError::Unreachable)
}

fn sync_to_replica(store: Option<&OrbitStore>, msg: &WireMessage) {
    let Some(store) = store else { return };
    match msg {
        WireMessage::WorkspacesSnapshot { workspaces, .. } => {
            for ws in workspaces {
                let _ = store.upsert_workspace(ws);
            }
        }
        WireMessage::WorkspaceCreated { workspace, .. } => {
            let _ = store.upsert_workspace(workspace);
        }
        WireMessage::AgentsSnapshot { agents, .. } => {
            for ag in agents {
                let _ = store.upsert_agent(ag, None);
            }
        }
        WireMessage::AgentCreated { agent, .. } | WireMessage::AgentUpdated { agent } => {
            let _ = store.upsert_agent(agent, None);
        }
        WireMessage::TimelineCatchup { events, .. } => {
            for ev in events {
                let _ = store.upsert_replica_timeline_event(ev);
            }
        }
        WireMessage::TimelineLive { event } => {
            let _ = store.upsert_replica_timeline_event(event);
        }
        _ => {}
    }
}

/// Fleet coordinator embedded inside the mobile app via `flutter_rust_bridge`.
pub struct MultiHostFleet {
    hosts: RwLock<HashMap<String, SavedHostProfile>>,
    sessions: RwLock<HashMap<String, HostSessionHandle>>,
    replica_store: Arc<OrbitStore>,
}

impl MultiHostFleet {
    pub fn open(local_db_path: impl AsRef<Path>) -> Result<Self, ClientCoreError> {
        Ok(Self {
            hosts: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            replica_store: Arc::new(OrbitStore::open(local_db_path)?),
        })
    }

    pub fn open_in_memory() -> Result<Self, ClientCoreError> {
        Ok(Self {
            hosts: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            replica_store: Arc::new(OrbitStore::open_in_memory()?),
        })
    }

    /// Pair a new host (e.g. scanning QR from Devbox, Spark workstation, or Mac mini).
    pub fn import_pairing_uri(&self, uri: &str) -> Result<SavedHostProfile, ClientCoreError> {
        let offer = PairingOffer::from_qr_uri(uri)?;
        let profile = SavedHostProfile {
            host_id: offer.host_id.clone(),
            display_name: offer.hostname,
            daemon_pubkey_b64: offer.daemon_pubkey_b64,
            direct_endpoints: offer.direct_endpoints,
            relay_url: offer.relay_url,
            state: HostConnectionState::Disconnected,
        };
        self.hosts
            .write()
            .map_err(|_| ClientCoreError::LockPoisoned)?
            .insert(offer.host_id, profile.clone());
        Ok(profile)
    }

    /// Establish a live connection to a saved host using Happy-Eyeballs routing.
    pub async fn connect_host(&self, host_id: &str) -> Result<HostSessionHandle, ClientCoreError> {
        let profile = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| ClientCoreError::LockPoisoned)?;
            guard
                .get(host_id)
                .cloned()
                .ok_or(ClientCoreError::Unreachable)?
        };
        let handle = connect_smart(&profile, Some(self.replica_store.clone())).await?;
        {
            let mut guard = self
                .hosts
                .write()
                .map_err(|_| ClientCoreError::LockPoisoned)?;
            if let Some(h) = guard.get_mut(host_id) {
                h.state = handle.mode;
            }
        }
        self.sessions
            .write()
            .map_err(|_| ClientCoreError::LockPoisoned)?
            .insert(host_id.to_string(), handle.clone());
        Ok(handle)
    }

    pub fn list_hosts(&self) -> Result<Vec<SavedHostProfile>, ClientCoreError> {
        let guard = self
            .hosts
            .read()
            .map_err(|_| ClientCoreError::LockPoisoned)?;
        Ok(guard.values().cloned().collect())
    }

    pub fn replica_store(&self) -> &Arc<OrbitStore> {
        &self.replica_store
    }
}
