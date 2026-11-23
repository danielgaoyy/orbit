//! Ultra-lightweight (`< 5.5MB` RSS) zero-knowledge E2EE Relay for Orbit.
//!
//! Designed to run on a personal AWS devbox or micro-VPS with `--allow-pubkey` admission locking
//! so strangers or open-source users can NEVER consume memory or egress bandwidth on your server.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::{
    Router,
    extract::{
        Path as AxumPath, Query, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use orbit_crypto::{
    RELAY_POP_MSG_LEN, RelayPopInitiator, SecureChannel, TUNNEL_ATTACH_MAC_LEN,
    TUNNEL_ATTACH_NONCE_LEN, decode_key_32, encode_key_32, verify_tunnel_attach_mac,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore, mpsc, oneshot};
use tracing::{info, warn};
use zeroize::Zeroizing;

/// Max WebSocket frame & message size across tunnel/client relay endpoints (48 MiB, safely carrying up to
/// 25 MiB base64/E2EE multimodal payloads without tearing down sessions).
pub const MAX_RELAY_WS_FRAME_BYTES: usize = 48 * 1024 * 1024;
/// Tight WebSocket frame & message cap on `/v1/daemon/control` (carries only 48-byte PoP frames
/// and small encrypted JSON control notifications, preventing unauthenticated memory-amplification DoS).
const MAX_CONTROL_WS_FRAME_BYTES: usize = 64 * 1024;
/// Default timeout for completing the 1-RTT Noise_NK Proof-of-Possession handshake on `/v1/daemon/control`.
pub const DAEMON_POP_TIMEOUT: Duration = Duration::from_secs(5);
/// Bounded timeout for sending a WebSocket Close frame when tearing down rejected/ended sockets.
const DAEMON_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
/// Hard cap on simultaneous unauthenticated daemon PoP handshakes before shedding load with 503.
pub const MAX_CONCURRENT_CONTROL_POPS: usize = 64;
/// Hard cap on simultaneous unauthenticated `/v1/daemon/tunnel` attach handshakes.
const MAX_CONCURRENT_TUNNEL_ATTACHES: usize = 128;
/// Shared deadline covering encrypted `open_tunnel` dispatch and daemon `/v1/daemon/tunnel` attachment.
const TUNNEL_ATTACH_TIMEOUT: Duration = Duration::from_secs(10);
/// Body returned by `GET /v1/daemon/probe` on HTTP 200 so clients can confirm an Orbit Relay endpoint.
pub const RELAY_PROBE_OK_BODY: &str = "orbit-relay-ok";
const HEX_LUT: &[u8; 16] = b"0123456789abcdef";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPersistedConfig {
    pub listen_addr: SocketAddr,
    pub allowed_pubkeys: Vec<String>,
}

impl Default for RelayPersistedConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([0, 0, 0, 0], 7878)),
            allowed_pubkeys: Vec::new(),
        }
    }
}

impl RelayPersistedConfig {
    pub fn load(home_dir: &Path) -> Self {
        let path = home_dir.join("relay.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, home_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(home_dir)?;
        let raw = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(home_dir.join("relay.json"), raw)
    }
}

#[derive(Clone)]
pub struct RelayConfig {
    pub listen_addr: SocketAddr,
    /// If non-empty, ONLY daemons whose `pubkey` is in this allowlist can register or receive clients.
    pub allowed_pubkeys: Arc<HashSet<String>>,
}

struct PendingClient {
    client_to_daemon_rx: mpsc::Receiver<Message>,
    daemon_to_client_tx: mpsc::Sender<Message>,
}

struct RegisteredHost {
    /// Monotonic connection identifier preventing a stale control loop exit from evicting a newer registration.
    conn_id: u64,
    /// Channel to ask the connected Daemon to spawn a new E2EE tunnel for an incoming mobile client.
    accept_tx: mpsc::Sender<PendingClient>,
}

struct WaitingTunnel {
    /// Public key of the host daemon that owns this pending tunnel slot.
    expected_pubkey: String,
    /// Per-tunnel 32-byte CSPRNG attach secret sent over the encrypted control channel.
    attach_secret: Zeroizing<[u8; 32]>,
    /// Shared deadline bounding tunnel attachment & authentication.
    deadline: tokio::time::Instant,
    tx: oneshot::Sender<WebSocket>,
}

#[derive(Clone)]
struct RelayState {
    allowed_pubkeys: Arc<HashSet<String>>,
    hosts: Arc<RwLock<HashMap<String, RegisteredHost>>>,
    waiting_tunnels: Arc<RwLock<HashMap<String, WaitingTunnel>>>,
    next_conn_id: Arc<AtomicU64>,
    pop_semaphore: Arc<Semaphore>,
    attach_semaphore: Arc<Semaphore>,
    pop_timeout: Duration,
    tunnel_timeout: Duration,
}

impl RelayState {
    fn new(
        allowed_pubkeys: Arc<HashSet<String>>,
        pop_timeout: Duration,
        max_concurrent_pops: usize,
        tunnel_timeout: Duration,
    ) -> Self {
        Self {
            allowed_pubkeys,
            hosts: Arc::new(RwLock::new(HashMap::new())),
            waiting_tunnels: Arc::new(RwLock::new(HashMap::new())),
            next_conn_id: Arc::new(AtomicU64::new(0)),
            pop_semaphore: Arc::new(Semaphore::new(max_concurrent_pops)),
            attach_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_TUNNEL_ATTACHES)),
            pop_timeout,
            tunnel_timeout,
        }
    }
}

#[derive(Deserialize)]
struct HostRegisterQuery {
    pubkey: String,
}

#[derive(Deserialize)]
struct TunnelAttachQuery {
    tunnel_id: String,
    pubkey: String,
}

fn build_relay_router(state: RelayState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/daemon/probe", get(http_daemon_probe))
        .route("/v1/daemon/control", get(ws_daemon_control))
        .route("/v1/daemon/tunnel", get(ws_daemon_tunnel))
        .route("/v1/client/{pubkey}", get(ws_client_connect))
        .with_state(state)
}

pub async fn run_relay(config: RelayConfig) -> anyhow::Result<()> {
    let state = RelayState::new(
        config.allowed_pubkeys.clone(),
        DAEMON_POP_TIMEOUT,
        MAX_CONCURRENT_CONTROL_POPS,
        TUNNEL_ATTACH_TIMEOUT,
    );
    let app = build_relay_router(state);

    let listener = tokio::net::TcpListener::bind(config.listen_addr).await?;
    info!(
        addr = %config.listen_addr,
        allowlist_count = config.allowed_pubkeys.len(),
        "Orbit Zero-Knowledge Relay listening"
    );
    axum::serve(listener, app).await?;
    Ok(())
}

fn is_pubkey_allowed(state: &RelayState, pubkey: &str) -> bool {
    state.allowed_pubkeys.is_empty() || state.allowed_pubkeys.contains(pubkey)
}

/// Pure read-only preflight probe for CLI onboarding (`orbit use-relay`).
/// Never opens a control socket or mutates `state.hosts`, so live daemons are never displaced.
async fn http_daemon_probe(
    Query(q): Query<HostRegisterQuery>,
    State(state): State<RelayState>,
) -> impl IntoResponse {
    if !is_pubkey_allowed(&state, &q.pubkey) {
        return (StatusCode::FORBIDDEN, "forbidden");
    }
    if q.pubkey.trim() != q.pubkey || decode_key_32(&q.pubkey).is_err() {
        return (StatusCode::BAD_REQUEST, "invalid pubkey");
    }
    (StatusCode::OK, RELAY_PROBE_OK_BODY)
}

async fn ws_daemon_control(
    ws: WebSocketUpgrade,
    Query(q): Query<HostRegisterQuery>,
    State(state): State<RelayState>,
) -> impl IntoResponse {
    if !is_pubkey_allowed(&state, &q.pubkey) {
        warn!(pubkey = %q.pubkey, "Rejected unauthorized daemon registration");
        return StatusCode::FORBIDDEN.into_response();
    }
    if q.pubkey.trim() != q.pubkey {
        warn!("Rejected daemon registration with non-canonical pubkey whitespace");
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(pubkey_bytes) = decode_key_32(&q.pubkey) else {
        warn!(pubkey = %q.pubkey, "Rejected daemon registration with invalid X25519 pubkey");
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(pop_permit) = Arc::clone(&state.pop_semaphore).try_acquire_owned() else {
        warn!(
            pubkey = %q.pubkey,
            "Rejected daemon control connection: concurrent PoP handshake limit reached"
        );
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_frame_size(MAX_CONTROL_WS_FRAME_BYTES)
        .max_message_size(MAX_CONTROL_WS_FRAME_BYTES)
        .on_upgrade(move |socket| {
            handle_daemon_control(socket, q.pubkey, pubkey_bytes, state, pop_permit)
        })
}

async fn close_socket_bounded(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let _ = tokio::time::timeout(DAEMON_CLOSE_TIMEOUT, async {
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code,
                reason: reason.into(),
            })))
            .await;
        let _ = SinkExt::close(socket).await;
    })
    .await;
}

async fn perform_daemon_pop(
    socket: &mut WebSocket,
    pubkey_bytes: &[u8; 32],
    timeout: Duration,
) -> Result<SecureChannel, &'static str> {
    let (initiator, msg1) =
        RelayPopInitiator::new(pubkey_bytes).map_err(|_| "failed to construct PoP challenge")?;

    tokio::time::timeout(timeout, socket.send(Message::Binary(msg1.into())))
        .await
        .map_err(|_| "timed out sending PoP challenge")?
        .map_err(|_| "failed sending PoP challenge")?;

    let msg2 = tokio::time::timeout(timeout, async {
        while let Some(frame) = socket.recv().await {
            match frame {
                Ok(Message::Binary(b)) => return Ok(b),
                Ok(Message::Ping(p)) => {
                    if socket.send(Message::Pong(p)).await.is_err() {
                        return Err("failed replying pong during PoP");
                    }
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(_)) => return Err("peer closed before PoP response"),
                Ok(Message::Text(_)) => return Err("unexpected text frame during PoP"),
                Err(_) => return Err("websocket error during PoP"),
            }
        }
        Err("socket closed before PoP response")
    })
    .await
    .map_err(|_| "timed out waiting for PoP response")??;

    if msg2.len() != RELAY_POP_MSG_LEN {
        return Err("invalid PoP response length");
    }

    initiator
        .finalize(&msg2)
        .map_err(|_| "cryptographic PoP verification failed")
}

async fn handle_daemon_control(
    mut socket: WebSocket,
    pubkey: String,
    pubkey_bytes: [u8; 32],
    state: RelayState,
    pop_permit: OwnedSemaphorePermit,
) {
    let mut control_chan =
        match perform_daemon_pop(&mut socket, &pubkey_bytes, state.pop_timeout).await {
            Ok(chan) => {
                // Release unauthenticated PoP handshake concurrency slot immediately once authenticated.
                drop(pop_permit);
                chan
            }
            Err(reason) => {
                // Drop permit BEFORE bounded close so slow attackers cannot pin semaphore slots.
                drop(pop_permit);
                warn!(
                    pubkey = %pubkey,
                    reason = %reason,
                    "Rejected daemon control socket: Proof-of-Possession failed"
                );
                close_socket_bounded(&mut socket, close_code::POLICY, reason).await;
                return;
            }
        };

    let my_conn_id = state.next_conn_id.fetch_add(1, Ordering::Relaxed) + 1;
    let (accept_tx, mut accept_rx) = mpsc::channel::<PendingClient>(32);
    state.hosts.write().await.insert(
        pubkey.clone(),
        RegisteredHost {
            conn_id: my_conn_id,
            accept_tx,
        },
    );
    info!(
        pubkey = %pubkey,
        conn_id = my_conn_id,
        "Host daemon authenticated (PoP) and registered on relay"
    );

    let registered_ack = serde_json::json!({ "op": "registered" }).to_string();
    let ack_sent = match control_chan.seal_record(registered_ack.as_bytes()) {
        Ok(ct) => tokio::time::timeout(
            state.tunnel_timeout,
            socket.send(Message::Binary(ct.into())),
        )
        .await
        .is_ok_and(|res| res.is_ok()),
        Err(_) => false,
    };

    if ack_sent {
        loop {
            tokio::select! {
                maybe_client = accept_rx.recv() => {
                    let Some(pending) = maybe_client else { break; };
                    let tunnel_id = generate_tunnel_id();
                    let mut attach_secret = Zeroizing::new([0u8; 32]);
                    rand::rng().fill_bytes(&mut *attach_secret);
                    let attach_secret_b64 = encode_key_32(&attach_secret);
                    let deadline = tokio::time::Instant::now() + state.tunnel_timeout;

                    let (tun_tx, tun_rx) = oneshot::channel::<WebSocket>();
                    state.waiting_tunnels.write().await.insert(
                        tunnel_id.clone(),
                        WaitingTunnel {
                            expected_pubkey: pubkey.clone(),
                            attach_secret,
                            deadline,
                            tx: tun_tx,
                        },
                    );

                    let notify = serde_json::json!({
                        "op": "open_tunnel",
                        "tunnel_id": tunnel_id,
                        "attach_secret": attach_secret_b64,
                    }).to_string();
                    let Ok(ct) = control_chan.seal_record(notify.as_bytes()) else {
                        state.waiting_tunnels.write().await.remove(&tunnel_id);
                        break;
                    };

                    let sent = tokio::time::timeout_at(
                        deadline,
                        socket.send(Message::Binary(ct.into())),
                    )
                    .await
                    .is_ok_and(|res| res.is_ok());
                    if !sent {
                        state.waiting_tunnels.write().await.remove(&tunnel_id);
                        break;
                    }

                    let waiting_tunnels = state.waiting_tunnels.clone();
                    let tid_cleanup = tunnel_id.clone();
                    tokio::spawn(async move {
                        if let Ok(Ok(daemon_ws)) =
                            tokio::time::timeout_at(deadline, tun_rx).await
                        {
                            bridge_ciphertext_streams(daemon_ws, pending).await;
                        } else {
                            waiting_tunnels.write().await.remove(&tid_cleanup);
                        }
                    });
                }
                msg = socket.recv() => {
                    match msg {
                        Some(Ok(Message::Ping(p))) => {
                            let _ = socket.send(Message::Pong(p)).await;
                        }
                        Some(Ok(Message::Close(_))) => break,
                        Some(Ok(_)) => {}
                        _ => break,
                    }
                }
            }
        }
    }

    let removed = {
        let mut hosts = state.hosts.write().await;
        if hosts
            .get(&pubkey)
            .is_some_and(|entry| entry.conn_id == my_conn_id)
        {
            hosts.remove(&pubkey);
            true
        } else {
            false
        }
    };
    if removed {
        info!(pubkey = %pubkey, conn_id = my_conn_id, "Host daemon disconnected from relay");
    } else {
        info!(
            pubkey = %pubkey,
            conn_id = my_conn_id,
            "Stale host daemon control connection closed (newer registration remains active)"
        );
    }
}

async fn authenticate_tunnel_attach(
    socket: &mut WebSocket,
    tunnel_id: &str,
    attach_secret: &[u8; 32],
    deadline: tokio::time::Instant,
) -> Result<(), &'static str> {
    let mut nonce = [0u8; TUNNEL_ATTACH_NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);

    tokio::time::timeout_at(
        deadline,
        socket.send(Message::Binary(nonce.to_vec().into())),
    )
    .await
    .map_err(|_| "timed out sending tunnel attach nonce")?
    .map_err(|_| "failed sending tunnel attach nonce")?;

    let mac_bytes = tokio::time::timeout_at(deadline, async {
        while let Some(frame) = socket.recv().await {
            match frame {
                Ok(Message::Binary(b)) => return Ok(b),
                Ok(Message::Ping(p)) => {
                    if socket.send(Message::Pong(p)).await.is_err() {
                        return Err("failed replying pong during tunnel attach");
                    }
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(_)) => return Err("peer closed before tunnel attach MAC"),
                Ok(Message::Text(_)) => return Err("unexpected text frame during tunnel attach"),
                Err(_) => return Err("websocket error during tunnel attach"),
            }
        }
        Err("socket closed before tunnel attach MAC")
    })
    .await
    .map_err(|_| "timed out waiting for tunnel attach MAC")??;

    if mac_bytes.len() != TUNNEL_ATTACH_MAC_LEN {
        return Err("invalid tunnel attach MAC length");
    }
    if !verify_tunnel_attach_mac(attach_secret, &nonce, tunnel_id, &mac_bytes) {
        return Err("tunnel attach HMAC verification failed");
    }
    Ok(())
}

async fn ws_daemon_tunnel(
    ws: WebSocketUpgrade,
    Query(q): Query<TunnelAttachQuery>,
    State(state): State<RelayState>,
) -> impl IntoResponse {
    if !is_pubkey_allowed(&state, &q.pubkey) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (attach_secret, deadline) = {
        let waiting = state.waiting_tunnels.read().await;
        match waiting.get(&q.tunnel_id) {
            None => return StatusCode::NOT_FOUND.into_response(),
            Some(entry) if entry.expected_pubkey != q.pubkey => {
                warn!(
                    tunnel_id = %q.tunnel_id,
                    claimed_pubkey = %q.pubkey,
                    "Rejected daemon tunnel attach with mismatched pubkey"
                );
                return StatusCode::FORBIDDEN.into_response();
            }
            Some(entry) if tokio::time::Instant::now() >= entry.deadline => {
                return StatusCode::NOT_FOUND.into_response();
            }
            Some(entry) => (Zeroizing::new(*entry.attach_secret), entry.deadline),
        }
    };
    let Ok(attach_permit) = Arc::clone(&state.attach_semaphore).try_acquire_owned() else {
        warn!(
            tunnel_id = %q.tunnel_id,
            "Rejected daemon tunnel attach: concurrent attach limit reached"
        );
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    ws.max_frame_size(MAX_RELAY_WS_FRAME_BYTES)
        .max_message_size(MAX_RELAY_WS_FRAME_BYTES)
        .on_upgrade(move |mut socket| async move {
            let auth_res =
                authenticate_tunnel_attach(&mut socket, &q.tunnel_id, &attach_secret, deadline)
                    .await;
            drop(attach_permit);
            if let Err(reason) = auth_res {
                warn!(
                    tunnel_id = %q.tunnel_id,
                    pubkey = %q.pubkey,
                    reason = %reason,
                    "Rejected unauthenticated daemon tunnel attach without consuming slot"
                );
                close_socket_bounded(&mut socket, close_code::POLICY, reason).await;
                return;
            }

            let tun_tx = {
                let mut waiting = state.waiting_tunnels.write().await;
                match waiting.entry(q.tunnel_id.clone()) {
                    Entry::Occupied(entry) if entry.get().expected_pubkey == q.pubkey => {
                        Some(entry.remove().tx)
                    }
                    _ => None,
                }
            };
            if let Some(tx) = tun_tx {
                let _ = tx.send(socket);
            } else {
                close_socket_bounded(&mut socket, close_code::NORMAL, "tunnel slot expired").await;
            }
        })
}

async fn ws_client_connect(
    ws: WebSocketUpgrade,
    AxumPath(pubkey): AxumPath<String>,
    State(state): State<RelayState>,
) -> impl IntoResponse {
    if !is_pubkey_allowed(&state, &pubkey) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let accept_tx = state
        .hosts
        .read()
        .await
        .get(&pubkey)
        .map(|h| h.accept_tx.clone());
    let Some(accept_tx) = accept_tx else {
        return StatusCode::BAD_GATEWAY.into_response();
    };

    ws.max_frame_size(MAX_RELAY_WS_FRAME_BYTES)
        .max_message_size(MAX_RELAY_WS_FRAME_BYTES)
        .on_upgrade(move |client_ws| async move {
            let (c2d_tx, c2d_rx) = mpsc::channel::<Message>(128);
            let (d2c_tx, mut d2c_rx) = mpsc::channel::<Message>(128);

            if accept_tx
                .send(PendingClient {
                    client_to_daemon_rx: c2d_rx,
                    daemon_to_client_tx: d2c_tx,
                })
                .await
                .is_err()
            {
                return;
            }

            let (mut c_sink, mut c_stream) = client_ws.split();
            loop {
                tokio::select! {
                    maybe_out = d2c_rx.recv() => {
                        match maybe_out {
                            Some(msg) => {
                                if c_sink.send(msg).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                    maybe_in = c_stream.next() => {
                        match maybe_in {
                            Some(Ok(msg)) => {
                                if c2d_tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                            _ => break,
                        }
                    }
                }
            }
            let _ = tokio::time::timeout(DAEMON_CLOSE_TIMEOUT, c_sink.close()).await;
        })
}

async fn bridge_ciphertext_streams(daemon_ws: WebSocket, mut pending: PendingClient) {
    let (mut d_sink, mut d_stream) = daemon_ws.split();
    loop {
        tokio::select! {
            maybe_in = pending.client_to_daemon_rx.recv() => {
                match maybe_in {
                    Some(msg) => {
                        if d_sink.send(msg).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            maybe_out = d_stream.next() => {
                match maybe_out {
                    Some(Ok(msg)) => {
                        if pending.daemon_to_client_tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        }
    }
    let _ = tokio::time::timeout(DAEMON_CLOSE_TIMEOUT, d_sink.close()).await;
}

/// Generate a 256-bit CSPRNG tunnel identifier (`tun_<64 lowercase hex chars>`).
fn generate_tunnel_id() -> String {
    format!("tun_{}", rand_hex_256())
}

fn rand_hex_256() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push(HEX_LUT[(b >> 4) as usize] as char);
        out.push(HEX_LUT[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_crypto::{HostIdentityKeypair, RelayPopResponder};
    use tokio_tungstenite::{
        connect_async,
        tungstenite::{Error as TungError, Message as TungMsg},
    };

    type TestWs = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    async fn spawn_test_relay(
        allowed_pubkeys: HashSet<String>,
        pop_timeout: Duration,
    ) -> (SocketAddr, RelayState, tokio::task::JoinHandle<()>) {
        spawn_test_relay_with_limits(
            allowed_pubkeys,
            pop_timeout,
            MAX_CONCURRENT_CONTROL_POPS,
            TUNNEL_ATTACH_TIMEOUT,
        )
        .await
    }

    async fn spawn_test_relay_with_limits(
        allowed_pubkeys: HashSet<String>,
        pop_timeout: Duration,
        max_concurrent_pops: usize,
        tunnel_timeout: Duration,
    ) -> (SocketAddr, RelayState, tokio::task::JoinHandle<()>) {
        let state = RelayState::new(
            Arc::new(allowed_pubkeys),
            pop_timeout,
            max_concurrent_pops,
            tunnel_timeout,
        );
        let app = build_relay_router(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test relay");
        let addr = listener.local_addr().expect("local addr");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, state, handle)
    }

    async fn recv_encrypted_control_json(
        ws: &mut TestWs,
        chan: &mut SecureChannel,
    ) -> (serde_json::Value, Vec<u8>) {
        let ct = match tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("control frame timeout")
            .expect("control frame")
            .expect("control frame ok")
        {
            TungMsg::Binary(b) => b.to_vec(),
            other => panic!("expected encrypted binary control frame, got: {other:?}"),
        };
        let plain = chan.open_record(&ct).expect("decrypt control frame");
        let json: serde_json::Value =
            serde_json::from_slice(&plain).expect("parse decrypted control json");
        (json, ct)
    }

    async fn connect_authenticated_daemon(
        addr: SocketAddr,
        kp: &HostIdentityKeypair,
    ) -> (TestWs, SecureChannel) {
        let url = format!("ws://{addr}/v1/daemon/control?pubkey={}", kp.public_key_b64);
        let (mut ws, _) = connect_async(&url).await.expect("connect control ws");

        let challenge = match tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("challenge timeout")
            .expect("challenge frame")
            .expect("challenge ok")
        {
            TungMsg::Binary(b) => b,
            other => panic!("expected binary PoP challenge, got: {other:?}"),
        };
        assert_eq!(challenge.len(), RELAY_POP_MSG_LEN);

        let priv_bytes = kp.private_bytes().expect("priv bytes");
        let (mut chan, response) =
            RelayPopResponder::accept(&priv_bytes, &challenge).expect("pop accept");
        assert_eq!(response.len(), RELAY_POP_MSG_LEN);
        ws.send(TungMsg::Binary(response.into()))
            .await
            .expect("send pop response");

        let (ack, raw_ct) = recv_encrypted_control_json(&mut ws, &mut chan).await;
        assert!(
            !String::from_utf8_lossy(&raw_ct).contains("registered"),
            "registered ack must be encrypted on the wire"
        );
        assert_eq!(ack.get("op").and_then(|v| v.as_str()), Some("registered"));
        (ws, chan)
    }

    async fn attach_authenticated_tunnel(
        addr: SocketAddr,
        pubkey: &str,
        tunnel_id: &str,
        attach_secret: &[u8; 32],
    ) -> TestWs {
        let tun_url = format!("ws://{addr}/v1/daemon/tunnel?tunnel_id={tunnel_id}&pubkey={pubkey}");
        let (mut ws, _) = connect_async(&tun_url).await.expect("connect tunnel ws");
        let nonce_bytes = match tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("nonce timeout")
            .expect("nonce frame")
            .expect("nonce ok")
        {
            TungMsg::Binary(b) => b,
            other => panic!("expected binary tunnel attach nonce, got: {other:?}"),
        };
        let nonce: [u8; TUNNEL_ATTACH_NONCE_LEN] = nonce_bytes
            .as_ref()
            .try_into()
            .expect("16-byte attach nonce");
        let mac = orbit_crypto::compute_tunnel_attach_mac(attach_secret, &nonce, tunnel_id);
        ws.send(TungMsg::Binary(mac.to_vec().into()))
            .await
            .expect("send tunnel attach MAC");
        ws
    }

    async fn assert_ws_http_status(url: &str, expected_status: u16) {
        let err = connect_async(url)
            .await
            .expect_err("expected HTTP rejection before WS upgrade");
        match err {
            TungError::Http(resp) => {
                assert_eq!(
                    resp.status().as_u16(),
                    expected_status,
                    "unexpected HTTP status for {url}"
                );
            }
            other => panic!("expected HTTP {expected_status} for {url}, got: {other:?}"),
        }
    }

    #[test]
    fn relay_ws_frame_limit_covers_25mib_base64_media_payloads() {
        assert_eq!(MAX_RELAY_WS_FRAME_BYTES, 48 * 1024 * 1024);
    }

    #[test]
    fn csprng_tunnel_ids_have_256bit_hex_format_and_are_unique() {
        let count = 256;
        let mut seen = HashSet::with_capacity(count);
        for _ in 0..count {
            let tid = generate_tunnel_id();
            assert!(tid.starts_with("tun_"), "missing tun_ prefix: {tid}");
            let hex = &tid[4..];
            assert_eq!(hex.len(), 64, "expected 256-bit (64 hex char) tunnel id");
            assert!(
                hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "tunnel id must be lowercase hex: {tid}"
            );
            assert!(seen.insert(tid), "CSPRNG tunnel_id collision detected");
        }
        assert_eq!(seen.len(), count);
    }

    #[tokio::test]
    async fn legitimate_daemon_pop_registration_and_encrypted_control_tunnel_bridging_succeeds() {
        let kp = HostIdentityKeypair::generate().expect("keygen");
        let mut allowed = HashSet::new();
        allowed.insert(kp.public_key_b64.clone());

        let (addr, _state, server) = spawn_test_relay(allowed, Duration::from_secs(5)).await;
        let (mut daemon_ctrl, mut ctrl_chan) = connect_authenticated_daemon(addr, &kp).await;

        // Client connects to the registered host
        let client_url = format!("ws://{addr}/v1/client/{}", kp.public_key_b64);
        let (mut client_ws, _) = connect_async(&client_url).await.expect("client connect");

        // Daemon receives ENCRYPTED open_tunnel notification with tunnel_id + attach_secret
        let (open_json, raw_ct) =
            recv_encrypted_control_json(&mut daemon_ctrl, &mut ctrl_chan).await;
        assert_eq!(
            open_json.get("op").and_then(|v| v.as_str()),
            Some("open_tunnel")
        );
        let tunnel_id = open_json
            .get("tunnel_id")
            .and_then(|v| v.as_str())
            .expect("tunnel_id");
        let secret_b64 = open_json
            .get("attach_secret")
            .and_then(|v| v.as_str())
            .expect("attach_secret");
        let attach_secret = decode_key_32(secret_b64).expect("decode attach_secret");
        assert!(tunnel_id.starts_with("tun_"));
        assert_eq!(tunnel_id.len(), 68);
        let wire_lossy = String::from_utf8_lossy(&raw_ct);
        assert!(
            !wire_lossy.contains("open_tunnel")
                && !wire_lossy.contains(tunnel_id)
                && !wire_lossy.contains(secret_b64),
            "open_tunnel, tunnel_id, and attach_secret must never appear in plaintext on the wire"
        );

        // Daemon attaches to the tunnel and authenticates with HMAC-SHA256(attach_secret, nonce || tunnel_id)
        let mut daemon_tun =
            attach_authenticated_tunnel(addr, &kp.public_key_b64, tunnel_id, &attach_secret).await;

        // Verify bidirectional ciphertext forwarding
        client_ws
            .send(TungMsg::Binary(b"client-ciphertext-1".to_vec().into()))
            .await
            .expect("client send");
        let got_at_daemon = tokio::time::timeout(Duration::from_secs(3), daemon_tun.next())
            .await
            .expect("daemon recv timeout")
            .expect("daemon frame")
            .expect("daemon ok");
        assert_eq!(
            got_at_daemon,
            TungMsg::Binary(b"client-ciphertext-1".to_vec().into())
        );

        daemon_tun
            .send(TungMsg::Binary(b"daemon-ciphertext-2".to_vec().into()))
            .await
            .expect("daemon send");
        let got_at_client = tokio::time::timeout(Duration::from_secs(3), client_ws.next())
            .await
            .expect("client recv timeout")
            .expect("client frame")
            .expect("client ok");
        assert_eq!(
            got_at_client,
            TungMsg::Binary(b"daemon-ciphertext-2".to_vec().into())
        );

        server.abort();
    }

    #[tokio::test]
    async fn attacker_without_private_key_fails_pop_and_cannot_register_or_evict_legitimate_daemon()
    {
        let victim_kp = HostIdentityKeypair::generate().expect("victim keygen");
        let attacker_kp = HostIdentityKeypair::generate().expect("attacker keygen");
        let mut allowed = HashSet::new();
        allowed.insert(victim_kp.public_key_b64.clone());

        let (addr, state, server) = spawn_test_relay(allowed, Duration::from_secs(5)).await;
        let ctrl_url = format!(
            "ws://{addr}/v1/daemon/control?pubkey={}",
            victim_kp.public_key_b64
        );

        // 1. Before legitimate daemon registers: attacker claims victim's allowlisted pubkey
        //    and responds with a different keypair / forged bytes -> must be rejected & NOT registered.
        {
            let (mut attacker_ws, _) = connect_async(&ctrl_url).await.expect("attacker upgrade");
            let challenge = match tokio::time::timeout(Duration::from_secs(3), attacker_ws.next())
                .await
                .expect("challenge timeout")
                .expect("challenge frame")
                .expect("challenge ok")
            {
                TungMsg::Binary(b) => b,
                other => panic!("expected challenge, got: {other:?}"),
            };

            // Wrong private key cannot decrypt challenge
            let wrong_priv = attacker_kp.private_bytes().expect("attacker priv");
            assert!(RelayPopResponder::accept(&wrong_priv, &challenge).is_err());

            // Attacker sends arbitrary bogus bytes (same length as a valid response)
            let forged = vec![0xABu8; RELAY_POP_MSG_LEN];
            attacker_ws
                .send(TungMsg::Binary(forged.into()))
                .await
                .expect("send forged");

            // Relay must reject and close the socket
            let next_frame = tokio::time::timeout(Duration::from_secs(3), attacker_ws.next())
                .await
                .expect("close timeout");
            assert!(
                matches!(
                    next_frame,
                    None | Some(Ok(TungMsg::Close(_))) | Some(Err(_))
                ),
                "expected relay to close attacker socket, got: {next_frame:?}"
            );

            // Victim slot MUST remain empty -> client gets 502 Bad Gateway
            assert!(
                !state
                    .hosts
                    .read()
                    .await
                    .contains_key(&victim_kp.public_key_b64)
            );
            assert_ws_http_status(
                &format!("ws://{addr}/v1/client/{}", victim_kp.public_key_b64),
                502,
            )
            .await;
        }

        // 2. Now legitimate daemon registers cleanly
        let (mut victim_ctrl, mut victim_chan) =
            connect_authenticated_daemon(addr, &victim_kp).await;
        let orig_conn_id = state
            .hosts
            .read()
            .await
            .get(&victim_kp.public_key_b64)
            .expect("victim registered")
            .conn_id;

        // 3. Attacker tries to hijack/evict the active legitimate daemon (both forged binary & text & immediate drop)
        for mode in ["forged_binary", "unexpected_text", "drop_immediately"] {
            let (mut attacker_ws, _) = connect_async(&ctrl_url).await.expect("attacker upgrade");
            let _challenge = tokio::time::timeout(Duration::from_secs(3), attacker_ws.next())
                .await
                .expect("challenge timeout")
                .expect("challenge frame")
                .expect("challenge ok");
            match mode {
                "forged_binary" => {
                    let _ = attacker_ws
                        .send(TungMsg::Binary(vec![0x55u8; RELAY_POP_MSG_LEN].into()))
                        .await;
                    let _ = tokio::time::timeout(Duration::from_secs(3), attacker_ws.next()).await;
                }
                "unexpected_text" => {
                    let _ = attacker_ws
                        .send(TungMsg::Text("{\"op\":\"hijack\"}".into()))
                        .await;
                    let _ = tokio::time::timeout(Duration::from_secs(3), attacker_ws.next()).await;
                }
                _ => {
                    drop(attacker_ws);
                    tokio::time::sleep(Duration::from_millis(30)).await;
                }
            }
        }

        // Legitimate daemon's registration & conn_id MUST remain completely intact!
        let current_conn_id = state
            .hosts
            .read()
            .await
            .get(&victim_kp.public_key_b64)
            .expect("victim must still be registered")
            .conn_id;
        assert_eq!(current_conn_id, orig_conn_id);

        // Verify a real client still reaches the legitimate daemon over encrypted control channel
        let (_client_ws, _) = connect_async(&format!(
            "ws://{addr}/v1/client/{}",
            victim_kp.public_key_b64
        ))
        .await
        .expect("client must reach victim");
        let (open_json, _) = recv_encrypted_control_json(&mut victim_ctrl, &mut victim_chan).await;
        assert_eq!(
            open_json.get("op").and_then(|v| v.as_str()),
            Some("open_tunnel")
        );

        server.abort();
    }

    #[tokio::test]
    async fn daemon_pop_timeout_closes_silent_socket_without_registering() {
        let kp = HostIdentityKeypair::generate().expect("keygen");
        let mut allowed = HashSet::new();
        allowed.insert(kp.public_key_b64.clone());

        let (addr, state, server) = spawn_test_relay(allowed, Duration::from_millis(150)).await;
        let url = format!("ws://{addr}/v1/daemon/control?pubkey={}", kp.public_key_b64);
        let (mut silent_ws, _) = connect_async(&url).await.expect("connect");

        // Receive challenge and intentionally do NOT reply
        let _challenge = tokio::time::timeout(Duration::from_secs(2), silent_ws.next())
            .await
            .expect("challenge timeout")
            .expect("challenge frame")
            .expect("challenge ok");

        // Relay must close after pop_timeout
        let closed = tokio::time::timeout(Duration::from_secs(2), silent_ws.next())
            .await
            .expect("relay did not enforce pop_timeout");
        assert!(
            matches!(closed, None | Some(Ok(TungMsg::Close(_))) | Some(Err(_))),
            "expected close after PoP timeout, got: {closed:?}"
        );
        assert!(!state.hosts.read().await.contains_key(&kp.public_key_b64));

        server.abort();
    }

    #[tokio::test]
    async fn concurrent_pop_semaphore_sheds_excess_load_and_releases_after_auth() {
        let kp = HostIdentityKeypair::generate().expect("keygen");
        let mut allowed = HashSet::new();
        allowed.insert(kp.public_key_b64.clone());

        let (addr, _state, server) =
            spawn_test_relay_with_limits(allowed, Duration::from_secs(5), 2, TUNNEL_ATTACH_TIMEOUT)
                .await;
        let ctrl_url = format!("ws://{addr}/v1/daemon/control?pubkey={}", kp.public_key_b64);

        // Hold 2 in-flight unauthenticated PoP handshakes
        let (mut stalled_1, _) = connect_async(&ctrl_url).await.expect("stalled 1");
        let _ = stalled_1.next().await;
        let (mut stalled_2, _) = connect_async(&ctrl_url).await.expect("stalled 2");
        let _ = stalled_2.next().await;

        // 3rd unauthenticated control connection MUST be rejected with 503 Service Unavailable
        assert_ws_http_status(&ctrl_url, 503).await;

        // Release one stalled connection -> permit returns -> legitimate daemon authenticates
        drop(stalled_1);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (_authed_ctrl, _chan) = connect_authenticated_daemon(addr, &kp).await;

        // Authenticated steady-state daemon has already released its PoP permit,
        // so one more unauthenticated PoP handshake fits (total in-flight = 2: stalled_2 + third_ws)
        let (mut third_ws, _) = connect_async(&ctrl_url)
            .await
            .expect("authenticated daemon must release its PoP permit");
        let _ = third_ws.next().await;
        assert_ws_http_status(&ctrl_url, 503).await;

        drop(stalled_2);
        drop(third_ws);
        server.abort();
    }

    #[tokio::test]
    async fn daemon_control_rejects_unallowlisted_and_malformed_pubkeys() {
        let allowed_kp = HostIdentityKeypair::generate().expect("allowed keygen");
        let stranger_kp = HostIdentityKeypair::generate().expect("stranger keygen");
        let mut allowed = HashSet::new();
        allowed.insert(allowed_kp.public_key_b64.clone());

        let (addr, _state, server) = spawn_test_relay(allowed, Duration::from_secs(5)).await;

        // Unallowlisted pubkey -> 403 Forbidden
        assert_ws_http_status(
            &format!(
                "ws://{addr}/v1/daemon/control?pubkey={}",
                stranger_kp.public_key_b64
            ),
            403,
        )
        .await;

        // Open relay (empty allowlist) still rejects non-32-byte malformed pubkey -> 400 Bad Request
        let (open_addr, _open_state, open_server) =
            spawn_test_relay(HashSet::new(), Duration::from_secs(5)).await;
        assert_ws_http_status(
            &format!("ws://{open_addr}/v1/daemon/control?pubkey=not_a_valid_key"),
            400,
        )
        .await;

        server.abort();
        open_server.abort();
    }

    #[tokio::test]
    async fn rapid_daemon_reconnect_old_disconnect_does_not_evict_new_connection() {
        let kp = HostIdentityKeypair::generate().expect("keygen");
        let mut allowed = HashSet::new();
        allowed.insert(kp.public_key_b64.clone());

        let (addr, state, server) = spawn_test_relay(allowed, Duration::from_secs(5)).await;

        // Connection 1 registers
        let (mut old_ctrl, _old_chan) = connect_authenticated_daemon(addr, &kp).await;
        let conn1_id = state
            .hosts
            .read()
            .await
            .get(&kp.public_key_b64)
            .expect("conn1 registered")
            .conn_id;

        // Connection 2 reconnects & authenticates BEFORE connection 1 closes
        let (mut new_ctrl, mut new_chan) = connect_authenticated_daemon(addr, &kp).await;
        let conn2_id = state
            .hosts
            .read()
            .await
            .get(&kp.public_key_b64)
            .expect("conn2 registered")
            .conn_id;
        assert!(conn2_id > conn1_id);

        // Now forcefully close connection 1 and wait for its server-side cleanup task to run
        let _ = old_ctrl.close(None).await;
        drop(old_ctrl);
        tokio::time::sleep(Duration::from_millis(80)).await;

        // Connection 2 MUST still be registered with conn2_id!
        let active_conn_id = state
            .hosts
            .read()
            .await
            .get(&kp.public_key_b64)
            .expect("conn2 must not be evicted by conn1 disconnect")
            .conn_id;
        assert_eq!(active_conn_id, conn2_id);

        // End-to-end verification: incoming client triggers encrypted open_tunnel on connection 2!
        let (mut client_ws, _) =
            connect_async(&format!("ws://{addr}/v1/client/{}", kp.public_key_b64))
                .await
                .expect("client connect after reconnect");

        let (open_json, _) = recv_encrypted_control_json(&mut new_ctrl, &mut new_chan).await;
        let tunnel_id = open_json
            .get("tunnel_id")
            .and_then(|v| v.as_str())
            .expect("tunnel_id");
        let attach_secret = decode_key_32(
            open_json
                .get("attach_secret")
                .and_then(|v| v.as_str())
                .expect("attach_secret"),
        )
        .expect("decode attach_secret");

        let mut daemon_tun =
            attach_authenticated_tunnel(addr, &kp.public_key_b64, tunnel_id, &attach_secret).await;

        client_ws
            .send(TungMsg::Binary(b"post-reconnect-ping".to_vec().into()))
            .await
            .expect("send ping");
        let got = tokio::time::timeout(Duration::from_secs(3), daemon_tun.next())
            .await
            .expect("recv timeout")
            .expect("recv frame")
            .expect("recv ok");
        assert_eq!(got, TungMsg::Binary(b"post-reconnect-ping".to_vec().into()));

        server.abort();
    }

    #[tokio::test]
    async fn tunnel_attach_rejects_unknown_id_wrong_pubkey_and_forged_mac_without_consuming_slot() {
        let daemon_a = HostIdentityKeypair::generate().expect("daemon A");
        let daemon_b = HostIdentityKeypair::generate().expect("daemon B");
        let stranger = HostIdentityKeypair::generate().expect("stranger");

        let mut allowed = HashSet::new();
        allowed.insert(daemon_a.public_key_b64.clone());
        allowed.insert(daemon_b.public_key_b64.clone());

        let (addr, _state, server) = spawn_test_relay(allowed, Duration::from_secs(5)).await;
        let (mut ctrl_a, mut chan_a) = connect_authenticated_daemon(addr, &daemon_a).await;

        // 1. Unknown tunnel_id with allowlisted pubkey -> 404 Not Found
        assert_ws_http_status(
            &format!(
                "ws://{addr}/v1/daemon/tunnel?tunnel_id=tun_nonexistent&pubkey={}",
                daemon_a.public_key_b64
            ),
            404,
        )
        .await;

        // 2. Client connects to Daemon A -> creates a pending tunnel bound to daemon_a
        let (mut client_ws, _) = connect_async(&format!(
            "ws://{addr}/v1/client/{}",
            daemon_a.public_key_b64
        ))
        .await
        .expect("client connect");
        let (open_json, _) = recv_encrypted_control_json(&mut ctrl_a, &mut chan_a).await;
        let tunnel_id = open_json
            .get("tunnel_id")
            .and_then(|v| v.as_str())
            .expect("tunnel_id");
        let attach_secret = decode_key_32(
            open_json
                .get("attach_secret")
                .and_then(|v| v.as_str())
                .expect("attach_secret"),
        )
        .expect("decode attach_secret");

        // 3. Unlisted stranger trying to hijack tunnel_id -> 403 Forbidden
        assert_ws_http_status(
            &format!(
                "ws://{addr}/v1/daemon/tunnel?tunnel_id={tunnel_id}&pubkey={}",
                stranger.public_key_b64
            ),
            403,
        )
        .await;

        // 4. Another ALLOWLISTED daemon (daemon_b) trying to steal daemon_a's tunnel_id -> 403 Forbidden!
        assert_ws_http_status(
            &format!(
                "ws://{addr}/v1/daemon/tunnel?tunnel_id={tunnel_id}&pubkey={}",
                daemon_b.public_key_b64
            ),
            403,
        )
        .await;

        // 5. Attacker knowing BOTH tunnel_id AND daemon_a's pubkey but lacking attach_secret
        //    fails HMAC-SHA256 verification and MUST NOT consume the pending tunnel slot!
        {
            let tun_url = format!(
                "ws://{addr}/v1/daemon/tunnel?tunnel_id={tunnel_id}&pubkey={}",
                daemon_a.public_key_b64
            );
            let (mut attacker_tun, _) = connect_async(&tun_url).await.expect("attacker upgrade");
            let nonce = tokio::time::timeout(Duration::from_secs(3), attacker_tun.next())
                .await
                .expect("nonce timeout")
                .expect("nonce frame")
                .expect("nonce ok");
            assert!(matches!(nonce, TungMsg::Binary(ref b) if b.len() == TUNNEL_ATTACH_NONCE_LEN));
            attacker_tun
                .send(TungMsg::Binary(vec![0xEEu8; TUNNEL_ATTACH_MAC_LEN].into()))
                .await
                .expect("send forged mac");
            let closed = tokio::time::timeout(Duration::from_secs(3), attacker_tun.next())
                .await
                .expect("close timeout");
            assert!(
                matches!(closed, None | Some(Ok(TungMsg::Close(_))) | Some(Err(_))),
                "expected relay to reject forged tunnel MAC, got: {closed:?}"
            );
        }

        // 6. Failed attempts must NOT have consumed/evicted the waiting tunnel slot:
        //    daemon_a can STILL authenticate with attach_secret and bridge ciphertext!
        let mut daemon_tun =
            attach_authenticated_tunnel(addr, &daemon_a.public_key_b64, tunnel_id, &attach_secret)
                .await;

        client_ws
            .send(TungMsg::Binary(b"still-alive".to_vec().into()))
            .await
            .expect("client send");
        let got = tokio::time::timeout(Duration::from_secs(3), daemon_tun.next())
            .await
            .expect("daemon recv timeout")
            .expect("daemon frame")
            .expect("daemon ok");
        assert_eq!(got, TungMsg::Binary(b"still-alive".to_vec().into()));

        // 7. Re-attaching to the already-consumed tunnel_id -> 404 Not Found
        assert_ws_http_status(
            &format!(
                "ws://{addr}/v1/daemon/tunnel?tunnel_id={tunnel_id}&pubkey={}",
                daemon_a.public_key_b64
            ),
            404,
        )
        .await;

        server.abort();
    }

    #[tokio::test]
    async fn client_socket_closes_promptly_when_pending_tunnel_times_out() {
        let kp = HostIdentityKeypair::generate().expect("keygen");
        let mut allowed = HashSet::new();
        allowed.insert(kp.public_key_b64.clone());

        let (addr, _state, server) = spawn_test_relay_with_limits(
            allowed,
            Duration::from_secs(5),
            MAX_CONCURRENT_CONTROL_POPS,
            Duration::from_millis(150),
        )
        .await;
        let (_daemon_ctrl, _ctrl_chan) = connect_authenticated_daemon(addr, &kp).await;

        // Client connects, but daemon intentionally ignores open_tunnel and never attaches
        let (mut client_ws, _) =
            connect_async(&format!("ws://{addr}/v1/client/{}", kp.public_key_b64))
                .await
                .expect("client connect");

        let closed = tokio::time::timeout(Duration::from_secs(2), client_ws.next())
            .await
            .expect("client socket hung instead of closing on tunnel timeout");
        assert!(
            matches!(closed, None | Some(Ok(TungMsg::Close(_))) | Some(Err(_))),
            "expected prompt close when tunnel timed out, got: {closed:?}"
        );

        server.abort();
    }

    #[tokio::test]
    async fn http_daemon_probe_is_read_only_and_never_displaces_live_daemon() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let allowed_kp = HostIdentityKeypair::generate().expect("allowed");
        let stranger_kp = HostIdentityKeypair::generate().expect("stranger");
        let mut allowed = HashSet::new();
        allowed.insert(allowed_kp.public_key_b64.clone());

        let (addr, state, server) = spawn_test_relay(allowed, Duration::from_secs(5)).await;
        let (_daemon_ctrl, _ctrl_chan) = connect_authenticated_daemon(addr, &allowed_kp).await;
        let orig_conn_id = state
            .hosts
            .read()
            .await
            .get(&allowed_kp.public_key_b64)
            .expect("registered")
            .conn_id;

        async fn raw_http_get(addr: SocketAddr, path: &str) -> (u16, String) {
            let mut stream = tokio::net::TcpStream::connect(addr).await.expect("tcp");
            let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
            stream.write_all(req.as_bytes()).await.expect("write");
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.expect("read");
            let text = String::from_utf8_lossy(&buf);
            let status = text
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|c| c.parse::<u16>().ok())
                .expect("status");
            let body = text
                .split_once("\r\n\r\n")
                .map(|(_, b)| b.to_string())
                .unwrap_or_default();
            (status, body)
        }

        // Allowlisted valid pubkey -> 200 orbit-relay-ok
        let (st, body) = raw_http_get(
            addr,
            &format!("/v1/daemon/probe?pubkey={}", allowed_kp.public_key_b64),
        )
        .await;
        assert_eq!(st, 200);
        assert_eq!(body, RELAY_PROBE_OK_BODY);

        // Unallowlisted pubkey -> 403
        let (st, _) = raw_http_get(
            addr,
            &format!("/v1/daemon/probe?pubkey={}", stranger_kp.public_key_b64),
        )
        .await;
        assert_eq!(st, 403);

        // Live daemon registration & conn_id remain completely untouched!
        let current_conn_id = state
            .hosts
            .read()
            .await
            .get(&allowed_kp.public_key_b64)
            .expect("live daemon still registered")
            .conn_id;
        assert_eq!(current_conn_id, orig_conn_id);

        // Open relay rejects malformed pubkey -> 400
        let (open_addr, _open_state, open_server) =
            spawn_test_relay(HashSet::new(), Duration::from_secs(5)).await;
        let (st, _) = raw_http_get(open_addr, "/v1/daemon/probe?pubkey=not_a_key").await;
        assert_eq!(st, 400);

        server.abort();
        open_server.abort();
    }
}
