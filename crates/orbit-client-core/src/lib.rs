//! Embedded Multi-Host Rust Client Core for Tauri Desktop, iOS (SwiftUI), Android (Compose), HarmonyOS 7+ (ArkUI), and `orbit` CLI.
//!
//! Provides:
//! - Concurrent Multi-Host fleet management (Devbox, Linux workstation, Mac mini)
//! - Happy-Eyeballs + Dedicated SSH Tunnel + Noise NK E2EE Relay smart routing with 10s Ping keepalive
//! - Zstd wire compression + zero-copy binary terminal framing
//! - `0ms` instant-open local SQLite WAL replica cache with monotonic `since_seq` catch-up
//! - Pure-Rust streaming Markdown AST, `syntect` syntax highlighting, Git diff model, and VT100 terminal grid

pub mod app_core;
pub mod desktop_ops;
pub mod render;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use orbit_crypto::{ClientHandshake, PairingOffer, SecureChannel, decode_key_32};
use orbit_protocol::{PROTOCOL_VERSION, TerminalBinaryFrame, WireMessage};
use orbit_store::{OrbitStore, StoreError};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

use crate::desktop_ops::{SshTunnelGuard, spawn_ssh_local_tunnel};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRouteMode {
    #[default]
    Auto,
    SshTunnel,
    DirectOnly,
    RelayOnly,
}

impl HostRouteMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::SshTunnel => "ssh_tunnel",
            Self::DirectOnly => "direct_only",
            Self::RelayOnly => "relay_only",
        }
    }

    pub fn from_opt_str(raw: Option<&str>) -> Self {
        match raw {
            Some("ssh_tunnel") => Self::SshTunnel,
            Some("direct_only") => Self::DirectOnly,
            Some("relay_only") => Self::RelayOnly,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedHostProfile {
    pub host_id: String,
    pub hostname: String,
    pub custom_name: Option<String>,
    pub ssh_alias: Option<String>,
    pub route_mode: HostRouteMode,
    pub remote_daemon_port: u16,
    pub custom_direct_endpoint: Option<String>,
    pub daemon_pubkey_b64: String,
    pub direct_endpoints: Vec<String>,
    pub relay_url: Option<String>,
    pub state: HostConnectionState,
}

impl SavedHostProfile {
    pub fn effective_display_name(&self) -> String {
        if let Some(ref cn) = self.custom_name
            && !cn.trim().is_empty()
        {
            return cn.trim().to_string();
        }
        if let Some(ref alias) = self.ssh_alias
            && !alias.trim().is_empty()
        {
            return alias.trim().to_string();
        }
        self.hostname.clone()
    }
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
    #[error("no reachable direct endpoint, SSH tunnel, or relay configured")]
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

/// Live bidirectional connection to one Orbit Host (Direct WS, SSH Tunnel WS, or Noise E2EE over Relay).
#[derive(Clone)]
pub struct HostSessionHandle {
    pub mode: HostConnectionState,
    pub via_ssh_tunnel: bool,
    out_tx: mpsc::Sender<OutboundPayload>,
    events_tx: broadcast::Sender<ClientInboundEvent>,
    _ssh_tunnel: Option<Arc<SshTunnelGuard>>,
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

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Connect directly to an Orbit Daemon WebSocket endpoint with Zstd compression & 10s Ping keepalive.
pub async fn connect_direct(
    ws_url: &str,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    connect_direct_with_host_id("", ws_url, replica, None).await
}

async fn connect_direct_with_host_id(
    host_id: &str,
    ws_url: &str,
    replica: Option<Arc<OrbitStore>>,
    ssh_tunnel: Option<Arc<SshTunnelGuard>>,
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

    // 10s WebSocket Ping keepalive so SSH tunnels & NAT routers never drop idle sessions
    let ping_tx = out_tx.clone();
    tokio::spawn(async move {
        let _ = ping_tx
            .send(OutboundPayload::Wire(WireMessage::Ping {
                ts_ms: unix_ms_now(),
            }))
            .await;
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.tick().await;
        loop {
            interval.tick().await;
            if ping_tx
                .send(OutboundPayload::Wire(WireMessage::Ping {
                    ts_ms: unix_ms_now(),
                }))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let tunnel_keep_writer = ssh_tunnel.clone();
    tokio::spawn(async move {
        let _hold = tunnel_keep_writer;
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
    let owned_host_id = host_id.to_string();
    let tunnel_keep_reader = ssh_tunnel.clone();
    tokio::spawn(async move {
        let _hold = tunnel_keep_reader;
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Text(txt) => {
                    if let Ok(wire) = serde_json::from_str::<WireMessage>(&txt) {
                        sync_to_replica(&owned_host_id, replica.as_deref(), &wire);
                        let _ = events_in.send(ClientInboundEvent::Wire(wire));
                    }
                }
                Message::Binary(bin) if !bin.is_empty() => {
                    if bin[0] == DIRECT_BIN_ZSTD_WIRE {
                        if let Ok(wire) = WireMessage::decode_zstd(&bin[1..]) {
                            sync_to_replica(&owned_host_id, replica.as_deref(), &wire);
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
        via_ssh_tunnel: ssh_tunnel.is_some(),
        out_tx,
        events_tx,
        _ssh_tunnel: ssh_tunnel,
    })
}

/// Connect through an untrusted/self-hosted Relay using `Noise_NK_25519_ChaChaPoly_SHA256` E2EE.
pub async fn connect_via_relay(
    relay_base_url: &str,
    daemon_pubkey_b64: &str,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    connect_via_relay_with_host_id("", relay_base_url, daemon_pubkey_b64, replica).await
}

async fn connect_via_relay_with_host_id(
    host_id: &str,
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

    let (hs, msg1) = ClientHandshake::new(&pubkey_bytes)?;
    sink.send(Message::Binary(msg1.into())).await?;

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

    {
        let hello = WireMessage::Hello {
            client_id: Uuid::new_v4().to_string(),
            client_name: "orbit-e2ee".into(),
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

    let ping_tx = out_tx.clone();
    tokio::spawn(async move {
        let _ = ping_tx
            .send(OutboundPayload::Wire(WireMessage::Ping {
                ts_ms: unix_ms_now(),
            }))
            .await;
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.tick().await;
        loop {
            interval.tick().await;
            if ping_tx
                .send(OutboundPayload::Wire(WireMessage::Ping {
                    ts_ms: unix_ms_now(),
                }))
                .await
                .is_err()
            {
                break;
            }
        }
    });

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
    let owned_host_id = host_id.to_string();
    tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Binary(ct) => {
                    let Ok(plain) = chan_reader.lock().await.open_record(&ct) else {
                        break;
                    };
                    if plain.is_empty() {
                        continue;
                    }
                    match plain[0] {
                        E2EE_FRAME_WIRE => {
                            if let Ok(wire) = WireMessage::decode_zstd(&plain[1..]) {
                                sync_to_replica(&owned_host_id, replica.as_deref(), &wire);
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
        via_ssh_tunnel: false,
        out_tx,
        events_tx,
        _ssh_tunnel: None,
    })
}

async fn try_connect_ssh_tunnel(
    profile: &SavedHostProfile,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err(ClientCoreError::Unreachable);
    }
    let Some(alias) = profile.ssh_alias.clone().filter(|s| !s.trim().is_empty()) else {
        return Err(ClientCoreError::Unreachable);
    };
    let port = if profile.remote_daemon_port == 0 {
        6868
    } else {
        profile.remote_daemon_port
    };

    let guard = tokio::task::spawn_blocking(move || spawn_ssh_local_tunnel(&alias, port))
        .await
        .map_err(|e| ClientCoreError::Handshake(format!("SSH 线程启动异常: {e}")))?
        .map_err(ClientCoreError::Handshake)?;

    let shared_guard = Arc::new(guard);
    let mut last_err = None;
    for _ in 0..22 {
        if shared_guard.has_exited() {
            return Err(ClientCoreError::Handshake("SSH 隧道进程意外退出".into()));
        }
        match tokio::time::timeout(
            Duration::from_millis(900),
            connect_direct_with_host_id(
                &profile.host_id,
                &shared_guard.ws_url,
                replica.clone(),
                Some(shared_guard.clone()),
            ),
        )
        .await
        {
            Ok(Ok(handle)) => return Ok(handle),
            Ok(Err(e)) => last_err = Some(e),
            Err(_) => {}
        }
        tokio::time::sleep(Duration::from_millis(220)).await;
    }

    Err(last_err.unwrap_or_else(|| ClientCoreError::Handshake("SSH 端口转发连接超时".into())))
}

async fn try_connect_direct_endpoints(
    profile: &SavedHostProfile,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    let mut endpoints = Vec::new();
    if let Some(ref custom) = profile.custom_direct_endpoint
        && !custom.trim().is_empty()
    {
        endpoints.push(custom.trim().to_string());
    }
    endpoints.extend(profile.direct_endpoints.iter().cloned());

    for ep in &endpoints {
        if let Ok(Ok(handle)) = tokio::time::timeout(
            Duration::from_millis(1200),
            connect_direct_with_host_id(&profile.host_id, ep, replica.clone(), None),
        )
        .await
        {
            return Ok(handle);
        }
    }
    Err(ClientCoreError::Unreachable)
}

/// Smart connector supporting `HostRouteMode` (`Auto`, `SshTunnel`, `DirectOnly`, `RelayOnly`):
/// - When an `ssh_alias` (`devbox`) is configured on Desktop in `Auto` mode, connects via dedicated SSH tunnel FIRST!
/// - Otherwise probes Direct WS -> SSH Tunnel -> Noise E2EE Relay.
pub async fn connect_smart(
    profile: &SavedHostProfile,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    let has_ssh = cfg!(any(target_os = "macos", target_os = "linux"))
        && profile
            .ssh_alias
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

    match profile.route_mode {
        HostRouteMode::SshTunnel => try_connect_ssh_tunnel(profile, replica).await,
        HostRouteMode::DirectOnly => try_connect_direct_endpoints(profile, replica).await,
        HostRouteMode::RelayOnly => {
            if let Some(ref relay_url) = profile.relay_url {
                connect_via_relay_with_host_id(
                    &profile.host_id,
                    relay_url,
                    &profile.daemon_pubkey_b64,
                    replica,
                )
                .await
            } else {
                Err(ClientCoreError::Handshake("尚未配置 E2EE 中继地址".into()))
            }
        }
        HostRouteMode::Auto => {
            let mut last_ssh_err = None;
            if has_ssh && profile.custom_direct_endpoint.is_none() {
                match try_connect_ssh_tunnel(profile, replica.clone()).await {
                    Ok(h) => return Ok(h),
                    Err(e) => last_ssh_err = Some(e),
                }
                if let Ok(h) = try_connect_direct_endpoints(profile, replica.clone()).await {
                    return Ok(h);
                }
            } else {
                if let Ok(h) = try_connect_direct_endpoints(profile, replica.clone()).await {
                    return Ok(h);
                }
                if has_ssh {
                    match try_connect_ssh_tunnel(profile, replica.clone()).await {
                        Ok(h) => return Ok(h),
                        Err(e) => last_ssh_err = Some(e),
                    }
                }
            }

            if let Some(ref relay_url) = profile.relay_url {
                return connect_via_relay_with_host_id(
                    &profile.host_id,
                    relay_url,
                    &profile.daemon_pubkey_b64,
                    replica,
                )
                .await;
            }

            Err(last_ssh_err.unwrap_or(ClientCoreError::Unreachable))
        }
    }
}

fn sync_to_replica(host_id: &str, store: Option<&OrbitStore>, msg: &WireMessage) {
    let Some(store) = store else { return };
    match msg {
        WireMessage::WorkspacesSnapshot { workspaces, .. } => {
            for ws in workspaces {
                let _ = store.upsert_workspace(ws);
            }
            if !host_id.is_empty() {
                let _ = store.upsert_replica_workspaces(host_id, workspaces);
            }
        }
        WireMessage::WorkspaceCreated { workspace, .. } => {
            let _ = store.upsert_workspace(workspace);
        }
        WireMessage::AgentsSnapshot { agents, .. } => {
            for ag in agents {
                let _ = store.upsert_agent(ag, None);
            }
            if !host_id.is_empty() {
                let _ = store.upsert_replica_agents(host_id, agents);
            }
        }
        WireMessage::AgentCreated { agent, .. } | WireMessage::AgentUpdated { agent } => {
            let _ = store.upsert_agent(agent, None);
            if !host_id.is_empty() {
                let _ = store.upsert_single_replica_agent(host_id, agent);
            }
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

/// Fleet coordinator embedded inside standalone CLI/mobile consumers.
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

    pub fn import_pairing_uri(&self, uri: &str) -> Result<SavedHostProfile, ClientCoreError> {
        let offer = PairingOffer::from_qr_uri(uri)?;
        let profile = SavedHostProfile {
            host_id: offer.host_id.clone(),
            hostname: offer.hostname,
            custom_name: None,
            ssh_alias: None,
            route_mode: HostRouteMode::Auto,
            remote_daemon_port: 6868,
            custom_direct_endpoint: None,
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
