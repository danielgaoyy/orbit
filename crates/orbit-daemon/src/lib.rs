//! Host-local Orbit Daemon (`orbit daemon`).
//!
//! Runs on each target machine (Devbox, Linux workstation, Mac mini), manages SQLite WAL state,
//! supervises detached Shims, serves direct WebSocket clients, and optionally registers outbound to Relay
//! with full `Noise_NK_25519_ChaChaPoly_SHA256` end-to-end encryption.

pub mod discovery;
pub mod git;
pub mod manager;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router,
    extract::{
        State,
        ws::{Message as AxumWsMsg, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use orbit_crypto::{DaemonHandshake, HostIdentityKeypair, PairingOffer};
use orbit_protocol::{
    HostInfo, PROTOCOL_VERSION, ProviderModelCatalog, TerminalBinaryFrame, WireMessage,
};
use orbit_providers::{
    catalogs_to_model_map, discover_installed_providers, probe_all_provider_models,
    probe_all_provider_quotas,
};
use orbit_store::OrbitStore;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex as AsyncMutex, RwLock, mpsc};
use tokio_tungstenite::{connect_async, tungstenite::Message as TungMsg};
use tracing::{info, warn};

use crate::git::{compute_git_diff, inspect_git_status, list_workspace_files};
use crate::manager::{AgentOrchestrator, DaemonBroadcast, PtyBroadcast};

const DIRECT_BIN_ZSTD_WIRE: u8 = 0x00;
const E2EE_FRAME_WIRE: u8 = 0x00;
const E2EE_FRAME_TERMINAL: u8 = 0x01;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonPersistedConfig {
    pub listen_addr: Option<SocketAddr>,
    pub relay_url: Option<String>,
}

impl DaemonPersistedConfig {
    pub fn load(home_dir: &Path) -> Self {
        std::fs::read_to_string(home_dir.join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, home_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(home_dir)?;
        let raw = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(home_dir.join("config.json"), raw)
    }
}

/// Auto-discover non-loopback LAN / Tailscale IPv4 WebSocket endpoints on the current host.
pub fn discover_lan_ws_endpoints(port: u16) -> Vec<String> {
    let mut ips = Vec::new();

    // 1. Primary outbound route interface IP via UDP connect trick (no packets sent!)
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0")
        && sock.connect("8.8.8.8:80").is_ok()
        && let Ok(local) = sock.local_addr()
        && !local.ip().is_loopback()
    {
        ips.push(local.ip().to_string());
    }

    // 2. Also scan `ip -4 -o addr show` (Linux) or `ifconfig` (macOS) for Tailscale (`100.x`) / secondary LANs
    if let Ok(out) = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
        && out.status.success()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let iface = parts[1];
                if iface == "lo" || iface.starts_with("docker") || iface.starts_with("br-") {
                    continue;
                }
                if let Some(ip_cidr) = parts.iter().skip(2).find(|p| p.contains('/'))
                    && let Some(ip) = ip_cidr.split('/').next()
                    && !ip.starts_with("127.")
                    && !ip.starts_with("198.18.")
                    && !ips.contains(&ip.to_string())
                {
                    ips.push(ip.to_string());
                }
            }
        }
    } else if let Ok(out) = std::process::Command::new("ifconfig").output()
        && out.status.success()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("inet ")
                && let Some(ip) = rest.split_whitespace().next()
                && !ip.starts_with("127.")
                && !ip.starts_with("169.254.")
                && !ip.starts_with("198.18.")
                && !ips.contains(&ip.to_string())
            {
                ips.push(ip.to_string());
            }
        }
    }

    if ips.is_empty() {
        vec![format!("ws://127.0.0.1:{port}/ws")]
    } else {
        ips.into_iter()
            .map(|ip| format!("ws://{ip}:{port}/ws"))
            .collect()
    }
}

pub struct DaemonConfig {
    pub home_dir: PathBuf,
    pub listen_addr: SocketAddr,
    pub relay_url: Option<String>,
}

pub struct DaemonRuntime {
    pub host_info: HostInfo,
    pub keypair: HostIdentityKeypair,
    pub store: Arc<OrbitStore>,
    pub orchestrator: Arc<AgentOrchestrator>,
    pub home_dir: PathBuf,
    provider_catalogs: RwLock<Option<Vec<ProviderModelCatalog>>>,
    catalog_refresh_lock: AsyncMutex<()>,
}

impl DaemonRuntime {
    pub async fn bootstrap(home_dir: &Path) -> anyhow::Result<Arc<Self>> {
        std::fs::create_dir_all(home_dir)?;
        std::fs::create_dir_all(home_dir.join("shims"))?;

        let keypair_path = home_dir.join("host-keypair.json");
        let keypair = if keypair_path.exists() {
            let raw = std::fs::read_to_string(&keypair_path)?;
            serde_json::from_str::<HostIdentityKeypair>(&raw)?
        } else {
            let generated = HostIdentityKeypair::generate()?;
            let raw = serde_json::to_string_pretty(&generated)?;
            std::fs::write(&keypair_path, raw)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&keypair_path, std::fs::Permissions::from_mode(0o600));
            }
            generated
        };

        let store = Arc::new(OrbitStore::open(home_dir.join("orbit.db"))?);
        let orchestrator = AgentOrchestrator::new(home_dir.to_path_buf(), store.clone()).await;

        let discovered = discover_installed_providers();
        let available_providers = discovered.into_iter().map(|d| d.kind).collect();

        let host_info = HostInfo {
            host_id: keypair.host_id()?,
            hostname: hostname_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            public_key_b64: keypair.public_key_b64.clone(),
            protocol_version: PROTOCOL_VERSION,
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
            available_providers,
        };

        let rt = Arc::new(Self {
            host_info,
            keypair,
            store,
            orchestrator,
            home_dir: home_dir.to_path_buf(),
            provider_catalogs: RwLock::new(None),
            catalog_refresh_lock: AsyncMutex::new(()),
        });

        let rt_warm = rt.clone();
        tokio::spawn(async move {
            let _ = rt_warm.get_or_probe_provider_catalogs(false).await;
        });

        Ok(rt)
    }

    pub async fn get_or_probe_provider_catalogs(
        &self,
        force_refresh: bool,
    ) -> Vec<ProviderModelCatalog> {
        if !force_refresh && let Some(cached) = self.provider_catalogs.read().await.clone() {
            return cached;
        }
        let _guard = self.catalog_refresh_lock.lock().await;
        if !force_refresh && let Some(cached) = self.provider_catalogs.read().await.clone() {
            return cached;
        }
        let probed = probe_all_provider_models().await;
        *self.provider_catalogs.write().await = Some(probed.clone());
        probed
    }

    pub fn build_pairing_offer(
        &self,
        direct_endpoints: Vec<String>,
        relay_url: Option<String>,
    ) -> PairingOffer {
        PairingOffer {
            host_id: self.host_info.host_id.clone(),
            hostname: self.host_info.hostname.clone(),
            daemon_pubkey_b64: self.keypair.public_key_b64.clone(),
            direct_endpoints,
            relay_url,
            pairing_secret: uuid::Uuid::new_v4().to_string(),
        }
    }
}

pub async fn run_daemon(config: DaemonConfig) -> anyhow::Result<()> {
    let mut persisted = DaemonPersistedConfig::load(&config.home_dir);
    let effective_relay = config.relay_url.or_else(|| persisted.relay_url.clone());
    let runtime = DaemonRuntime::bootstrap(&config.home_dir).await?;
    let listener = match tokio::net::TcpListener::bind(config.listen_addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && config.listen_addr.port() != 0 => {
            let mut fallback_listener = None;
            for candidate_port in 6868u16..=6999 {
                if candidate_port == config.listen_addr.port() {
                    continue;
                }
                let addr = SocketAddr::new(config.listen_addr.ip(), candidate_port);
                if let Ok(l) = tokio::net::TcpListener::bind(addr).await {
                    warn!(
                        requested = %config.listen_addr,
                        rebound = %addr,
                        "Requested port already in use by another user/process; automatically isolated onto free port"
                    );
                    persisted.listen_addr = Some(addr);
                    let _ = persisted.save(&config.home_dir);
                    fallback_listener = Some(l);
                    break;
                }
            }
            match fallback_listener {
                Some(l) => l,
                None => return Err(e.into()),
            }
        }
        Err(e) => return Err(e.into()),
    };
    let bound_addr = listener.local_addr()?;

    let direct_eps = if bound_addr.ip().is_unspecified() {
        discover_lan_ws_endpoints(bound_addr.port())
    } else {
        vec![format!("ws://{bound_addr}/ws")]
    };
    let offer = runtime.build_pairing_offer(direct_eps, effective_relay.clone());

    info!(
        host_id = %runtime.host_info.host_id,
        hostname = %runtime.host_info.hostname,
        providers = ?runtime.host_info.available_providers,
        listen = %bound_addr,
        relay = ?effective_relay,
        "Orbit Daemon ready"
    );
    if let Ok(uri) = offer.to_qr_uri() {
        info!(pairing_uri = %uri, "Scan or import this Multi-Host pairing URI on Orbit Mobile");
    }

    if let Some(relay_url) = effective_relay {
        let rt_relay = runtime.clone();
        tokio::spawn(async move {
            outbound_relay_loop(rt_relay, relay_url).await;
        });
    }

    let app = Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/ws", get(ws_handler))
        .with_state(runtime);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Spawn an embedded Daemon on a dynamic local port (ideal for integration tests & local attach).
pub async fn spawn_embedded_daemon(
    home_dir: PathBuf,
    relay_url: Option<String>,
) -> anyhow::Result<(SocketAddr, Arc<DaemonRuntime>)> {
    let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    if let Some(r_url) = relay_url {
        let rt_relay = runtime.clone();
        tokio::spawn(async move {
            outbound_relay_loop(rt_relay, r_url).await;
        });
    }

    let rt_http = runtime.clone();
    let app = Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/ws", get(ws_handler))
        .with_state(rt_http);

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok((addr, runtime))
}

async fn healthz_handler(State(runtime): State<Arc<DaemonRuntime>>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "ok",
        "host_id": runtime.host_info.host_id,
        "version": runtime.host_info.daemon_version,
    }))
}

// --- Outbound Relay Control & Noise NK E2EE Tunnel Spawner ---

async fn outbound_relay_loop(runtime: Arc<DaemonRuntime>, relay_base: String) {
    let base = relay_base.trim_end_matches('/').to_string();
    let pubkey = runtime.keypair.public_key_b64.clone();
    let ctrl_url = format!("{base}/v1/daemon/control?pubkey={pubkey}");

    loop {
        match connect_async(&ctrl_url).await {
            Ok((ws, _)) => {
                info!(relay = %base, "Connected outbound to Orbit Relay");
                let (mut sink, mut stream) = ws.split();
                while let Some(Ok(msg)) = stream.next().await {
                    match msg {
                        TungMsg::Text(txt) => {
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt)
                                && v.get("op").and_then(|x| x.as_str()) == Some("open_tunnel")
                                && let Some(tid) = v.get("tunnel_id").and_then(|x| x.as_str())
                            {
                                let rt_tun = runtime.clone();
                                let base_tun = base.clone();
                                let tid_owned = tid.to_string();
                                tokio::spawn(async move {
                                    let _ = serve_e2ee_relay_tunnel(rt_tun, &base_tun, &tid_owned)
                                        .await;
                                });
                            }
                        }
                        TungMsg::Ping(p) => {
                            let _ = sink.send(TungMsg::Pong(p)).await;
                        }
                        TungMsg::Close(_) => break,
                        _ => {}
                    }
                }
                warn!(relay = %base, "Disconnected from Relay, reconnecting in 2s...");
            }
            Err(e) => {
                warn!(relay = %base, error = %e, "Relay connection failed, retrying in 2s...");
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn serve_e2ee_relay_tunnel(
    runtime: Arc<DaemonRuntime>,
    relay_base: &str,
    tunnel_id: &str,
) -> anyhow::Result<()> {
    let pubkey = &runtime.keypair.public_key_b64;
    let priv_bytes = runtime.keypair.private_bytes()?;
    let tun_url = format!("{relay_base}/v1/daemon/tunnel?tunnel_id={tunnel_id}&pubkey={pubkey}");

    let (ws, _) = connect_async(&tun_url).await?;
    let (mut sink, mut stream) = ws.split();

    // Receive Noise NK Message 1 from Client
    let msg1 = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(m)) = stream.next().await {
            if let TungMsg::Binary(b) = m {
                return Some(b);
            }
        }
        None
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("tunnel closed before Noise msg1"))?;

    let (chan, msg2) = DaemonHandshake::accept(&priv_bytes, &msg1)?;
    sink.send(TungMsg::Binary(msg2.into())).await?;

    let shared_chan = Arc::new(tokio::sync::Mutex::new(chan));
    let (in_tx, in_rx) = mpsc::channel::<SessionFrame>(256);
    let (out_tx, mut out_rx) = mpsc::channel::<SessionFrame>(256);

    let chan_writer = shared_chan.clone();
    tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            let plain = match frame {
                SessionFrame::Wire { msg, .. } => match msg.encode_zstd(3) {
                    Ok(z) => {
                        let mut p = Vec::with_capacity(1 + z.len());
                        p.push(E2EE_FRAME_WIRE);
                        p.extend_from_slice(&z);
                        p
                    }
                    Err(_) => continue,
                },
                SessionFrame::Terminal(t) => {
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
            if sink.send(TungMsg::Binary(ct.into())).await.is_err() {
                break;
            }
        }
    });

    let chan_reader = shared_chan;
    tokio::spawn(async move {
        while let Some(Ok(m)) = stream.next().await {
            if let TungMsg::Binary(ct) = m {
                let Ok(plain) = chan_reader.lock().await.open_record(&ct) else {
                    break;
                };
                if plain.is_empty() {
                    continue;
                }
                match plain[0] {
                    E2EE_FRAME_WIRE => {
                        if let Ok(wire) = WireMessage::decode_zstd(&plain[1..])
                            && in_tx
                                .send(SessionFrame::Wire {
                                    msg: wire,
                                    zstd: true,
                                })
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                    E2EE_FRAME_TERMINAL => {
                        if let Ok(term) = TerminalBinaryFrame::decode(&plain[1..])
                            && in_tx.send(SessionFrame::Terminal(term)).await.is_err()
                        {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    run_client_session_loop(runtime, in_rx, out_tx).await;
    Ok(())
}

// --- Direct Axum WebSocket Handler ---

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(runtime): State<Arc<DaemonRuntime>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_direct_socket(socket, runtime))
}

async fn handle_direct_socket(socket: WebSocket, runtime: Arc<DaemonRuntime>) {
    let (mut sink, mut stream) = socket.split();
    let (in_tx, in_rx) = mpsc::channel::<SessionFrame>(256);
    let (out_tx, mut out_rx) = mpsc::channel::<SessionFrame>(256);

    tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            let ws_msg = match frame {
                SessionFrame::Wire { msg, zstd: true } => match msg.encode_zstd(3) {
                    Ok(z) => {
                        let mut b = Vec::with_capacity(1 + z.len());
                        b.push(DIRECT_BIN_ZSTD_WIRE);
                        b.extend_from_slice(&z);
                        AxumWsMsg::Binary(b.into())
                    }
                    Err(_) => continue,
                },
                SessionFrame::Wire { msg, zstd: false } => match serde_json::to_string(&msg) {
                    Ok(txt) => AxumWsMsg::Text(txt.into()),
                    Err(_) => continue,
                },
                SessionFrame::Terminal(t) => AxumWsMsg::Binary(t.encode().into()),
            };
            if sink.send(ws_msg).await.is_err() {
                break;
            }
        }
    });

    tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                AxumWsMsg::Text(txt) => {
                    if let Ok(wire) = serde_json::from_str::<WireMessage>(&txt)
                        && in_tx
                            .send(SessionFrame::Wire {
                                msg: wire,
                                zstd: false,
                            })
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
                AxumWsMsg::Binary(bin) if !bin.is_empty() => {
                    if bin[0] == DIRECT_BIN_ZSTD_WIRE {
                        if let Ok(wire) = WireMessage::decode_zstd(&bin[1..])
                            && in_tx
                                .send(SessionFrame::Wire {
                                    msg: wire,
                                    zstd: true,
                                })
                                .await
                                .is_err()
                        {
                            break;
                        }
                    } else if let Ok(term) = TerminalBinaryFrame::decode(&bin)
                        && in_tx.send(SessionFrame::Terminal(term)).await.is_err()
                    {
                        break;
                    }
                }
                AxumWsMsg::Close(_) => break,
                _ => {}
            }
        }
    });

    run_client_session_loop(runtime, in_rx, out_tx).await;
}

// --- Unified Transport-Neutral Session Controller (shared by Direct WS & Relay E2EE) ---

#[allow(clippy::large_enum_variant)]
enum SessionFrame {
    Wire { msg: WireMessage, zstd: bool },
    Terminal(TerminalBinaryFrame),
}

async fn run_client_session_loop(
    runtime: Arc<DaemonRuntime>,
    mut in_rx: mpsc::Receiver<SessionFrame>,
    out_tx: mpsc::Sender<SessionFrame>,
) {
    let mut use_zstd = false;
    let mut subscribed_agents: HashSet<String> = HashSet::new();
    let mut slot_to_terminal: HashMap<u8, (String, tokio::task::AbortHandle)> = HashMap::new();
    let mut bcast_rx = runtime.orchestrator.broadcast_tx.subscribe();

    loop {
        tokio::select! {
            maybe_in = in_rx.recv() => {
                let Some(frame) = maybe_in else { break; };
                match frame {
                    SessionFrame::Terminal(tframe) => {
                        match tframe {
                            TerminalBinaryFrame::Input { slot, data } => {
                                if let Some((tid, _)) = slot_to_terminal.get(&slot) {
                                    runtime.orchestrator.write_terminal_input(tid, data).await;
                                }
                            }
                            TerminalBinaryFrame::Resize { slot, cols, rows } => {
                                if let Some((tid, _)) = slot_to_terminal.get(&slot) {
                                    runtime.orchestrator.resize_terminal(tid, cols, rows).await;
                                }
                            }
                            _ => {}
                        }
                    }
                    SessionFrame::Wire { msg, zstd } => {
                        if zstd {
                            use_zstd = true;
                        }
                        if let Some(reply) = handle_wire_rpc(
                            &runtime,
                            msg,
                            &mut use_zstd,
                            &mut subscribed_agents,
                            &mut slot_to_terminal,
                            &out_tx,
                        )
                        .await
                            && out_tx
                                .send(SessionFrame::Wire {
                                    msg: reply,
                                    zstd: use_zstd,
                                })
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                }
            }

            Ok(bcast) = bcast_rx.recv() => {
                match bcast {
                    DaemonBroadcast::AgentUpdated(agent) => {
                        if out_tx
                            .send(SessionFrame::Wire {
                                msg: WireMessage::AgentUpdated { agent },
                                zstd: use_zstd,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    DaemonBroadcast::TimelineLive(event) => {
                        if subscribed_agents.contains(&event.agent_id)
                            && out_tx
                                .send(SessionFrame::Wire {
                                    msg: WireMessage::TimelineLive { event },
                                    zstd: use_zstd,
                                })
                                .await
                                .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        }
    }

    for (_, (_, abort_handle)) in slot_to_terminal.drain() {
        abort_handle.abort();
    }
}

async fn handle_wire_rpc(
    runtime: &Arc<DaemonRuntime>,
    req: WireMessage,
    use_zstd: &mut bool,
    subscribed_agents: &mut HashSet<String>,
    slot_to_terminal: &mut HashMap<u8, (String, tokio::task::AbortHandle)>,
    out_tx: &mpsc::Sender<SessionFrame>,
) -> Option<WireMessage> {
    match req {
        WireMessage::Hello { supports_zstd, .. } => {
            *use_zstd = supports_zstd;
            Some(WireMessage::Welcome {
                host: runtime.host_info.clone(),
            })
        }
        WireMessage::Ping { ts_ms } => Some(WireMessage::Pong { ts_ms }),

        // Workspaces & Git
        WireMessage::DiscoverProjects { request_id } => {
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let projects = tokio::task::spawn_blocking(move || {
                    crate::discovery::discover_host_projects(&store)
                })
                .await
                .unwrap_or_default();
                let _ = tx
                    .send(SessionFrame::Wire {
                        msg: WireMessage::DiscoveredProjectsSnapshot {
                            request_id,
                            projects,
                        },
                        zstd,
                    })
                    .await;
            });
            None
        }
        WireMessage::ListDirectory { request_id, path } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let (current_path, parent_path, entries) = tokio::task::spawn_blocking(move || {
                    crate::discovery::list_directory_entries(path.as_deref())
                })
                .await
                .unwrap_or_else(|_| ("/".into(), None, Vec::new()));
                let _ = tx
                    .send(SessionFrame::Wire {
                        msg: WireMessage::DirectoryEntriesResponse {
                            request_id,
                            current_path,
                            parent_path,
                            entries,
                        },
                        zstd,
                    })
                    .await;
            });
            None
        }
        WireMessage::ListModels { request_id } => {
            let rt = Arc::clone(runtime);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let catalogs = rt.get_or_probe_provider_catalogs(false).await;
                let _ = tx
                    .send(SessionFrame::Wire {
                        msg: WireMessage::ModelsSnapshot {
                            request_id,
                            models: catalogs_to_model_map(&catalogs),
                        },
                        zstd,
                    })
                    .await;
            });
            None
        }
        WireMessage::ListWorkspaces { request_id } => match runtime.store.list_workspaces() {
            Ok(workspaces) => Some(WireMessage::WorkspacesSnapshot {
                request_id,
                workspaces,
            }),
            Err(e) => Some(rpc_err(request_id, "store_error", e)),
        },
        WireMessage::CreateWorkspace {
            request_id,
            source_cwd,
            title,
            worktree_branch,
            base_ref,
        } => match runtime
            .orchestrator
            .ensure_workspace(&source_cwd, title, worktree_branch, base_ref)
            .await
        {
            Ok(workspace) => Some(WireMessage::WorkspaceCreated {
                request_id,
                workspace,
            }),
            Err(e) => Some(rpc_err(request_id, "workspace_error", e)),
        },
        WireMessage::GetGitStatus { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let mut status = inspect_git_status(&resolved).await;
                status.cwd = cwd;
                let _ = tx
                    .send(SessionFrame::Wire {
                        msg: WireMessage::GitStatusResponse { request_id, status },
                        zstd,
                    })
                    .await;
            });
            None
        }
        WireMessage::GetGitDiff {
            request_id,
            cwd,
            path,
            staged,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match compute_git_diff(&resolved, path.as_deref(), staged).await {
                    Ok(unified_diff) => WireMessage::GitDiffResponse {
                        request_id,
                        cwd,
                        path,
                        unified_diff,
                    },
                    Err(e) => rpc_err(request_id, "git_diff_error", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ListWorkspaceFiles { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let files = list_workspace_files(&resolved, 2500).await;
                let _ = tx
                    .send(SessionFrame::Wire {
                        msg: WireMessage::WorkspaceFilesResponse {
                            request_id,
                            cwd,
                            files,
                        },
                        zstd,
                    })
                    .await;
            });
            None
        }

        // Agents
        WireMessage::ListAgents {
            request_id,
            workspace_id,
        } => match runtime.store.list_agents(workspace_id.as_deref()) {
            Ok(agents) => Some(WireMessage::AgentsSnapshot { request_id, agents }),
            Err(e) => Some(rpc_err(request_id, "store_error", e)),
        },
        WireMessage::CreateAgent {
            request_id,
            cwd,
            extra_dirs,
            title,
            provider,
            model,
            effort,
            mode,
            initial_prompt,
            initial_display_prompt,
            initial_badges,
            worktree,
        } => {
            match runtime
                .orchestrator
                .create_and_start_agent(crate::manager::CreateAgentSpec {
                    cwd,
                    extra_dirs,
                    title,
                    provider,
                    model,
                    effort,
                    mode,
                    initial_prompt,
                    initial_display_prompt,
                    initial_badges,
                    worktree,
                })
                .await
            {
                Ok(agent) => {
                    subscribed_agents.insert(agent.agent_id.clone());
                    Some(WireMessage::AgentCreated { request_id, agent })
                }
                Err(e) => Some(rpc_err(request_id, "create_agent_failed", e)),
            }
        }
        WireMessage::SendPrompt {
            request_id,
            agent_id,
            prompt,
            display_prompt,
            badges,
        } => {
            subscribed_agents.insert(agent_id.clone());
            match runtime
                .orchestrator
                .send_prompt_with_meta(&agent_id, prompt, display_prompt, badges)
                .await
            {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "send_prompt_failed", e)),
            }
        }
        WireMessage::InterruptTurn {
            request_id,
            agent_id,
        } => match runtime.orchestrator.interrupt_agent(&agent_id).await {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "interrupt_failed", e)),
        },
        WireMessage::SetSessionMode {
            request_id,
            agent_id,
            mode,
        } => match runtime.orchestrator.set_session_mode(&agent_id, mode).await {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "set_mode_failed", e)),
        },
        WireMessage::SetAgentConfig {
            request_id,
            agent_id,
            model,
            effort,
        } => match runtime
            .orchestrator
            .set_agent_config(&agent_id, model, effort)
            .await
        {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "set_config_failed", e)),
        },
        WireMessage::SetAgentModel {
            request_id,
            agent_id,
            model,
        } => match runtime.orchestrator.set_agent_model(&agent_id, model).await {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "set_model_failed", e)),
        },
        WireMessage::SwitchAgentProvider {
            request_id,
            agent_id,
            provider,
            model,
            effort,
        } => match runtime
            .orchestrator
            .switch_agent_provider(&agent_id, provider, model, effort)
            .await
        {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "switch_provider_failed", e)),
        },
        WireMessage::ArchiveAgent {
            request_id,
            agent_id,
        } => {
            subscribed_agents.remove(&agent_id);
            match runtime.orchestrator.archive_agent(&agent_id).await {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "archive_failed", e)),
            }
        }
        WireMessage::ResolvePermission {
            request_id,
            agent_id,
            permission_request_id,
            decision,
        } => match runtime
            .orchestrator
            .resolve_permission(&agent_id, permission_request_id, decision)
            .await
        {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "resolve_permission_failed", e)),
        },
        WireMessage::AnswerQuestion {
            request_id,
            agent_id,
            question_request_id,
            answers,
        } => match runtime
            .orchestrator
            .answer_question(&agent_id, question_request_id, answers)
            .await
        {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "answer_question_failed", e)),
        },
        WireMessage::SubscribeTimeline {
            request_id,
            agent_id,
            since_seq,
        } => {
            subscribed_agents.insert(agent_id.clone());
            match runtime
                .store
                .fetch_timeline_since(&agent_id, since_seq, 2000)
            {
                Ok((events, has_more)) => Some(WireMessage::TimelineCatchup {
                    request_id,
                    agent_id,
                    events,
                    has_more,
                }),
                Err(e) => Some(rpc_err(request_id, "timeline_error", e)),
            }
        }

        // Terminals
        WireMessage::ListTerminals { request_id } => {
            let terminals = runtime.orchestrator.list_live_terminals().await;
            Some(WireMessage::TerminalsSnapshot {
                request_id,
                terminals,
            })
        }
        WireMessage::CreateTerminal {
            request_id,
            cwd,
            title,
            cols,
            rows,
        } => match runtime
            .orchestrator
            .create_terminal(cwd, title, cols, rows)
            .await
        {
            Ok(terminal) => {
                let slot = (1u8..=250u8)
                    .find(|s| !slot_to_terminal.contains_key(s))
                    .unwrap_or(1u8);
                bind_terminal_stream_to_client(
                    runtime,
                    &terminal.terminal_id,
                    slot,
                    slot_to_terminal,
                    out_tx,
                )
                .await;
                Some(WireMessage::TerminalAttached {
                    request_id,
                    terminal,
                    slot,
                })
            }
            Err(e) => Some(rpc_err(request_id, "create_terminal_failed", e)),
        },
        WireMessage::AttachTerminal {
            request_id,
            terminal_id,
            slot,
        } => {
            if let Some(terminal) = bind_terminal_stream_to_client(
                runtime,
                &terminal_id,
                slot,
                slot_to_terminal,
                out_tx,
            )
            .await
            {
                Some(WireMessage::TerminalAttached {
                    request_id,
                    terminal,
                    slot,
                })
            } else {
                Some(rpc_err(
                    request_id,
                    "terminal_not_found",
                    "terminal does not exist",
                ))
            }
        }
        WireMessage::KillTerminal {
            request_id,
            terminal_id,
        } => {
            slot_to_terminal.retain(|_, (tid, handle)| {
                if tid == &terminal_id {
                    handle.abort();
                    false
                } else {
                    true
                }
            });
            match runtime.orchestrator.kill_terminal(&terminal_id).await {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "kill_terminal_failed", e)),
            }
        }

        // Quotas & Provider Model Catalogs (spawned off-loop so slow CLI probes never block RPC or PTY!)
        WireMessage::RefreshQuotas { request_id } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let snapshots = probe_all_provider_quotas().await;
                let _ = tx
                    .send(SessionFrame::Wire {
                        msg: WireMessage::QuotaReport {
                            request_id: Some(request_id),
                            snapshots,
                        },
                        zstd,
                    })
                    .await;
            });
            None
        }
        WireMessage::ListProviderModels {
            request_id,
            force_refresh,
        } => {
            let rt = Arc::clone(runtime);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let catalogs = rt.get_or_probe_provider_catalogs(force_refresh).await;
                let _ = tx
                    .send(SessionFrame::Wire {
                        msg: WireMessage::ProviderModelsSnapshot {
                            request_id,
                            catalogs,
                        },
                        zstd,
                    })
                    .await;
            });
            None
        }
        _ => None,
    }
}

pub(crate) fn expand_host_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = trimmed.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(trimmed)
}

async fn bind_terminal_stream_to_client(
    runtime: &Arc<DaemonRuntime>,
    terminal_id: &str,
    slot: u8,
    slot_to_terminal: &mut HashMap<u8, (String, tokio::task::AbortHandle)>,
    out_tx: &mpsc::Sender<SessionFrame>,
) -> Option<orbit_protocol::TerminalSummary> {
    let (summary, snap, mut rx) = runtime.orchestrator.attach_terminal(terminal_id).await?;
    if let Some((_, prev_handle)) = slot_to_terminal.remove(&slot) {
        prev_handle.abort();
    }

    let tx_clone = out_tx.clone();
    let join_handle = tokio::spawn(async move {
        if !snap.is_empty()
            && tx_clone
                .send(SessionFrame::Terminal(TerminalBinaryFrame::Snapshot {
                    slot,
                    data: snap,
                }))
                .await
                .is_err()
        {
            return;
        }
        while let Ok(ev) = rx.recv().await {
            match ev {
                PtyBroadcast::Snapshot(data) => {
                    if tx_clone
                        .send(SessionFrame::Terminal(TerminalBinaryFrame::Snapshot {
                            slot,
                            data,
                        }))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                PtyBroadcast::Output(data) => {
                    if tx_clone
                        .send(SessionFrame::Terminal(TerminalBinaryFrame::Output {
                            slot,
                            data,
                        }))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                PtyBroadcast::Closed => break,
            }
        }
    });
    slot_to_terminal.insert(slot, (terminal_id.to_string(), join_handle.abort_handle()));

    Some(summary)
}

fn rpc_err(request_id: uuid::Uuid, code: &str, message: impl ToString) -> WireMessage {
    WireMessage::RpcError {
        request_id: Some(request_id),
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn hostname_string() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "orbit-host".to_string())
}
