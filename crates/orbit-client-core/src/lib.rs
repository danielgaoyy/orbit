//! Embedded Multi-Host Rust Client Core for Tauri Desktop, iOS (SwiftUI), Android (Compose), HarmonyOS 7+ (ArkUI), and `orbit` CLI.
//!
//! Provides:
//! - Concurrent Multi-Host fleet management (Devbox, Linux workstation, Mac mini)
//! - Happy-Eyeballs + Dedicated SSH Tunnel + Noise NK E2EE Relay smart routing with 10s Ping keepalive
//! - Zstd wire compression + zero-copy binary terminal framing
//! - `0ms` instant-open local SQLite WAL replica cache with monotonic `since_seq` catch-up
//! - Pure-Rust streaming Markdown AST, `syntect` syntax highlighting, Git diff model, and VT100 terminal grid

pub mod app_core;
pub mod catalog;
pub mod cloud_sync;
pub mod composer;
pub mod contract;
pub mod desktop_ops;
pub mod diagram;
pub mod export;
pub mod logging;
pub mod math;
pub mod render;
pub mod sidebar;
pub mod stage;
pub mod subagents;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use orbit_crypto::{ClientHandshake, PairingOffer, SecureChannel, decode_key_32};
use orbit_protocol::{HostInfo, PROTOCOL_VERSION, TerminalBinaryFrame, WireMessage};
use orbit_relay_transport::connect_relay_ws;
use orbit_store::{OrbitStore, StoreError};
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        Error as WsError, Message,
        client::IntoClientRequest,
        handshake::client::Request as WsClientRequest,
        http::{HeaderValue, header::AUTHORIZATION},
        protocol::{WebSocketConfig, frame::coding::CloseCode},
    },
};
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
    #[serde(default)]
    pub pairing_secret: String,
    #[serde(default)]
    pub pairing_secret_updated_at_ms: i64,
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
pub(crate) enum OutboundPayload {
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
    #[error("cloud sync error: {0}")]
    CloudSync(#[from] crate::cloud_sync::CloudSyncError),
    #[error("websocket error: {0}")]
    Ws(Box<tokio_tungstenite::tungstenite::Error>),
    #[error("relay transport error: {0}")]
    RelayTransport(#[from] orbit_relay_transport::RelayTransportError),
    #[error("connection handshake failed: {0}")]
    Handshake(String),
    #[error("authentication failed: {0}")]
    Unauthorized(String),
    #[error("host identity mismatch (expected {expected}, got {actual})")]
    HostIdentityMismatch { expected: String, actual: String },
    #[error("no reachable direct endpoint, SSH tunnel, or relay configured")]
    Unreachable,
    #[error("channel closed")]
    ChannelClosed,
    #[error("lock poisoned")]
    LockPoisoned,
    #[error("{0}")]
    Transport(String),
}

impl ClientCoreError {
    /// Returns `true` when the connection failed due to missing, invalid, or rejected
    /// `pairing_secret` authentication (Direct WS 401/403 or Relay E2EE `Noise_NKpsk0`).
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            Self::Unauthorized(_)
                | Self::Crypto(
                    orbit_crypto::CryptoError::InvalidPairingSecret
                        | orbit_crypto::CryptoError::AuthenticationFailed
                )
        ) || matches!(
            self,
            Self::Ws(ws_err)
                if matches!(ws_err.as_ref(), WsError::Http(resp) if matches!(resp.status().as_u16(), 401 | 403))
        )
    }
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
    pub welcome_host: Option<HostInfo>,
    out_tx: mpsc::Sender<OutboundPayload>,
    events_tx: broadcast::Sender<ClientInboundEvent>,
    _ssh_tunnel: Option<Arc<SshTunnelGuard>>,
}

impl HostSessionHandle {
    #[cfg(test)]
    pub(crate) fn new_test_channel() -> (Self, mpsc::Receiver<OutboundPayload>) {
        let (out_tx, out_rx) = mpsc::channel(32);
        let (events_tx, _) = broadcast::channel(16);
        let handle = Self {
            mode: HostConnectionState::OnlineDirect,
            via_ssh_tunnel: false,
            welcome_host: None,
            out_tx,
            events_tx,
            _ssh_tunnel: None,
        };
        (handle, out_rx)
    }

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

/// Build an authenticated WebSocket upgrade request carrying `Authorization: Bearer <pairing_secret>`.
pub fn build_authenticated_ws_request(
    ws_url: &str,
    pairing_secret: &str,
) -> Result<WsClientRequest, ClientCoreError> {
    let secret = pairing_secret.trim();
    if secret.is_empty() {
        return Err(ClientCoreError::Unauthorized(
            "missing pairing_secret for direct WebSocket authentication".into(),
        ));
    }
    let mut req = ws_url.into_client_request()?;
    let header_val = HeaderValue::from_str(&format!("Bearer {secret}")).map_err(|e| {
        ClientCoreError::Unauthorized(format!("invalid pairing_secret header: {e}"))
    })?;
    req.headers_mut().insert(AUTHORIZATION, header_val);
    Ok(req)
}

/// Connect directly to an Orbit Daemon WebSocket endpoint with Bearer auth, Zstd compression & 10s Ping keepalive.
pub async fn connect_direct(
    ws_url: &str,
    pairing_secret: &str,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    connect_direct_with_host_id("", ws_url, pairing_secret, replica, None).await
}

/// Connect directly to an Orbit Daemon WebSocket endpoint and verify `Welcome.host_id` matches `expected_host_id`.
pub async fn connect_direct_verified(
    expected_host_id: &str,
    ws_url: &str,
    pairing_secret: &str,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    connect_direct_with_host_id(expected_host_id, ws_url, pairing_secret, replica, None).await
}

async fn connect_direct_with_host_id(
    host_id: &str,
    ws_url: &str,
    pairing_secret: &str,
    replica: Option<Arc<OrbitStore>>,
    ssh_tunnel: Option<Arc<SshTunnelGuard>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    let req = build_authenticated_ws_request(ws_url, pairing_secret)?;
    let mut ws_cfg = WebSocketConfig::default();
    ws_cfg.max_frame_size = Some(orbit_protocol::MAX_WS_FRAME_BYTES);
    ws_cfg.max_message_size = Some(orbit_protocol::MAX_WS_FRAME_BYTES);
    let (ws_stream, _) = connect_async_with_config(req, Some(ws_cfg), false).await?;
    let (mut sink, mut stream) = ws_stream.split();

    let hello = WireMessage::Hello {
        client_id: Uuid::new_v4().to_string(),
        client_name: "orbit-client".into(),
        protocol_version: PROTOCOL_VERSION,
        supports_zstd: true,
    };
    let hello_json = serde_json::to_string(&hello).map_err(orbit_protocol::ProtocolError::from)?;
    sink.send(Message::Text(hello_json.into())).await?;

    // Verify `WireMessage::Welcome` BEFORE syncing any state so two users sharing a machine
    // can NEVER cross-connect or contaminate each other's local session replica!
    let mut welcome_host: Option<HostInfo> = None;
    let welcome_deadline = tokio::time::Instant::now() + Duration::from_millis(2500);
    while welcome_host.is_none() {
        let remaining = welcome_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(ClientCoreError::Handshake(
                "timed out waiting for daemon Welcome".into(),
            ));
        }
        let next_msg = tokio::time::timeout(remaining, stream.next())
            .await
            .map_err(|_| {
                ClientCoreError::Handshake("timed out waiting for daemon Welcome".into())
            })?;
        match next_msg {
            Some(Ok(Message::Text(txt))) => {
                if let Ok(WireMessage::Welcome { host }) = serde_json::from_str::<WireMessage>(&txt)
                {
                    welcome_host = Some(host);
                }
            }
            Some(Ok(Message::Binary(bin))) if !bin.is_empty() && bin[0] == DIRECT_BIN_ZSTD_WIRE => {
                if let Ok(WireMessage::Welcome { host }) = WireMessage::decode_zstd(&bin[1..]) {
                    welcome_host = Some(host);
                }
            }
            Some(Ok(Message::Close(_))) | None => {
                return Err(ClientCoreError::ChannelClosed);
            }
            Some(Err(e)) => return Err(e.into()),
            _ => {}
        }
    }

    if let Some(ref host_info) = welcome_host
        && !host_id.is_empty()
        && host_info.host_id != host_id
    {
        let _ = sink.close().await;
        return Err(ClientCoreError::HostIdentityMismatch {
            expected: host_id.to_string(),
            actual: host_info.host_id.clone(),
        });
    }

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
                Message::Text(txt) => match serde_json::from_str::<WireMessage>(&txt) {
                    Ok(wire) => {
                        sync_to_replica(&owned_host_id, replica.as_deref(), &wire).await;
                        let _ = events_in.send(ClientInboundEvent::Wire(wire));
                    }
                    Err(err) => {
                        tracing::warn!(host_id = %owned_host_id, error = %err, bytes = txt.len(), "failed to decode direct JSON wire message");
                    }
                },
                Message::Binary(bin) if !bin.is_empty() => {
                    if bin[0] == DIRECT_BIN_ZSTD_WIRE {
                        match WireMessage::decode_zstd(&bin[1..]) {
                            Ok(wire) => {
                                sync_to_replica(&owned_host_id, replica.as_deref(), &wire).await;
                                let _ = events_in.send(ClientInboundEvent::Wire(wire));
                            }
                            Err(err) => {
                                tracing::warn!(host_id = %owned_host_id, error = %err, compressed_bytes = bin.len(), "failed to decode direct zstd wire message");
                            }
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
        welcome_host,
        out_tx,
        events_tx,
        _ssh_tunnel: ssh_tunnel,
    })
}

/// Connect through an untrusted/self-hosted Relay using mutually authenticated
/// `Noise_NKpsk0_25519_ChaChaPoly_SHA256` E2EE with the host's `pairing_secret`.
pub async fn connect_via_relay(
    relay_base_url: &str,
    daemon_pubkey_b64: &str,
    pairing_secret: &str,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    connect_via_relay_with_host_id(
        "",
        relay_base_url,
        daemon_pubkey_b64,
        pairing_secret,
        replica,
    )
    .await
}

async fn connect_via_relay_with_host_id(
    host_id: &str,
    relay_base_url: &str,
    daemon_pubkey_b64: &str,
    pairing_secret: &str,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    // Reject empty or malformed pairing_secret fail-closed BEFORE dialing the relay.
    let _ = orbit_crypto::derive_e2ee_psk(pairing_secret).map_err(|_| {
        ClientCoreError::Unauthorized(
            "missing or invalid pairing_secret for relay E2EE authentication".into(),
        )
    })?;
    let pubkey_bytes = decode_key_32(daemon_pubkey_b64)?;
    let (hs, msg1) = ClientHandshake::new(&pubkey_bytes, pairing_secret).map_err(|e| match e {
        orbit_crypto::CryptoError::InvalidPairingSecret
        | orbit_crypto::CryptoError::AuthenticationFailed => {
            ClientCoreError::Unauthorized(e.to_string())
        }
        other => ClientCoreError::Crypto(other),
    })?;
    let path_and_query = format!("/v1/client/{daemon_pubkey_b64}");

    let ws_stream = connect_relay_ws(relay_base_url, &path_and_query)
        .await
        .map_err(|err| {
            let detail = err.to_string();
            if detail.contains("502") {
                ClientCoreError::Handshake(
                    "Host daemon is not registered on Relay (502) — ensure the host daemon is running with this Relay configured".into(),
                )
            } else if detail.contains("403") {
                ClientCoreError::Handshake(
                    "Relay rejected host public key (403 Forbidden) — authorize this host on the Relay".into(),
                )
            } else {
                ClientCoreError::RelayTransport(err)
            }
        })?;
    let (mut sink, mut stream) = ws_stream.split();

    sink.send(Message::Binary(msg1.into())).await?;

    let msg2 = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(item) = stream.next().await {
            match item {
                Ok(Message::Binary(b)) => return Ok(b),
                Ok(Message::Ping(p)) => {
                    sink.send(Message::Pong(p)).await?;
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(frame)) => {
                    let is_unauthorized = frame.as_ref().is_some_and(|f| {
                        f.code == CloseCode::Policy || f.reason.eq_ignore_ascii_case("unauthorized")
                    });
                    if is_unauthorized {
                        return Err(ClientCoreError::Unauthorized(
                            "relay E2EE authentication rejected by daemon".into(),
                        ));
                    }
                    return Err(ClientCoreError::Handshake(
                        "relay closed during E2EE handshake".into(),
                    ));
                }
                Ok(_) => {
                    return Err(ClientCoreError::Handshake(
                        "unexpected frame during relay E2EE handshake".into(),
                    ));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(ClientCoreError::Handshake(
            "relay closed during E2EE handshake".into(),
        ))
    })
    .await
    .map_err(|_| ClientCoreError::Handshake("relay E2EE handshake timed out".into()))??;

    let write_chan: SecureChannel = hs.finalize(&msg2).map_err(|e| match e {
        orbit_crypto::CryptoError::InvalidPairingSecret
        | orbit_crypto::CryptoError::AuthenticationFailed => {
            ClientCoreError::Unauthorized(e.to_string())
        }
        other => ClientCoreError::Crypto(other),
    })?;

    let (ct_out_tx, mut ct_out_rx) = mpsc::channel::<Vec<u8>>(256);
    let (ct_in_tx, ct_in_rx) = mpsc::channel::<Vec<u8>>(256);

    tokio::spawn(async move {
        while let Some(ct) = ct_out_rx.recv().await {
            if sink.send(Message::Binary(ct.into())).await.is_err() {
                break;
            }
        }
    });

    tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                Message::Binary(ct) => {
                    if ct_in_tx.send(ct.to_vec()).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    complete_e2ee_session_tasks(host_id, write_chan, ct_out_tx, ct_in_rx, replica, None).await
}

/// Connect to a remote LAN-isolated Daemon by tunneling end-to-end encrypted (`Noise_NKpsk0`)
/// frames through an already-online Peer Bridge Daemon session.
///
/// Security: The bridge Daemon ONLY forwards opaque encrypted bytes via `PeerBridgeOpen` /
/// `PeerBridgeData` and cannot decrypt credentials, prompts, or filesystem contents.
pub async fn connect_via_peer_bridge(
    bridge_session: &HostSessionHandle,
    target_host_id: &str,
    target_pubkey_b64: &str,
    target_pairing_secret: &str,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let _ = orbit_crypto::derive_e2ee_psk(target_pairing_secret).map_err(|_| {
        ClientCoreError::Unauthorized(
            "missing or invalid pairing_secret for peer bridge E2EE authentication".into(),
        )
    })?;
    let pubkey_bytes = decode_key_32(target_pubkey_b64)?;
    let (hs, msg1) =
        ClientHandshake::new(&pubkey_bytes, target_pairing_secret).map_err(|e| match e {
            orbit_crypto::CryptoError::InvalidPairingSecret
            | orbit_crypto::CryptoError::AuthenticationFailed => {
                ClientCoreError::Unauthorized(e.to_string())
            }
            other => ClientCoreError::Crypto(other),
        })?;

    let mut bridge_events = bridge_session.subscribe();
    let request_id = Uuid::new_v4();
    let bridge_id = Uuid::new_v4();
    bridge_session
        .send_wire(WireMessage::PeerBridgeOpen {
            request_id,
            bridge_id,
            target_host_id: target_host_id.to_string(),
        })
        .await?;

    // Wait for bridge Daemon to open `/ws/e2ee` stream to target host
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match bridge_events.recv().await {
                Ok(ClientInboundEvent::Wire(WireMessage::PeerBridgeOpened {
                    request_id: r,
                    bridge_id: b,
                })) if r == request_id && b == bridge_id => return Ok(()),
                Ok(ClientInboundEvent::Wire(WireMessage::RpcError {
                    request_id: Some(r),
                    code,
                    message,
                })) if r == request_id => {
                    return Err(ClientCoreError::Handshake(format!("{code}: {message}")));
                }
                Ok(ClientInboundEvent::Disconnected) | Err(_) => {
                    return Err(ClientCoreError::Unreachable);
                }
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| ClientCoreError::Handshake("timed out opening peer bridge tunnel".into()))??;

    // Send Noise_NKpsk0 msg1 directly to target Daemon
    bridge_session
        .send_wire(WireMessage::PeerBridgeData {
            bridge_id,
            payload_b64: STANDARD.encode(&msg1),
        })
        .await?;

    // Await Noise msg2 from target Daemon via PeerBridgeData
    let msg2 = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match bridge_events.recv().await {
                Ok(ClientInboundEvent::Wire(WireMessage::PeerBridgeData {
                    bridge_id: b,
                    payload_b64,
                })) if b == bridge_id => {
                    return STANDARD.decode(&payload_b64).map_err(|e| {
                        ClientCoreError::Handshake(format!("invalid base64 peer bridge msg2: {e}"))
                    });
                }
                Ok(ClientInboundEvent::Wire(WireMessage::PeerBridgeClosed {
                    bridge_id: b,
                    reason,
                })) if b == bridge_id => {
                    let is_unauth = reason
                        .as_deref()
                        .is_some_and(|r| r.to_ascii_lowercase().contains("unauthorized"));
                    if is_unauth {
                        return Err(ClientCoreError::Unauthorized(
                            "peer bridge E2EE authentication rejected by target daemon".into(),
                        ));
                    }
                    return Err(ClientCoreError::Handshake(format!(
                        "peer bridge closed during E2EE handshake: {}",
                        reason.unwrap_or_else(|| "closed".to_string())
                    )));
                }
                Ok(ClientInboundEvent::Disconnected) | Err(_) => {
                    return Err(ClientCoreError::Handshake(
                        "bridge disconnected during peer E2EE handshake".into(),
                    ));
                }
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| ClientCoreError::Handshake("peer bridge E2EE handshake timed out".into()))??;

    let write_chan: SecureChannel = hs.finalize(&msg2).map_err(|e| match e {
        orbit_crypto::CryptoError::InvalidPairingSecret
        | orbit_crypto::CryptoError::AuthenticationFailed => {
            ClientCoreError::Unauthorized(e.to_string())
        }
        other => ClientCoreError::Crypto(other),
    })?;

    let (ct_out_tx, mut ct_out_rx) = mpsc::channel::<Vec<u8>>(256);
    let (ct_in_tx, ct_in_rx) = mpsc::channel::<Vec<u8>>(256);

    let writer_bridge = bridge_session.clone();
    tokio::spawn(async move {
        while let Some(ct) = ct_out_rx.recv().await {
            if writer_bridge
                .send_wire(WireMessage::PeerBridgeData {
                    bridge_id,
                    payload_b64: STANDARD.encode(&ct),
                })
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = writer_bridge
            .send_wire(WireMessage::PeerBridgeClosed {
                bridge_id,
                reason: None,
            })
            .await;
    });

    tokio::spawn(async move {
        loop {
            match bridge_events.recv().await {
                Ok(ClientInboundEvent::Wire(WireMessage::PeerBridgeData {
                    bridge_id: b,
                    payload_b64,
                })) if b == bridge_id => {
                    if let Ok(bytes) = STANDARD.decode(&payload_b64)
                        && ct_in_tx.send(bytes).await.is_err()
                    {
                        break;
                    }
                }
                Ok(ClientInboundEvent::Wire(WireMessage::PeerBridgeClosed {
                    bridge_id: b,
                    ..
                })) if b == bridge_id => break,
                Ok(ClientInboundEvent::Disconnected)
                | Err(tokio::sync::broadcast::error::RecvError::Closed)
                | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                Ok(_) => {}
            }
        }
    });

    complete_e2ee_session_tasks(
        target_host_id,
        write_chan,
        ct_out_tx,
        ct_in_rx,
        replica,
        Some(bridge_session.clone()),
    )
    .await
}

async fn complete_e2ee_session_tasks(
    host_id: &str,
    write_chan: SecureChannel,
    ct_out_tx: mpsc::Sender<Vec<u8>>,
    mut ct_in_rx: mpsc::Receiver<Vec<u8>>,
    replica: Option<Arc<OrbitStore>>,
    hold_bridge_session: Option<HostSessionHandle>,
) -> Result<HostSessionHandle, ClientCoreError> {
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
        ct_out_tx
            .send(ct)
            .await
            .map_err(|_| ClientCoreError::ChannelClosed)?;
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
        let _keep_bridge = hold_bridge_session;
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
            if ct_out_tx.send(ct).await.is_err() {
                break;
            }
        }
    });

    let events_in = events_tx.clone();
    let chan_reader = shared_chan;
    let owned_host_id = host_id.to_string();
    tokio::spawn(async move {
        while let Some(ct) = ct_in_rx.recv().await {
            let Ok(plain) = chan_reader.lock().await.open_record(&ct) else {
                break;
            };
            if plain.is_empty() {
                continue;
            }
            match plain[0] {
                E2EE_FRAME_WIRE => match WireMessage::decode_zstd(&plain[1..]) {
                    Ok(wire) => {
                        sync_to_replica(&owned_host_id, replica.as_deref(), &wire).await;
                        let _ = events_in.send(ClientInboundEvent::Wire(wire));
                    }
                    Err(err) => {
                        tracing::warn!(host_id = %owned_host_id, error = %err, compressed_bytes = plain.len(), "failed to decode E2EE zstd wire message");
                    }
                },
                E2EE_FRAME_TERMINAL => {
                    if let Ok(term) = TerminalBinaryFrame::decode(&plain[1..]) {
                        let _ = events_in.send(ClientInboundEvent::Terminal(term));
                    }
                }
                _ => {}
            }
        }
        let _ = events_in.send(ClientInboundEvent::Disconnected);
    });

    Ok(HostSessionHandle {
        mode: HostConnectionState::OnlineRelay,
        via_ssh_tunnel: false,
        welcome_host: None,
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
        .map_err(|e| ClientCoreError::Handshake(format!("Failed to spawn SSH task: {e}")))?
        .map_err(ClientCoreError::Handshake)?;

    let shared_guard = Arc::new(guard);
    let mut last_err = None;
    for _ in 0..45 {
        if shared_guard.has_exited() {
            let detail = shared_guard
                .take_stderr_summary()
                .unwrap_or_else(|| "SSH tunnel exited unexpectedly".into());
            return Err(ClientCoreError::Handshake(detail));
        }
        match tokio::time::timeout(
            Duration::from_millis(3000),
            connect_direct_with_host_id(
                &profile.host_id,
                &shared_guard.ws_url,
                &profile.pairing_secret,
                replica.clone(),
                Some(shared_guard.clone()),
            ),
        )
        .await
        {
            Ok(Ok(handle)) => return Ok(handle),
            Ok(Err(e @ ClientCoreError::HostIdentityMismatch { .. })) => return Err(e),
            Ok(Err(e)) if e.is_auth_error() => return Err(e),
            Ok(Err(e)) => last_err = Some(e),
            Err(_) => {}
        }
        tokio::time::sleep(Duration::from_millis(220)).await;
    }

    Err(last_err.unwrap_or_else(|| {
        ClientCoreError::Handshake("SSH port-forward connection timed out".into())
    }))
}

fn endpoint_priority_tier(ep: &str) -> u8 {
    let authority = ep
        .trim()
        .strip_prefix("ws://")
        .or_else(|| ep.strip_prefix("wss://"))
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    if host == "127.0.0.1" || host == "10.0.2.2" || host.eq_ignore_ascii_case("localhost") {
        0
    } else if host.starts_with("198.18.") || host.starts_with("198.19.") {
        // RFC 2544 / Clash / Surge fake-IP TUN interfaces often accept TCP SYN then blackhole WS handshakes
        3
    } else if host.starts_with("192.168.139.") || host.starts_with("192.168.215.") {
        // Common local VM / bridge virtual subnets
        2
    } else {
        1
    }
}

fn has_custom_direct_endpoint(profile: &SavedHostProfile) -> bool {
    profile
        .custom_direct_endpoint
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
}

async fn try_connect_direct_endpoints(
    profile: &SavedHostProfile,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    let has_custom = has_custom_direct_endpoint(profile);
    // Only dial explicitly paired/configured direct endpoints for this host — never speculatively
    // probe unconfigured local/emulator addresses with this host's Bearer credential!
    let mut raw_endpoints: Vec<String> = Vec::new();
    if let Some(ref custom) = profile.custom_direct_endpoint
        && !custom.trim().is_empty()
    {
        raw_endpoints.push(custom.trim().to_string());
    }
    raw_endpoints.extend(profile.direct_endpoints.iter().cloned());

    // Deduplicate while preserving order
    let mut endpoints: Vec<String> = Vec::with_capacity(raw_endpoints.len());
    for ep in raw_endpoints {
        if !endpoints.contains(&ep) {
            endpoints.push(ep);
        }
    }
    if !has_custom {
        endpoints.sort_by_key(|ep| endpoint_priority_tier(ep));
    }
    if endpoints.is_empty() {
        return Err(ClientCoreError::Unreachable);
    }

    // Happy-Eyeballs concurrent racing (60ms stagger) so dead TUN/VM adapters never block LAN Wi-Fi
    let mut set = tokio::task::JoinSet::new();
    for (idx, ep) in endpoints.into_iter().enumerate() {
        let host_id = profile.host_id.clone();
        let pairing_secret = profile.pairing_secret.clone();
        let rep = replica.clone();
        let stagger_ms = (idx as u64) * 60;
        set.spawn(async move {
            if stagger_ms > 0 {
                tokio::time::sleep(Duration::from_millis(stagger_ms)).await;
            }
            tokio::time::timeout(
                Duration::from_millis(1500),
                connect_direct_with_host_id(&host_id, &ep, &pairing_secret, rep, None),
            )
            .await
        });
    }

    let mut mismatch_err = None;
    let mut auth_err = None;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Ok(Ok(handle))) => {
                set.abort_all();
                return Ok(handle);
            }
            Ok(Ok(Err(e @ ClientCoreError::HostIdentityMismatch { .. }))) => {
                mismatch_err = Some(e);
            }
            Ok(Ok(Err(e))) if e.is_auth_error() => {
                auth_err = Some(e);
            }
            _ => {}
        }
    }
    Err(mismatch_err
        .or(auth_err)
        .unwrap_or(ClientCoreError::Unreachable))
}

async fn connect_auto(
    profile: &SavedHostProfile,
    replica: Option<Arc<OrbitStore>>,
    has_ssh: bool,
) -> Result<HostSessionHandle, ClientCoreError> {
    let mut last_ssh_err = None;
    let mut auth_err = None;
    let mut mismatch_err = None;
    if has_ssh && !has_custom_direct_endpoint(profile) {
        match try_connect_ssh_tunnel(profile, replica.clone()).await {
            Ok(h) => return Ok(h),
            Err(e @ ClientCoreError::HostIdentityMismatch { .. }) => {
                mismatch_err = Some(e);
            }
            Err(e) if e.is_auth_error() => auth_err = Some(e),
            Err(e) => last_ssh_err = Some(e),
        }
        match try_connect_direct_endpoints(profile, replica.clone()).await {
            Ok(h) => return Ok(h),
            Err(e @ ClientCoreError::HostIdentityMismatch { .. }) => {
                mismatch_err = Some(e);
            }
            Err(e) if e.is_auth_error() => auth_err = Some(e),
            Err(_) => {}
        }
    } else {
        match try_connect_direct_endpoints(profile, replica.clone()).await {
            Ok(h) => return Ok(h),
            Err(e @ ClientCoreError::HostIdentityMismatch { .. }) => {
                mismatch_err = Some(e);
            }
            Err(e) if e.is_auth_error() => auth_err = Some(e),
            Err(_) => {}
        }
        if has_ssh {
            match try_connect_ssh_tunnel(profile, replica.clone()).await {
                Ok(h) => return Ok(h),
                Err(e @ ClientCoreError::HostIdentityMismatch { .. }) => {
                    mismatch_err = Some(e);
                }
                Err(e) if e.is_auth_error() => auth_err = Some(e),
                Err(e) => last_ssh_err = Some(e),
            }
        }
    }

    if let Some(ref relay_url) = profile.relay_url {
        match connect_via_relay_with_host_id(
            &profile.host_id,
            relay_url,
            &profile.daemon_pubkey_b64,
            &profile.pairing_secret,
            replica,
        )
        .await
        {
            Ok(h) => return Ok(h),
            Err(e) if e.is_auth_error() => return Err(e),
            Err(e) => return Err(mismatch_err.or(auth_err).unwrap_or(e)),
        }
    }

    Err(mismatch_err
        .or(auth_err)
        .or(last_ssh_err)
        .unwrap_or(ClientCoreError::Unreachable))
}

/// Smart connector supporting `HostRouteMode` (`Auto`, `SshTunnel`, `DirectOnly`, `RelayOnly`):
/// - When an `ssh_alias` (`devbox`) is configured on Desktop in `Auto` mode, connects via dedicated SSH tunnel FIRST!
/// - On mobile (`!macos && !linux`), `SshTunnel` degrades gracefully to the `Auto` route (Direct -> E2EE Relay).
/// - Otherwise probes Direct WS -> SSH Tunnel -> Noise E2EE Relay.
pub async fn connect_smart(
    profile: &SavedHostProfile,
    replica: Option<Arc<OrbitStore>>,
) -> Result<HostSessionHandle, ClientCoreError> {
    let desktop_ssh_supported = cfg!(any(target_os = "macos", target_os = "linux"));
    let has_ssh = desktop_ssh_supported
        && profile
            .ssh_alias
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

    match profile.route_mode {
        HostRouteMode::SshTunnel => {
            if desktop_ssh_supported {
                try_connect_ssh_tunnel(profile, replica).await
            } else {
                connect_auto(profile, replica, false).await
            }
        }
        HostRouteMode::DirectOnly => try_connect_direct_endpoints(profile, replica).await,
        HostRouteMode::RelayOnly => {
            if let Some(ref relay_url) = profile.relay_url {
                connect_via_relay_with_host_id(
                    &profile.host_id,
                    relay_url,
                    &profile.daemon_pubkey_b64,
                    &profile.pairing_secret,
                    replica,
                )
                .await
            } else {
                Err(ClientCoreError::Handshake(
                    "No E2EE relay URL configured".into(),
                ))
            }
        }
        HostRouteMode::Auto => connect_auto(profile, replica, has_ssh).await,
    }
}

async fn retry_replica_write<F>(mut op: F) -> Result<(), orbit_store::StoreError>
where
    F: FnMut() -> Result<(), orbit_store::StoreError>,
{
    const BACKOFF_MS: [u64; 2] = [25, 75];
    let mut attempt = 0usize;
    loop {
        match op() {
            Ok(()) => return Ok(()),
            Err(err) if err.is_busy() && attempt < BACKOFF_MS.len() => {
                let delay = Duration::from_millis(BACKOFF_MS[attempt]);
                attempt += 1;
                tokio::time::sleep(delay).await;
            }
            Err(err) => return Err(err),
        }
    }
}

async fn sync_to_replica(host_id: &str, store: Option<&OrbitStore>, msg: &WireMessage) {
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
        WireMessage::WorkspaceCreated { workspace, .. }
        | WireMessage::WorkspaceUpdated { workspace } => {
            let _ = store.upsert_workspace(workspace);
            if !host_id.is_empty() {
                let _ = store.upsert_single_replica_workspace(host_id, workspace);
            }
        }
        WireMessage::AgentsSnapshot { agents, .. } => {
            if !host_id.is_empty() {
                let _ = store.upsert_replica_agents(host_id, agents);
            } else {
                for ag in agents {
                    let _ = store.upsert_agent(ag, None);
                }
            }
        }
        WireMessage::AgentCreated { agent, .. } | WireMessage::AgentUpdated { agent } => {
            if !host_id.is_empty() {
                let _ = store.upsert_single_replica_agent(host_id, agent);
            } else {
                let _ = store.upsert_agent(agent, None);
            }
        }
        WireMessage::AgentForked {
            agent, workspace, ..
        } => {
            if let Some(ws) = workspace {
                let _ = store.upsert_workspace(ws);
                if !host_id.is_empty() {
                    let _ = store.upsert_single_replica_workspace(host_id, ws);
                }
            }
            if !host_id.is_empty() {
                let _ = store.upsert_single_replica_agent(host_id, agent);
            } else {
                let _ = store.upsert_agent(agent, None);
            }
        }
        WireMessage::AgentDeleted { agent_id } => {
            if !host_id.is_empty() {
                let _ = store.delete_replica_agent(host_id, agent_id);
            }
        }
        WireMessage::TimelineCatchup {
            agent_id,
            events,
            has_older_history,
            ..
        } => {
            if let Err(err) = retry_replica_write(|| {
                store.upsert_replica_timeline_events_batch_for_host(host_id, events)
            })
            .await
            {
                tracing::warn!(
                    host_id = %host_id,
                    agent_id = %agent_id,
                    error = %err,
                    "failed to persist TimelineCatchup batch to replica; resetting contiguous cursor"
                );
                let _ = store.clear_replica_contiguous_seq(agent_id);
            } else if *has_older_history {
                let _ = store.set_replica_has_older(agent_id, true);
            }
        }
        WireMessage::TimelineHistoryPage {
            agent_id,
            events,
            has_more_older,
            ..
        } => {
            if !events.is_empty()
                && let Err(err) = retry_replica_write(|| {
                    store.upsert_replica_timeline_events_batch_for_host(host_id, events)
                })
                .await
            {
                tracing::warn!(
                    host_id = %host_id,
                    agent_id = %agent_id,
                    error = %err,
                    "failed to persist TimelineHistoryPage batch to replica"
                );
            } else {
                let _ = store.set_replica_has_older(agent_id, *has_more_older);
            }
        }
        WireMessage::TimelineLive { event } => {
            if let Err(err) =
                retry_replica_write(|| store.upsert_replica_timeline_event_for_host(host_id, event))
                    .await
            {
                tracing::warn!(
                    host_id = %host_id,
                    agent_id = %event.agent_id,
                    seq = event.seq,
                    error = %err,
                    "failed to persist TimelineLive event to replica; resetting contiguous cursor"
                );
                let _ = store.clear_replica_contiguous_seq(&event.agent_id);
            }
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
            pairing_secret: offer.pairing_secret,
            pairing_secret_updated_at_ms: unix_ms_now() as i64,
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

#[cfg(test)]
mod direct_auth_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn connect_smart_never_probes_unconfigured_speculative_loopback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind decoy listener");
        let addr = listener.local_addr().expect("addr");
        let saw_any_request = Arc::new(AtomicBool::new(false));
        let saw_bg = Arc::clone(&saw_any_request);

        let server = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = vec![0u8; 1024];
                let _ = sock.read(&mut buf).await;
                saw_bg.store(true, Ordering::SeqCst);
                let _ = sock
                    .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });

        // Profile points at NO direct endpoints even though `remote_daemon_port` matches our decoy
        // loopback listener: `connect_smart` MUST NOT speculatively dial `127.0.0.1:<port>` with the bearer token!
        let profile = SavedHostProfile {
            host_id: "host_expected_real".into(),
            hostname: "real-box".into(),
            custom_name: None,
            ssh_alias: None,
            route_mode: HostRouteMode::DirectOnly,
            remote_daemon_port: addr.port(),
            custom_direct_endpoint: None,
            daemon_pubkey_b64: "pk".into(),
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "super-secret-bearer-token-must-never-leak".into(),
            pairing_secret_updated_at_ms: 1,
            state: HostConnectionState::Disconnected,
        };

        let err = connect_smart(&profile, None)
            .await
            .err()
            .expect("empty direct endpoints must fail unreachable");
        assert!(
            matches!(err, ClientCoreError::Unreachable),
            "expected Unreachable, got {err:?}"
        );
        assert!(
            !saw_any_request.load(Ordering::SeqCst),
            "connect_smart must NEVER speculatively dial unconfigured loopback addresses with bearer credentials"
        );
        server.abort();
    }

    #[tokio::test]
    async fn connect_via_relay_rejects_empty_secret_before_dialing_and_auto_preserves_auth_error() {
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keygen");

        // 1. Empty/whitespace pairing_secret must fail closed with Unauthorized BEFORE dialing relay
        for bad_secret in ["", "   ", "bad\nsecret"] {
            let err = connect_via_relay("ws://127.0.0.1:1", &kp.public_key_b64, bad_secret, None)
                .await
                .err()
                .expect("empty/invalid secret must fail");
            assert!(
                matches!(err, ClientCoreError::Unauthorized(_)) && err.is_auth_error(),
                "expected Unauthorized auth error for {bad_secret:?}, got {err:?}"
            );
        }

        // 2. Auto mode preserves Direct WS 401 auth error instead of swallowing it as Unreachable
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = vec![0u8; 1024];
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });

        let profile = SavedHostProfile {
            host_id: kp.host_id().expect("host_id"),
            hostname: "auto-auth-box".into(),
            custom_name: None,
            ssh_alias: None,
            route_mode: HostRouteMode::Auto,
            remote_daemon_port: addr.port(),
            custom_direct_endpoint: None,
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: vec![format!("ws://{addr}/ws")],
            relay_url: None,
            pairing_secret: "wrong-secret".into(),
            pairing_secret_updated_at_ms: 1,
            state: HostConnectionState::Disconnected,
        };

        let err = connect_smart(&profile, None)
            .await
            .err()
            .expect("Auto mode must surface direct 401 auth rejection");
        assert!(
            err.is_auth_error(),
            "Auto mode must preserve auth error rather than returning Unreachable, got {err:?}"
        );
        server.abort();
    }
}
