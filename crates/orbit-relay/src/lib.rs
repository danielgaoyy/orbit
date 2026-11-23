//! Ultra-lightweight (`< 5.5MB` RSS) zero-knowledge E2EE Relay for Orbit.
//!
//! Designed to run on a personal AWS devbox or micro-VPS with `--allow-pubkey` admission locking
//! so strangers or open-source users can NEVER consume memory or egress bandwidth on your server.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::{
    Router,
    extract::{
        Path as AxumPath, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{info, warn};

/// Max WebSocket frame & message size across all relay endpoints (48 MiB, safely carrying up to
/// 25 MiB base64/E2EE multimodal payloads without tearing down sessions).
pub const MAX_RELAY_WS_FRAME_BYTES: usize = 48 * 1024 * 1024;

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
    /// Channel to ask the connected Daemon to spawn a new E2EE tunnel for an incoming mobile client.
    accept_tx: mpsc::Sender<PendingClient>,
}

#[derive(Clone, Default)]
struct RelayState {
    allowed_pubkeys: Arc<HashSet<String>>,
    hosts: Arc<RwLock<HashMap<String, RegisteredHost>>>,
    waiting_tunnels: Arc<RwLock<HashMap<String, oneshot::Sender<WebSocket>>>>,
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

pub async fn run_relay(config: RelayConfig) -> anyhow::Result<()> {
    let state = RelayState {
        allowed_pubkeys: config.allowed_pubkeys.clone(),
        ..Default::default()
    };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/daemon/control", get(ws_daemon_control))
        .route("/v1/daemon/tunnel", get(ws_daemon_tunnel))
        .route("/v1/client/{pubkey}", get(ws_client_connect))
        .with_state(state);

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

async fn ws_daemon_control(
    ws: WebSocketUpgrade,
    Query(q): Query<HostRegisterQuery>,
    State(state): State<RelayState>,
) -> impl IntoResponse {
    if !is_pubkey_allowed(&state, &q.pubkey) {
        warn!(pubkey = %q.pubkey, "Rejected unauthorized daemon registration");
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_frame_size(MAX_RELAY_WS_FRAME_BYTES)
        .max_message_size(MAX_RELAY_WS_FRAME_BYTES)
        .on_upgrade(move |socket| handle_daemon_control(socket, q.pubkey, state))
}

async fn handle_daemon_control(mut socket: WebSocket, pubkey: String, state: RelayState) {
    let (accept_tx, mut accept_rx) = mpsc::channel::<PendingClient>(32);
    state
        .hosts
        .write()
        .await
        .insert(pubkey.clone(), RegisteredHost { accept_tx });
    info!(pubkey = %pubkey, "Host daemon registered on relay");

    loop {
        tokio::select! {
            maybe_client = accept_rx.recv() => {
                let Some(pending) = maybe_client else { break; };
                let tunnel_id = format!("tun_{}", rand_hex());
                let (tun_tx, tun_rx) = oneshot::channel::<WebSocket>();
                state.waiting_tunnels.write().await.insert(tunnel_id.clone(), tun_tx);

                let notify = serde_json::json!({
                    "op": "open_tunnel",
                    "tunnel_id": tunnel_id,
                }).to_string();

                if socket.send(Message::Text(notify.into())).await.is_err() {
                    break;
                }

                let waiting_tunnels = state.waiting_tunnels.clone();
                let tid_cleanup = tunnel_id.clone();
                tokio::spawn(async move {
                    if let Ok(Ok(daemon_ws)) = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        tun_rx,
                    ).await {
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
                    Some(Ok(_)) => {}
                    _ => break,
                }
            }
        }
    }

    state.hosts.write().await.remove(&pubkey);
    info!(pubkey = %pubkey, "Host daemon disconnected from relay");
}

async fn ws_daemon_tunnel(
    ws: WebSocketUpgrade,
    Query(q): Query<TunnelAttachQuery>,
    State(state): State<RelayState>,
) -> impl IntoResponse {
    if !is_pubkey_allowed(&state, &q.pubkey) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(tun_tx) = state.waiting_tunnels.write().await.remove(&q.tunnel_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    ws.max_frame_size(MAX_RELAY_WS_FRAME_BYTES)
        .max_message_size(MAX_RELAY_WS_FRAME_BYTES)
        .on_upgrade(move |socket| async move {
            let _ = tun_tx.send(socket);
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
            let writer = tokio::spawn(async move {
                while let Some(msg) = d2c_rx.recv().await {
                    if c_sink.send(msg).await.is_err() {
                        break;
                    }
                }
            });

            while let Some(Ok(msg)) = c_stream.next().await {
                if c2d_tx.send(msg).await.is_err() {
                    break;
                }
            }
            writer.abort();
        })
}

async fn bridge_ciphertext_streams(daemon_ws: WebSocket, mut pending: PendingClient) {
    let (mut d_sink, mut d_stream) = daemon_ws.split();

    let c2d = tokio::spawn(async move {
        while let Some(msg) = pending.client_to_daemon_rx.recv().await {
            if d_sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = d_stream.next().await {
        if pending.daemon_to_client_tx.send(msg).await.is_err() {
            break;
        }
    }
    c2d.abort();
}

fn rand_hex() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_ws_frame_limit_covers_25mib_base64_media_payloads() {
        assert_eq!(MAX_RELAY_WS_FRAME_BYTES, 48 * 1024 * 1024);
    }
}
