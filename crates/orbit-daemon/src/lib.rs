//! Host-local Orbit Daemon (`orbit daemon`).
//!
//! Runs on each target machine (Devbox, Linux workstation, Mac mini), manages SQLite WAL state,
//! supervises detached Shims, serves direct WebSocket clients, exposes the built-in Streamable
//! HTTP MCP orchestration server (`/mcp/agents`), runs the Cron/Heartbeat scheduler, maintains
//! Daemon-to-Daemon Connected Host links, and optionally registers outbound to Relay with full
//! `Noise_NK_25519_ChaChaPoly_SHA256` end-to-end encryption.

pub mod attachments;
pub mod connected_hosts;
pub mod cron;
pub mod discovery;
pub mod finish_subscriptions;
pub mod git;
pub mod idle_cleanup;
pub mod manager;
pub mod mcp;
pub mod messaging;
pub mod scheduler;
pub mod skills;
pub mod tools;
pub mod workspace_board;
pub mod workspace_cluster;
pub mod workspace_organizer;
pub mod workspace_runtime;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router,
    body::Body,
    extract::{
        State,
        ws::{Message as AxumWsMsg, WebSocket, WebSocketUpgrade},
    },
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use orbit_crypto::{DaemonHandshake, HostIdentityKeypair, PairingOffer};
use orbit_protocol::{
    AgentMessageKind, HostInfo, OrchestrationConfig, PROTOCOL_VERSION, ProviderModelCatalog,
    ScheduleStatus, TerminalBinaryFrame, WireMessage,
};
use orbit_providers::{
    SideQuestionSpec, catalogs_to_model_map, discover_installed_providers,
    probe_all_provider_models, probe_all_provider_quotas, run_side_question,
};
use orbit_relay_transport::{RelayConnector, is_ssh_relay_url, redact_relay_url};
use orbit_store::{OrbitStore, ReceiptOutcome};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, RwLock, mpsc};
use tokio_tungstenite::tungstenite::Message as TungMsg;
use tracing::{info, warn};
use uuid::Uuid;

use crate::attachments::AttachmentStagingManager;
use crate::connected_hosts::ConnectedHostsManager;
use crate::finish_subscriptions::FinishSubscriptionManager;
use crate::git::{compute_git_diff, inspect_git_status, list_workspace_files, read_workspace_file};
use crate::idle_cleanup::IdleRuntimeCleanup;
use crate::manager::{
    AgentOrchestrator, CreateAgentSpec, CreateWorkspaceSpec, DaemonBroadcast, PtyBroadcast,
};
use crate::messaging::{
    AgentMessageDelivery, AgentPromptSource, DEFAULT_COALESCE_WINDOW, QueuedAgentMessage,
};
use crate::scheduler::ScheduleEngine;
use crate::workspace_runtime::WorkspaceScriptRegistry;

const DIRECT_BIN_ZSTD_WIRE: u8 = 0x00;
const E2EE_FRAME_WIRE: u8 = 0x00;
const E2EE_FRAME_TERMINAL: u8 = 0x01;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonPersistedConfig {
    pub listen_addr: Option<SocketAddr>,
    pub relay_url: Option<String>,
    #[serde(default)]
    pub orchestration: OrchestrationConfig,
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
        let config_path = home_dir.join("config.json");
        std::fs::write(&config_path, raw)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
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
    pub attachments: Arc<AttachmentStagingManager>,
    pub home_dir: PathBuf,
    pub mcp_token: String,
    pub listen_port: Arc<RwLock<u16>>,
    pub orch_config: Arc<RwLock<OrchestrationConfig>>,
    pub delivery: Arc<AgentMessageDelivery>,
    pub finish_subs: Arc<FinishSubscriptionManager>,
    pub connected_hosts: Arc<ConnectedHostsManager>,
    pub scheduler: Arc<ScheduleEngine>,
    pub idle_cleanup: Arc<IdleRuntimeCleanup>,
    provider_catalogs: RwLock<Option<Vec<ProviderModelCatalog>>>,
    catalog_refresh_lock: AsyncMutex<()>,
    upgrade_lock: AsyncMutex<()>,
}

impl DaemonRuntime {
    /// Lightweight, side-effect-free runtime bootstrap (safe for `orbit pair`, `orbit status`,
    /// and tests). Call [`DaemonRuntime::activate_server`] after binding the TCP listener to start
    /// shim recovery, connected hosts, finish subscriptions, scheduler, and idle cleanup.
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

        let persisted = DaemonPersistedConfig::load(home_dir);
        let mcp_token = crate::mcp::load_or_create_mcp_token(home_dir)?;
        let listen_port = Arc::new(RwLock::new(
            persisted.listen_addr.map(|a| a.port()).unwrap_or(0),
        ));
        let orch_config = Arc::new(RwLock::new(persisted.orchestration));

        let host_id = keypair.host_id()?;
        let store = Arc::new(OrbitStore::open(home_dir.join("orbit.db"))?);
        let scripts = WorkspaceScriptRegistry::new();
        let orchestrator = AgentOrchestrator::new(
            home_dir.to_path_buf(),
            host_id.clone(),
            mcp_token.clone(),
            listen_port.clone(),
            orch_config.clone(),
            store.clone(),
            scripts,
        )
        .await;
        let attachments = Arc::new(AttachmentStagingManager::new(home_dir));

        let orch_for_dispatch = orchestrator.clone();
        let delivery = AgentMessageDelivery::new(
            DEFAULT_COALESCE_WINDOW,
            Arc::new(move |batch| {
                let orch = orch_for_dispatch.clone();
                Box::pin(async move { orch.dispatch_coalesced_batch(batch).await })
            }),
        );

        let connected_hosts = ConnectedHostsManager::new(host_id.clone(), store.clone());
        let finish_subs = FinishSubscriptionManager::new(
            host_id.clone(),
            store.clone(),
            orchestrator.clone(),
            delivery.clone(),
            connected_hosts.clone(),
        );
        let scheduler = ScheduleEngine::new(store.clone(), orchestrator.clone(), delivery.clone());
        let idle_cleanup = IdleRuntimeCleanup::new(
            store.clone(),
            orchestrator.clone(),
            delivery.clone(),
            finish_subs.clone(),
            orch_config.clone(),
        );

        let discovered = discover_installed_providers();
        let available_providers = discovered.into_iter().map(|d| d.kind).collect();

        let host_info = HostInfo {
            host_id,
            hostname: hostname_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            public_key_b64: keypair.public_key_b64.clone(),
            protocol_version: PROTOCOL_VERSION,
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
            available_providers,
        };

        Ok(Arc::new(Self {
            host_info,
            keypair,
            store,
            orchestrator,
            attachments,
            home_dir: home_dir.to_path_buf(),
            mcp_token,
            listen_port,
            orch_config,
            delivery,
            finish_subs,
            connected_hosts,
            scheduler,
            idle_cleanup,
            provider_catalogs: RwLock::new(None),
            catalog_refresh_lock: AsyncMutex::new(()),
            upgrade_lock: AsyncMutex::new(()),
        }))
    }

    /// Record the active bound TCP port and activate all background orchestration loops.
    pub async fn activate_server(self: &Arc<Self>, bound_port: u16) {
        *self.listen_port.write().await = bound_port;
        write_live_port_file(&self.home_dir, bound_port);

        // 1. Reattach surviving shims now that the MCP server port is known
        self.orchestrator.activate_runtime().await;
        // 2. Establish outbound Daemon-to-Daemon mesh connections
        self.connected_hosts.start_all().await;
        // 3. Resume any persisted finish subscriptions across restarts
        self.finish_subs.start_recovery().await;
        // 4. Start Cron/Heartbeat scheduler & Idle Runtime Cleanup background loops
        drop(self.scheduler.spawn_background());
        drop(self.idle_cleanup.spawn_background());

        // 5. Warm provider model catalogs in background
        let rt_warm = Arc::clone(self);
        tokio::spawn(async move {
            let _ = rt_warm.get_or_probe_provider_catalogs(false).await;
        });

        // 6. Refresh installed bundled orchestration skills if previously enabled
        let installed_skills = self.orch_config.read().await.installed_skills.clone();
        if !installed_skills.is_empty() {
            tokio::task::spawn_blocking(move || {
                let user_home = resolve_user_home();
                let _ = crate::skills::sync_orchestration_skills(&user_home, &installed_skills);
            });
        }
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

fn write_live_port_file(home_dir: &Path, port: u16) {
    let port_file = home_dir.join("daemon-port");
    let _ = std::fs::write(&port_file, format!("{port}\n"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&port_file, std::fs::Permissions::from_mode(0o600));
    }
}

/// Read the active local Daemon port (prefers `$ORBIT_HOME/daemon-port`, falling back to `config.json` or `6868`).
pub fn read_live_daemon_port(home_dir: &Path) -> u16 {
    if let Ok(raw) = std::fs::read_to_string(home_dir.join("daemon-port"))
        && let Ok(port) = raw.trim().parse::<u16>()
        && port > 0
    {
        return port;
    }
    DaemonPersistedConfig::load(home_dir)
        .listen_addr
        .map(|a| a.port())
        .filter(|&p| p > 0)
        .unwrap_or(6868)
}

fn resolve_user_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn build_http_router(runtime: Arc<DaemonRuntime>) -> Router {
    Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/ws", get(ws_handler))
        .route("/mcp/agents", post(crate::mcp::mcp_agents_handler))
        .fallback(service_reverse_proxy_fallback)
        .with_state(runtime)
}

async fn service_reverse_proxy_fallback(
    State(runtime): State<Arc<DaemonRuntime>>,
    req: Request<Body>,
) -> Response {
    if let Some(host_val) = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        && let Some(target_port) = runtime
            .orchestrator
            .scripts
            .resolve_proxy_port(host_val)
            .await
    {
        return runtime
            .orchestrator
            .scripts
            .proxy_http_request(target_port, req)
            .await;
    }
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

pub async fn run_daemon(config: DaemonConfig) -> anyhow::Result<()> {
    let mut persisted = DaemonPersistedConfig::load(&config.home_dir);
    let effective_relay = config.relay_url.or_else(|| persisted.relay_url.clone());
    let cleaned_sockets =
        orbit_shim::cleanup_stale_shim_sockets(&config.home_dir.join("shims")).await;
    if cleaned_sockets > 0 {
        info!(
            removed = cleaned_sockets,
            "Cleaned up stale shim sockets from previous runs"
        );
    }
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
    runtime.activate_server(bound_addr.port()).await;

    let direct_eps = if bound_addr.ip().is_unspecified() {
        discover_lan_ws_endpoints(bound_addr.port())
    } else {
        vec![format!("ws://{bound_addr}/ws")]
    };
    let offer = runtime.build_pairing_offer(direct_eps, effective_relay.clone());
    let redacted_relay = effective_relay.as_deref().map(redact_relay_url);

    info!(
        host_id = %runtime.host_info.host_id,
        hostname = %runtime.host_info.hostname,
        providers = ?runtime.host_info.available_providers,
        listen = %bound_addr,
        mcp_url = %format!("http://127.0.0.1:{}/mcp/agents", bound_addr.port()),
        relay = ?redacted_relay,
        "Orbit Daemon ready"
    );
    if effective_relay.as_deref().is_some_and(is_ssh_relay_url) {
        info!("Run 'orbit pair' to print Multi-Host pairing URI");
    } else if let Ok(uri) = offer.to_qr_uri() {
        info!(pairing_uri = %uri, "Scan or import this Multi-Host pairing URI on Orbit Mobile");
    }

    if let Some(relay_url) = effective_relay {
        let rt_relay = runtime.clone();
        tokio::spawn(async move {
            outbound_relay_loop(rt_relay, relay_url).await;
        });
    }

    let app = build_http_router(runtime);
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
    runtime.activate_server(addr.port()).await;

    if let Some(r_url) = relay_url {
        let rt_relay = runtime.clone();
        tokio::spawn(async move {
            outbound_relay_loop(rt_relay, r_url).await;
        });
    }

    let app = build_http_router(runtime.clone());
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
    let display_relay = redact_relay_url(&relay_base);
    let pubkey = runtime.keypair.public_key_b64.clone();
    let ctrl_path = format!("/v1/daemon/control?pubkey={pubkey}");

    loop {
        match RelayConnector::connect(&relay_base).await {
            Ok(connector) => {
                let connector = Arc::new(connector);
                match connector.open_ws(&ctrl_path).await {
                    Ok(ws) => {
                        info!(relay = %display_relay, "Connected outbound to Orbit Relay");
                        let (mut sink, mut stream) = ws.split();
                        while let Some(Ok(msg)) = stream.next().await {
                            match msg {
                                TungMsg::Text(txt) => {
                                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt)
                                        && v.get("op").and_then(|x| x.as_str())
                                            == Some("open_tunnel")
                                        && let Some(tid) =
                                            v.get("tunnel_id").and_then(|x| x.as_str())
                                    {
                                        let rt_tun = runtime.clone();
                                        let conn_tun = Arc::clone(&connector);
                                        let tid_owned = tid.to_string();
                                        tokio::spawn(async move {
                                            let _ = serve_e2ee_relay_tunnel(
                                                rt_tun, &conn_tun, &tid_owned,
                                            )
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
                        warn!(relay = %display_relay, "Disconnected from Relay, reconnecting in 2s...");
                    }
                    Err(e) => {
                        warn!(relay = %display_relay, error = %e, "Relay control stream failed, retrying in 2s...");
                    }
                }
            }
            Err(e) => {
                warn!(relay = %display_relay, error = %e, "Relay connection failed, retrying in 2s...");
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn serve_e2ee_relay_tunnel(
    runtime: Arc<DaemonRuntime>,
    connector: &Arc<RelayConnector>,
    tunnel_id: &str,
) -> anyhow::Result<()> {
    let pubkey = &runtime.keypair.public_key_b64;
    let priv_bytes = runtime.keypair.private_bytes()?;
    let tun_path = format!("/v1/daemon/tunnel?tunnel_id={tunnel_id}&pubkey={pubkey}");

    let ws = connector.open_ws(&tun_path).await?;
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
                    E2EE_FRAME_WIRE => match WireMessage::decode_zstd(&plain[1..]) {
                        Ok(wire) => {
                            if in_tx
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
                        Err(err) => {
                            tracing::warn!(error = %err, bytes = plain.len(), "failed to decode E2EE zstd wire message");
                        }
                    },
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
                        match WireMessage::decode_zstd(&bin[1..]) {
                            Ok(wire) => {
                                if in_tx
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
                            Err(err) => {
                                tracing::warn!(error = %err, bytes = bin.len(), "failed to decode direct zstd wire message");
                            }
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
    let session_id = Uuid::new_v4();
    let mut use_zstd = false;
    let mut subscribed_agents: HashSet<String> = HashSet::new();
    let mut subscribed_provider_subagents: HashSet<(String, String)> = HashSet::new();
    let mut slot_to_terminal: HashMap<u8, (String, tokio::task::AbortHandle)> = HashMap::new();
    let mut active_side_questions: Vec<tokio::task::AbortHandle> = Vec::new();
    let mut bcast_rx = runtime.orchestrator.broadcast_tx.subscribe();
    let mut host_status_rx = runtime.connected_hosts.status_updates_tx.subscribe();

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
                        active_side_questions.retain(|h| !h.is_finished());
                        if let Some(reply) = handle_wire_rpc(
                            &runtime,
                            session_id,
                            msg,
                            &mut use_zstd,
                            &mut subscribed_agents,
                            &mut subscribed_provider_subagents,
                            &mut slot_to_terminal,
                            &mut active_side_questions,
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

            bcast_res = bcast_rx.recv() => {
                let bcast = match bcast_res {
                    Ok(b) => b,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            session_id = %session_id,
                            skipped,
                            "Daemon broadcast lagged for client session; closing to force cursor resync"
                        );
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                let maybe_msg = match bcast {
                    DaemonBroadcast::AgentUpdated(agent) => {
                        Some(WireMessage::AgentUpdated { agent })
                    }
                    DaemonBroadcast::AgentDeleted(agent_id) => {
                        subscribed_agents.remove(&agent_id);
                        Some(WireMessage::AgentDeleted { agent_id })
                    }
                    DaemonBroadcast::TimelineLive(event) => {
                        if subscribed_agents.contains(&event.agent_id) {
                            Some(WireMessage::TimelineLive { event })
                        } else {
                            None
                        }
                    }
                    DaemonBroadcast::WorkspaceUpdated(workspace) => {
                        Some(WireMessage::WorkspaceUpdated { workspace })
                    }
                    DaemonBroadcast::WorkspaceSetupProgress(progress) => {
                        Some(WireMessage::WorkspaceSetupProgress { progress })
                    }
                    DaemonBroadcast::WorkspaceScriptUpdated(script) => {
                        Some(WireMessage::WorkspaceScriptUpdated { script })
                    }
                    DaemonBroadcast::ScheduleUpdated(schedule) => {
                        Some(WireMessage::ScheduleUpdated { schedule })
                    }
                    DaemonBroadcast::ScheduleDeleted(schedule_id) => {
                        Some(WireMessage::ScheduleDeleted { schedule_id })
                    }
                    DaemonBroadcast::ConnectedHostRemoved(registration_id) => {
                        Some(WireMessage::ConnectedHostRemoved { registration_id })
                    }
                    DaemonBroadcast::ProviderSubagentUpdated(subagent) => {
                        Some(WireMessage::ProviderSubagentUpdated { subagent })
                    }
                    DaemonBroadcast::ProviderSubagentTimelineLive(event) => {
                        if subscribed_provider_subagents.contains(&(
                            event.parent_agent_id.clone(),
                            event.subagent_id.clone(),
                        )) {
                            Some(WireMessage::ProviderSubagentTimelineLive { event })
                        } else {
                            None
                        }
                    }
                    DaemonBroadcast::AgentRewound {
                        origin_session_id,
                        outcome,
                    } => {
                        if origin_session_id != session_id
                            && subscribed_agents.contains(&outcome.agent.agent_id)
                        {
                            Some(WireMessage::AgentRewound {
                                request_id: None,
                                agent: outcome.agent,
                                to_before_seq: outcome.to_before_seq,
                                events: outcome.events,
                                restored_files: outcome.restored_files,
                                restored_prompt: outcome.restored_prompt,
                                notice: outcome.notice,
                            })
                        } else {
                            None
                        }
                    }
                    DaemonBroadcast::HostSyncStateUpdated(state) => {
                        Some(WireMessage::HostSyncStateUpdated { state })
                    }
                    DaemonBroadcast::WorkspaceBoardUpdated(board) => {
                        Some(WireMessage::WorkspaceBoardUpdated { board })
                    }
                    DaemonBroadcast::WorkspaceEventRecorded(event) => {
                        Some(WireMessage::WorkspaceEventRecorded { event })
                    }
                    DaemonBroadcast::WorkspaceOrganizeProgress(event) => {
                        Some(WireMessage::WorkspaceOrganizeProgress { event })
                    }
                };
                if let Some(msg) = maybe_msg
                    && out_tx
                        .send(SessionFrame::Wire { msg, zstd: use_zstd })
                        .await
                        .is_err()
                {
                    break;
                }
            }

            Ok(host) = host_status_rx.recv() => {
                if out_tx
                    .send(SessionFrame::Wire {
                        msg: WireMessage::ConnectedHostUpdated { host },
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

    for (_, (_, abort_handle)) in slot_to_terminal.drain() {
        abort_handle.abort();
    }
    for abort_handle in active_side_questions.drain(..) {
        abort_handle.abort();
    }
}

#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
async fn handle_wire_rpc(
    runtime: &Arc<DaemonRuntime>,
    session_id: Uuid,
    req: WireMessage,
    use_zstd: &mut bool,
    subscribed_agents: &mut HashSet<String>,
    subscribed_provider_subagents: &mut HashSet<(String, String)>,
    slot_to_terminal: &mut HashMap<u8, (String, tokio::task::AbortHandle)>,
    active_side_questions: &mut Vec<tokio::task::AbortHandle>,
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
        WireMessage::ListWorkspaces {
            request_id,
            include_archived,
            include_background,
        } => match runtime
            .store
            .list_workspaces_filtered(include_archived, include_background)
        {
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
            isolation,
            worktree_mode,
            pr_number,
            parent_workspace_id,
            background,
            idempotency_key,
            is_curated,
            mission_goal,
            project_root,
        } => {
            let mut reserved_key: Option<String> = None;
            if let Some(ref idem) = idempotency_key
                && !idem.trim().is_empty()
            {
                let hash = hash_payload(&serde_json::json!({
                    "source_cwd": source_cwd,
                    "title": title,
                    "worktree_branch": worktree_branch,
                    "base_ref": base_ref,
                    "isolation": isolation,
                    "worktree_mode": worktree_mode,
                    "pr_number": pr_number,
                    "background": background,
                    "is_curated": is_curated,
                }));
                match runtime
                    .store
                    .check_or_reserve_receipt("ws_create", idem, &hash)
                {
                    Ok(ReceiptOutcome::Committed(json)) => {
                        if let Ok(ws) = serde_json::from_str(&json) {
                            return Some(WireMessage::WorkspaceCreated {
                                request_id,
                                workspace: ws,
                            });
                        }
                    }
                    Ok(ReceiptOutcome::Conflict) => {
                        return Some(rpc_err(
                            request_id,
                            "idempotency_conflict",
                            "idempotency_key was reused with different parameters",
                        ));
                    }
                    Ok(ReceiptOutcome::PendingInFlight) => {
                        return Some(rpc_err(
                            request_id,
                            "idempotency_pending",
                            "a request with this idempotency_key is already in flight",
                        ));
                    }
                    Ok(ReceiptOutcome::Reserved) => {
                        reserved_key = Some(idem.clone());
                    }
                    Err(e) => return Some(rpc_err(request_id, "store_error", e)),
                }
            }

            let effective_curated = is_curated.unwrap_or(false);
            match runtime
                .orchestrator
                .create_workspace_v2(CreateWorkspaceSpec {
                    source_cwd,
                    title,
                    worktree_branch,
                    base_ref,
                    isolation,
                    worktree_mode,
                    pr_number,
                    parent_workspace_id,
                    background,
                    project_root,
                    mission_goal,
                    allow_reuse_existing: !effective_curated,
                    is_curated: effective_curated,
                })
                .await
            {
                Ok(workspace) => {
                    if let Some(ref k) = reserved_key
                        && let Ok(raw) = serde_json::to_string(&workspace)
                    {
                        let _ = runtime.store.commit_receipt("ws_create", k, &raw);
                    }
                    Some(WireMessage::WorkspaceCreated {
                        request_id,
                        workspace,
                    })
                }
                Err(e) => {
                    if let Some(ref k) = reserved_key {
                        let _ = runtime.store.delete_pending_receipt("ws_create", k);
                    }
                    let err_code = if e.to_string().contains("not_a_git_repo") {
                        "not_a_git_repo"
                    } else {
                        "workspace_error"
                    };
                    Some(rpc_err(request_id, err_code, e))
                }
            }
        }
        WireMessage::RenameWorkspace {
            request_id,
            workspace_id,
            title,
        } => match runtime
            .orchestrator
            .rename_workspace(&workspace_id, &title)
            .await
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "rename_workspace_failed", e)),
        },
        WireMessage::ArchiveWorkspace {
            request_id,
            workspace_id,
            keep_agents_unclassified,
        } => match runtime
            .orchestrator
            .archive_workspace_with_options(&workspace_id, keep_agents_unclassified)
            .await
        {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "archive_workspace_failed", e)),
        },
        WireMessage::UnarchiveWorkspace {
            request_id,
            workspace_id,
        } => match runtime
            .orchestrator
            .unarchive_workspace(&workspace_id)
            .await
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "unarchive_workspace_failed", e)),
        },
        WireMessage::ListWorkspaceScripts {
            request_id,
            workspace_id,
        } => match runtime
            .orchestrator
            .list_workspace_scripts(&workspace_id)
            .await
        {
            Ok(scripts) => Some(WireMessage::WorkspaceScriptsSnapshot {
                request_id,
                workspace_id,
                scripts,
            }),
            Err(e) => Some(rpc_err(request_id, "list_scripts_failed", e)),
        },
        WireMessage::StartWorkspaceScript {
            request_id,
            workspace_id,
            script_name,
        } => match runtime
            .orchestrator
            .start_workspace_script(&workspace_id, &script_name)
            .await
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "start_script_failed", e)),
        },
        WireMessage::StopWorkspaceScript {
            request_id,
            workspace_id,
            script_name,
        } => match runtime
            .orchestrator
            .stop_workspace_script(&workspace_id, &script_name)
            .await
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "stop_script_failed", e)),
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
        WireMessage::InspectGitRepos {
            request_id,
            cwd,
            extra_dirs,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let extra_paths: Vec<PathBuf> =
                    extra_dirs.iter().map(|d| expand_host_path(d)).collect();
                let (is_git_repo, git_branch, repos) = tokio::task::spawn_blocking(move || {
                    crate::discovery::inspect_project_git_repos(&resolved, &extra_paths)
                })
                .await
                .unwrap_or((false, None, Vec::new()));
                let _ = tx
                    .send(SessionFrame::Wire {
                        msg: WireMessage::GitReposResponse {
                            request_id,
                            cwd,
                            is_git_repo,
                            git_branch,
                            repos,
                        },
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
        WireMessage::ReadWorkspaceFile {
            request_id,
            cwd,
            path,
            max_bytes,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match read_workspace_file(&resolved_cwd, &path, max_bytes).await {
                    Ok(file_data) => WireMessage::WorkspaceFileContentResponse {
                        request_id,
                        cwd,
                        path,
                        resolved_path: file_data.resolved_path,
                        content_kind: file_data.content_kind,
                        mime_type: file_data.mime_type,
                        content: file_data.content,
                        size_bytes: file_data.size_bytes,
                        truncated: file_data.truncated,
                    },
                    Err(e) => rpc_err(request_id, "read_file_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // Agents
        WireMessage::ListAgents {
            request_id,
            workspace_id,
            include_archived,
            include_background,
            parent_agent_id,
        } => match runtime.store.list_agents_filtered(
            workspace_id.as_deref(),
            include_archived,
            include_background,
            parent_agent_id.as_deref(),
        ) {
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
            workspace_id,
            parent_agent_id,
            parent_host_id,
            profile_id,
            background,
            auto_archive,
            notify_on_finish,
            idempotency_key,
            base_ref,
            worktree_mode,
            branch,
            handle,
            auto_cluster,
        } => {
            let mut reserved_key: Option<String> = None;
            if let Some(ref idem) = idempotency_key
                && !idem.trim().is_empty()
            {
                let hash = hash_payload(&serde_json::json!({
                    "cwd": cwd,
                    "title": title,
                    "provider": provider,
                    "model": model,
                    "mode": mode,
                    "initial_prompt": initial_prompt,
                    "worktree": worktree,
                    "workspace_id": workspace_id,
                    "profile_id": profile_id,
                }));
                match runtime
                    .store
                    .check_or_reserve_receipt("agent_create", idem, &hash)
                {
                    Ok(ReceiptOutcome::Committed(json)) => {
                        if let Ok(agent) =
                            serde_json::from_str::<orbit_protocol::AgentSummary>(&json)
                        {
                            subscribed_agents.insert(agent.agent_id.clone());
                            return Some(WireMessage::AgentCreated { request_id, agent });
                        }
                    }
                    Ok(ReceiptOutcome::Conflict) => {
                        return Some(rpc_err(
                            request_id,
                            "idempotency_conflict",
                            "idempotency_key was reused with different parameters",
                        ));
                    }
                    Ok(ReceiptOutcome::PendingInFlight) => {
                        return Some(rpc_err(
                            request_id,
                            "idempotency_pending",
                            "a request with this idempotency_key is already in flight",
                        ));
                    }
                    Ok(ReceiptOutcome::Reserved) => {
                        reserved_key = Some(idem.clone());
                    }
                    Err(e) => return Some(rpc_err(request_id, "store_error", e)),
                }
            }

            let parent_id_for_watch = parent_agent_id.clone();
            let parent_host_for_watch = parent_host_id.clone();
            match runtime
                .orchestrator
                .create_and_start_agent(CreateAgentSpec {
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
                    workspace_id,
                    parent_agent_id,
                    parent_host_id,
                    profile_id,
                    background,
                    auto_archive,
                    base_ref,
                    worktree_mode,
                    branch,
                    pr_number: None,
                    handle,
                    auto_cluster,
                })
                .await
            {
                Ok(agent) => {
                    subscribed_agents.insert(agent.agent_id.clone());
                    // Register durable local finish subscription if caller is a local parent agent
                    if notify_on_finish == Some(true)
                        && let Some(caller_id) = parent_id_for_watch
                    {
                        let is_local_parent = parent_host_for_watch
                            .as_deref()
                            .is_none_or(|h| h == runtime.host_info.host_id);
                        if is_local_parent
                            && runtime.store.get_agent(&caller_id).ok().flatten().is_some()
                        {
                            let _ = runtime
                                .finish_subs
                                .register(caller_id, agent.agent_id.clone(), None, true)
                                .await;
                        }
                    }
                    if let Some(ref k) = reserved_key
                        && let Ok(raw) = serde_json::to_string(&agent)
                    {
                        let _ = runtime.store.commit_receipt("agent_create", k, &raw);
                    }
                    Some(WireMessage::AgentCreated { request_id, agent })
                }
                Err(e) => {
                    if let Some(ref k) = reserved_key {
                        let _ = runtime.store.delete_pending_receipt("agent_create", k);
                    }
                    let err_code = if e.to_string().contains("not_a_git_repo") {
                        "not_a_git_repo"
                    } else {
                        "create_agent_failed"
                    };
                    Some(rpc_err(request_id, err_code, e))
                }
            }
        }
        WireMessage::SendPrompt {
            request_id,
            agent_id,
            prompt,
            display_prompt,
            badges,
            active_turn_behavior,
            message_id,
            caller_agent_id,
            notify_on_finish,
        } => {
            subscribed_agents.insert(agent_id.clone());
            if let Some(caller_id) = caller_agent_id {
                let caller_title = runtime
                    .store
                    .get_agent(&caller_id)
                    .ok()
                    .flatten()
                    .map(|a| a.title);
                if notify_on_finish == Some(true)
                    && runtime.store.get_agent(&caller_id).ok().flatten().is_some()
                {
                    let _ = runtime
                        .finish_subs
                        .register(caller_id.clone(), agent_id.clone(), None, false)
                        .await;
                }
                let qmsg = QueuedAgentMessage {
                    message_id: message_id
                        .unwrap_or_else(|| format!("am_{}", uuid::Uuid::new_v4().simple())),
                    recipient_id: agent_id,
                    body: prompt,
                    active_turn_behavior,
                    session_mode: None,
                    unarchive: true,
                    source: AgentPromptSource {
                        kind: AgentMessageKind::DirectMessage,
                        source_agent_id: Some(caller_id),
                        source_host_id: Some(runtime.host_info.host_id.clone()),
                        source_title: caller_title,
                        event: None,
                        permission_context: None,
                    },
                };
                match runtime.delivery.enqueue(qmsg).await {
                    Ok(_) => Some(WireMessage::Ack { request_id }),
                    Err(e) => Some(rpc_err(request_id, "send_prompt_failed", e)),
                }
            } else {
                // Support `@all <message>` human broadcast to all sibling peers in the same Mission Workspace
                let broadcast_body = prompt
                    .trim_start()
                    .strip_prefix("@all ")
                    .or_else(|| prompt.trim_start().strip_prefix("@ALL "))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned);
                match runtime
                    .orchestrator
                    .send_prompt_with_options(
                        &agent_id,
                        prompt,
                        display_prompt,
                        badges,
                        active_turn_behavior,
                        message_id,
                    )
                    .await
                {
                    Ok(()) => {
                        if let Some(body) = broadcast_body
                            && let Ok(Some(cur_ag)) = runtime.store.get_agent(&agent_id)
                            && let Ok(peers) = runtime.store.list_agents_filtered(
                                Some(&cur_ag.workspace_id),
                                false,
                                false,
                                None,
                            )
                        {
                            for peer in peers {
                                if peer.agent_id == agent_id {
                                    continue;
                                }
                                let _ = runtime
                                    .delivery
                                    .enqueue(QueuedAgentMessage {
                                        recipient_id: peer.agent_id,
                                        message_id: format!("bc_{}", uuid::Uuid::new_v4().simple()),
                                        source: AgentPromptSource {
                                            kind: AgentMessageKind::DirectMessage,
                                            source_agent_id: Some(agent_id.clone()),
                                            source_host_id: Some(runtime.host_info.host_id.clone()),
                                            source_title: Some("Workspace @all".into()),
                                            event: Some("workspace_broadcast".into()),
                                            permission_context: None,
                                        },
                                        body: body.clone(),
                                        active_turn_behavior,
                                        session_mode: None,
                                        unarchive: true,
                                    })
                                    .await;
                            }
                        }
                        Some(WireMessage::Ack { request_id })
                    }
                    Err(e) => Some(rpc_err(request_id, "send_prompt_failed", e)),
                }
            }
        }
        WireMessage::EnqueuePrompt {
            request_id,
            agent_id,
            prompt,
            display_prompt,
            badges,
            queue_id,
        } => {
            subscribed_agents.insert(agent_id.clone());
            match runtime
                .orchestrator
                .enqueue_user_prompt(&agent_id, prompt, display_prompt, badges, queue_id)
                .await
            {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "enqueue_prompt_failed", e)),
            }
        }
        WireMessage::RemoveQueuedPrompt {
            request_id,
            agent_id,
            queue_id,
        } => {
            match runtime
                .orchestrator
                .remove_queued_user_prompt(&agent_id, &queue_id)
                .await
            {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "remove_queued_prompt_failed", e)),
            }
        }
        WireMessage::FlushQueuedPromptNow {
            request_id,
            agent_id,
            queue_id,
        } => {
            subscribed_agents.insert(agent_id.clone());
            match runtime
                .orchestrator
                .flush_queued_user_prompt_now(&agent_id, &queue_id)
                .await
            {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "flush_queued_prompt_failed", e)),
            }
        }
        WireMessage::ReorderQueuedPrompts {
            request_id,
            agent_id,
            queue_ids,
        } => {
            match runtime
                .orchestrator
                .reorder_queued_user_prompts(&agent_id, &queue_ids)
                .await
            {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "reorder_queued_prompts_failed", e)),
            }
        }
        WireMessage::UpgradeDaemon {
            request_id,
            target_version,
        } => {
            let rt = Arc::clone(runtime);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match rt.self_upgrade_and_restart(target_version).await {
                    Ok(()) => WireMessage::Ack { request_id },
                    Err(e) => rpc_err(request_id, "upgrade_daemon_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
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
            runtime.finish_subs.cancel_for_caller(&agent_id).await;
            match runtime.orchestrator.archive_agent(&agent_id).await {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "archive_failed", e)),
            }
        }
        WireMessage::UnarchiveAgent {
            request_id,
            agent_id,
        } => match runtime.orchestrator.unarchive_agent(&agent_id).await {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "unarchive_failed", e)),
        },
        WireMessage::DeleteAgent {
            request_id,
            agent_id,
        } => {
            subscribed_agents.remove(&agent_id);
            runtime.finish_subs.cancel_for_caller(&agent_id).await;
            match runtime.orchestrator.delete_agent(&agent_id).await {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "delete_agent_failed", e)),
            }
        }
        WireMessage::DetachAgent {
            request_id,
            agent_id,
        } => match runtime.orchestrator.detach_agent(&agent_id).await {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "detach_agent_failed", e)),
        },
        WireMessage::MarkAgentRead {
            request_id,
            agent_id,
            seen_seq,
        } => match runtime
            .orchestrator
            .mark_agent_read(&agent_id, seen_seq)
            .await
        {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "mark_read_failed", e)),
        },
        WireMessage::MarkAgentUnread {
            request_id,
            agent_id,
        } => match runtime.orchestrator.mark_agent_unread(&agent_id).await {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "mark_unread_failed", e)),
        },
        WireMessage::SetAgentPinned {
            request_id,
            agent_id,
            pinned,
            pinned_order,
        } => match runtime
            .orchestrator
            .set_agent_pinned(&agent_id, pinned, pinned_order)
            .await
        {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "set_pinned_failed", e)),
        },
        WireMessage::UpdateAgent {
            request_id,
            agent_id,
            title,
            model,
            effort,
            mode,
            labels,
            handle,
            task_summary,
        } => match runtime
            .orchestrator
            .update_agent(
                &agent_id,
                title,
                model,
                effort,
                mode,
                labels,
                handle,
                task_summary,
            )
            .await
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "update_agent_failed", e)),
        },
        WireMessage::GetHostSyncState { request_id } => {
            match runtime.orchestrator.get_host_sync_state() {
                Ok(state) => Some(WireMessage::HostSyncStateSnapshot { request_id, state }),
                Err(e) => Some(rpc_err(request_id, "host_sync_error", e)),
            }
        }
        WireMessage::UpsertHostProject {
            request_id,
            project,
        } => match runtime.orchestrator.upsert_host_project(project) {
            Ok(state) => Some(WireMessage::HostSyncStateSnapshot { request_id, state }),
            Err(e) => Some(rpc_err(request_id, "upsert_host_project_failed", e)),
        },
        WireMessage::DeleteHostProject {
            request_id,
            project_id,
        } => match runtime.orchestrator.delete_host_project(&project_id) {
            Ok(state) => Some(WireMessage::HostSyncStateSnapshot { request_id, state }),
            Err(e) => Some(rpc_err(request_id, "delete_host_project_failed", e)),
        },
        WireMessage::SetHostProjectOrder {
            request_id,
            project_order,
            project_sort_mode,
        } => match runtime
            .orchestrator
            .set_host_project_order(project_order, project_sort_mode)
        {
            Ok(state) => Some(WireMessage::HostSyncStateSnapshot { request_id, state }),
            Err(e) => Some(rpc_err(request_id, "set_project_order_failed", e)),
        },
        WireMessage::GetWorkspaceBoard {
            request_id,
            workspace_id,
        } => match runtime.orchestrator.get_workspace_board(&workspace_id) {
            Ok(board) => Some(WireMessage::WorkspaceBoardSnapshot { request_id, board }),
            Err(e) => Some(rpc_err(request_id, "get_board_failed", e)),
        },
        WireMessage::UpdateWorkspaceBoard {
            request_id,
            workspace_id,
            content,
            section,
            updated_by,
        } => match runtime.orchestrator.update_workspace_board(
            &workspace_id,
            &content,
            section.as_deref(),
            updated_by.as_deref().or(Some("user")),
            None,
        ) {
            Ok(board) => Some(WireMessage::WorkspaceBoardSnapshot { request_id, board }),
            Err(e) => Some(rpc_err(request_id, "update_board_failed", e)),
        },
        WireMessage::ListWorkspaceEvents {
            request_id,
            workspace_id,
            limit,
        } => match runtime
            .orchestrator
            .list_workspace_events(&workspace_id, limit.unwrap_or(50) as usize)
        {
            Ok(events) => Some(WireMessage::WorkspaceEventsSnapshot {
                request_id,
                workspace_id,
                events,
            }),
            Err(e) => Some(rpc_err(request_id, "list_ws_events_failed", e)),
        },
        WireMessage::UpdateWorkspaceGoal {
            request_id,
            workspace_id,
            title,
            mission_goal,
        } => match runtime
            .orchestrator
            .update_workspace_goal(&workspace_id, title, mission_goal)
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "update_ws_goal_failed", e)),
        },
        WireMessage::MoveAgentToWorkspace {
            request_id,
            agent_id,
            target_workspace_id,
            new_workspace_title,
            make_unclassified,
        } => match runtime
            .orchestrator
            .move_agent_to_workspace_v2(
                &agent_id,
                target_workspace_id,
                new_workspace_title,
                make_unclassified,
            )
            .await
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "move_agent_failed", e)),
        },
        WireMessage::MergeWorkspaces {
            request_id,
            source_workspace_id,
            target_workspace_id,
        } => match runtime
            .orchestrator
            .merge_workspaces(&source_workspace_id, &target_workspace_id)
            .await
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "merge_workspaces_failed", e)),
        },
        WireMessage::DismissClusterSuggestion {
            request_id,
            agent_id,
        } => match runtime.orchestrator.dismiss_cluster_suggestion(&agent_id) {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "dismiss_cluster_failed", e)),
        },
        WireMessage::OrganizeProjectWorkspaces {
            request_id,
            project_root,
            agent_ids,
            auto_apply,
        } => {
            let orch = Arc::clone(&runtime.orchestrator);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match orch
                    .organize_project_workspaces(
                        Some(request_id),
                        &project_root,
                        agent_ids,
                        auto_apply,
                    )
                    .await
                {
                    Ok(_) => WireMessage::Ack { request_id },
                    Err(e) => rpc_err(request_id, "organize_workspaces_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::SummarizeAgent {
            request_id,
            agent_id,
        } => {
            let orch = Arc::clone(&runtime.orchestrator);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match orch.summarize_agent(&agent_id).await {
                    Ok(_) => WireMessage::Ack { request_id },
                    Err(e) => rpc_err(request_id, "summarize_agent_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ListPendingPermissions {
            request_id,
            agent_id,
        } => {
            let permissions = runtime
                .orchestrator
                .list_pending_permissions(agent_id.as_deref())
                .await;
            Some(WireMessage::PendingPermissionsSnapshot {
                request_id,
                permissions,
            })
        }
        WireMessage::RewindAgent {
            request_id,
            agent_id,
            to_before_seq,
            restore_files,
        } => match runtime
            .orchestrator
            .rewind_agent(&agent_id, to_before_seq, restore_files, session_id)
            .await
        {
            Ok(outcome) => Some(WireMessage::AgentRewound {
                request_id: Some(request_id),
                agent: outcome.agent,
                to_before_seq: outcome.to_before_seq,
                events: outcome.events,
                restored_files: outcome.restored_files,
                restored_prompt: outcome.restored_prompt,
                notice: outcome.notice,
            }),
            Err(e) => Some(rpc_err(request_id, "rewind_failed", e)),
        },
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
            match runtime.store.fetch_timeline_since_budgeted(
                &agent_id,
                since_seq,
                2000,
                orbit_protocol::MAX_WIRE_TIMELINE_PAGE_BYTES,
            ) {
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
        WireMessage::CaptureTerminal {
            request_id,
            terminal_id,
            lines,
        } => match runtime
            .orchestrator
            .capture_terminal_text(&terminal_id, lines)
            .await
        {
            Ok(text) => Some(WireMessage::TerminalCaptured {
                request_id,
                terminal_id,
                text,
            }),
            Err(e) => Some(rpc_err(request_id, "capture_terminal_failed", e)),
        },
        WireMessage::SendTerminalKeys {
            request_id,
            terminal_id,
            text,
            keys,
        } => match runtime
            .orchestrator
            .send_terminal_keys(&terminal_id, text.as_deref(), &keys)
            .await
        {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "send_terminal_keys_failed", e)),
        },

        // Agent Profiles
        WireMessage::ListProfiles { request_id } => match runtime.store.list_profiles() {
            Ok(profiles) => Some(WireMessage::ProfilesSnapshot {
                request_id,
                profiles,
            }),
            Err(e) => Some(rpc_err(request_id, "store_error", e)),
        },
        WireMessage::UpsertProfile {
            request_id,
            mut profile,
        } => {
            if profile.profile_id.trim().is_empty() {
                profile.profile_id =
                    format!("prof_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
            }
            profile.updated_at = chrono::Utc::now();
            match runtime.store.upsert_profile(&profile) {
                Ok(()) => Some(WireMessage::ProfileUpserted {
                    request_id,
                    profile,
                }),
                Err(e) => Some(rpc_err(request_id, "upsert_profile_failed", e)),
            }
        }
        WireMessage::DeleteProfile {
            request_id,
            profile_id,
        } => match runtime.store.delete_profile(&profile_id) {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "delete_profile_failed", e)),
        },

        // Schedules & Heartbeats
        WireMessage::ListSchedules {
            request_id,
            include_completed,
        } => match runtime.store.list_schedules(include_completed) {
            Ok(schedules) => Some(WireMessage::SchedulesSnapshot {
                request_id,
                schedules,
            }),
            Err(e) => Some(rpc_err(request_id, "store_error", e)),
        },
        WireMessage::CreateSchedule {
            request_id,
            name,
            cron_expr,
            timezone,
            prompt,
            target,
            max_runs,
            expires_at,
            created_by_agent,
        } => match runtime.scheduler.create_schedule(
            name,
            &cron_expr,
            timezone,
            prompt,
            target,
            max_runs,
            expires_at,
            created_by_agent,
        ) {
            Ok(schedule) => Some(WireMessage::ScheduleCreated {
                request_id,
                schedule,
            }),
            Err(e) => Some(rpc_err(request_id, "create_schedule_failed", e)),
        },
        WireMessage::UpdateSchedule {
            request_id,
            schedule_id,
            name,
            cron_expr,
            prompt,
            max_runs,
            expires_at,
            archive_on_finish,
        } => match runtime.scheduler.update_schedule(
            &schedule_id,
            name,
            cron_expr.as_deref(),
            prompt,
            max_runs,
            expires_at,
            archive_on_finish,
        ) {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "update_schedule_failed", e)),
        },
        WireMessage::PauseSchedule {
            request_id,
            schedule_id,
        } => match runtime
            .scheduler
            .set_schedule_status(&schedule_id, ScheduleStatus::Paused)
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "pause_schedule_failed", e)),
        },
        WireMessage::ResumeSchedule {
            request_id,
            schedule_id,
        } => match runtime
            .scheduler
            .set_schedule_status(&schedule_id, ScheduleStatus::Active)
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "resume_schedule_failed", e)),
        },
        WireMessage::RunScheduleOnce {
            request_id,
            schedule_id,
        } => {
            let scheduler = runtime.scheduler.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match scheduler.run_once(&schedule_id).await {
                    Ok(_) => WireMessage::Ack { request_id },
                    Err(e) => rpc_err(request_id, "run_schedule_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::DeleteSchedule {
            request_id,
            schedule_id,
        } => match runtime.scheduler.delete_schedule(&schedule_id) {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "delete_schedule_failed", e)),
        },
        WireMessage::ListScheduleRuns {
            request_id,
            schedule_id,
            limit,
        } => match runtime
            .store
            .list_schedule_runs(&schedule_id, limit.unwrap_or(50) as usize)
        {
            Ok(runs) => Some(WireMessage::ScheduleRunsSnapshot {
                request_id,
                schedule_id,
                runs,
            }),
            Err(e) => Some(rpc_err(request_id, "store_error", e)),
        },

        // Daemon-to-Daemon Connected Hosts
        WireMessage::ListConnectedHosts { request_id } => {
            let hosts = runtime.connected_hosts.list_summaries().await;
            Some(WireMessage::ConnectedHostsSnapshot { request_id, hosts })
        }
        WireMessage::AddConnectedHost {
            request_id,
            label,
            pairing_uri,
        } => match runtime
            .connected_hosts
            .add_from_pairing_uri(label, &pairing_uri)
            .await
        {
            Ok(host) => Some(WireMessage::ConnectedHostAdded { request_id, host }),
            Err(e) => Some(rpc_err(request_id, "add_connected_host_failed", e)),
        },
        WireMessage::UpdateConnectedHost {
            request_id,
            registration_id,
            label,
            enabled,
        } => match runtime
            .connected_hosts
            .update_registration(&registration_id, label, enabled)
            .await
        {
            Ok(_) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "update_connected_host_failed", e)),
        },
        WireMessage::RemoveConnectedHost {
            request_id,
            registration_id,
        } => match runtime
            .connected_hosts
            .remove_registration(&registration_id)
            .await
        {
            Ok(_) => {
                let _ = runtime
                    .orchestrator
                    .broadcast_tx
                    .send(DaemonBroadcast::ConnectedHostRemoved(registration_id));
                Some(WireMessage::Ack { request_id })
            }
            Err(e) => Some(rpc_err(request_id, "remove_connected_host_failed", e)),
        },

        // Host Orchestration Config & Bundled Skills
        WireMessage::GetOrchestrationConfig { request_id } => {
            let config = runtime.orch_config.read().await.clone();
            Some(WireMessage::OrchestrationConfigSnapshot { request_id, config })
        }
        WireMessage::UpdateOrchestrationConfig { request_id, config } => {
            *runtime.orch_config.write().await = config.clone();
            let mut persisted = DaemonPersistedConfig::load(&runtime.home_dir);
            persisted.orchestration = config.clone();
            match persisted.save(&runtime.home_dir) {
                Ok(()) => Some(WireMessage::OrchestrationConfigSnapshot { request_id, config }),
                Err(e) => Some(rpc_err(request_id, "save_config_failed", e)),
            }
        }
        WireMessage::ListOrchestrationSkills { request_id } => {
            let user_home = resolve_user_home();
            let skills = crate::skills::inspect_orchestration_skills(&user_home);
            Some(WireMessage::OrchestrationSkillsSnapshot { request_id, skills })
        }
        WireMessage::SyncOrchestrationSkills {
            request_id,
            skill_ids,
        } => {
            let user_home = resolve_user_home();
            match crate::skills::sync_orchestration_skills(&user_home, &skill_ids) {
                Ok(skills) => {
                    {
                        let mut cfg = runtime.orch_config.write().await;
                        cfg.installed_skills = skill_ids;
                        let mut persisted = DaemonPersistedConfig::load(&runtime.home_dir);
                        persisted.orchestration = cfg.clone();
                        let _ = persisted.save(&runtime.home_dir);
                    }
                    Some(WireMessage::OrchestrationSkillsSnapshot { request_id, skills })
                }
                Err(e) => Some(rpc_err(request_id, "sync_skills_failed", e)),
            }
        }

        // Provider-Native Subagent Observability
        WireMessage::ListProviderSubagents {
            request_id,
            parent_agent_id,
        } => match runtime.store.list_provider_subagents(&parent_agent_id) {
            Ok(subagents) => Some(WireMessage::ProviderSubagentsSnapshot {
                request_id,
                parent_agent_id,
                subagents,
            }),
            Err(e) => Some(rpc_err(request_id, "store_error", e)),
        },
        WireMessage::SubscribeProviderSubagent {
            request_id,
            parent_agent_id,
            subagent_id,
            since_seq,
        } => {
            subscribed_provider_subagents.insert((parent_agent_id.clone(), subagent_id.clone()));
            match runtime.store.fetch_provider_subagent_events_since(
                &parent_agent_id,
                &subagent_id,
                since_seq,
                1000,
            ) {
                Ok((events, has_more)) => Some(WireMessage::ProviderSubagentTimelineCatchup {
                    request_id,
                    parent_agent_id,
                    subagent_id,
                    events,
                    has_more,
                }),
                Err(e) => Some(rpc_err(request_id, "store_error", e)),
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
        WireMessage::StageAttachmentChunk {
            request_id,
            upload_id,
            filename,
            chunk_index,
            total_chunks,
            data_b64,
        } => {
            let rt = Arc::clone(runtime);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match rt
                    .attachments
                    .ingest_chunk(upload_id, filename, chunk_index, total_chunks, data_b64)
                    .await
                {
                    Ok(res) => WireMessage::AttachmentStaged {
                        request_id,
                        upload_id: res.upload_id,
                        chunk_index: res.chunk_index,
                        completed: res.completed,
                        path: res.path,
                        filename: res.filename,
                        size_bytes: res.size_bytes,
                    },
                    Err(e) => rpc_err(request_id, "stage_attachment_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::AskSideQuestion {
            request_id,
            agent_id: _,
            cwd,
            provider,
            model,
            question,
            context_summary,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let handle = tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let (delta_tx, mut delta_rx) = mpsc::unbounded_channel::<String>();
                let forward_tx = tx.clone();
                let forward_task = tokio::spawn(async move {
                    while let Some(chunk) = delta_rx.recv().await {
                        if forward_tx
                            .send(SessionFrame::Wire {
                                msg: WireMessage::SideQuestionDelta {
                                    request_id,
                                    text_delta: chunk,
                                },
                                zstd,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                });

                let result = run_side_question(
                    SideQuestionSpec {
                        provider,
                        cwd: resolved_cwd,
                        model,
                        question,
                        context_summary,
                    },
                    delta_tx,
                )
                .await;

                let _ = forward_task.await;

                let done_msg = match result {
                    Ok(full_text) => WireMessage::SideQuestionDone {
                        request_id,
                        full_text,
                        error: None,
                    },
                    Err(err) => WireMessage::SideQuestionDone {
                        request_id,
                        full_text: String::new(),
                        error: Some(err),
                    },
                };
                let _ = tx
                    .send(SessionFrame::Wire {
                        msg: done_msg,
                        zstd,
                    })
                    .await;
            });
            active_side_questions.push(handle.abort_handle());
            None
        }
        _ => None,
    }
}

fn hash_payload(val: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(val.to_string().as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
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
    let orchestrator = Arc::clone(&runtime.orchestrator);
    let tid_owned = terminal_id.to_string();
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
        loop {
            match rx.recv().await {
                Ok(PtyBroadcast::Snapshot(data)) => {
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
                Ok(PtyBroadcast::Output(data)) => {
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
                Ok(PtyBroadcast::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        terminal_id = %tid_owned,
                        skipped,
                        "PTY client broadcast lagged; resyncing with latest snapshot"
                    );
                    if let Some(fresh) = orchestrator.terminal_snapshot(&tid_owned).await
                        && !fresh.is_empty()
                        && tx_clone
                            .send(SessionFrame::Terminal(TerminalBinaryFrame::Snapshot {
                                slot,
                                data: fresh,
                            }))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
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

fn shell_single_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\"'\"'"))
}

fn daemon_release_asset_name() -> anyhow::Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("orbit-cli-macos-arm64"),
        ("linux", "x86_64") => Ok("orbit-cli-linux-x64"),
        ("linux", "aarch64") => Ok("orbit-cli-linux-arm64"),
        (os, arch) => anyhow::bail!("Unsupported platform for self-upgrade: {os}/{arch}"),
    }
}

impl DaemonRuntime {
    /// Download the matching Orbit release binary, verify `--version`, atomically install to
    /// `~/.local/bin/orbit`, and trigger a detached `orbit daemon up` hot-restart so all
    /// `orbit shim` agent & PTY sessions stay alive uninterrupted.
    pub async fn self_upgrade_and_restart(
        &self,
        target_version: Option<String>,
    ) -> anyhow::Result<()> {
        use tokio::io::AsyncWriteExt;

        let _upgrade_guard = self.upgrade_lock.lock().await;

        let asset = daemon_release_asset_name()?;
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("Unable to resolve HOME directory"))?;
        let install_dir = home.join(".local/bin");
        std::fs::create_dir_all(&install_dir)?;
        let install_path = install_dir.join("orbit");
        let tmp_path = install_dir.join(format!(
            "orbit.upgrade.{}.{}",
            std::process::id(),
            Uuid::new_v4().simple()
        ));

        let repo = std::env::var("ORBIT_REPO")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "danielgaoyy/orbit".to_string());

        let mut urls: Vec<String> = Vec::new();
        if let Some(raw_ver) = target_version
            .as_deref()
            .map(str::trim)
            .map(|s| s.trim_start_matches(['v', 'V']))
            .filter(|s| !s.is_empty())
        {
            if !is_valid_release_semver(raw_ver) {
                anyhow::bail!("Invalid target version format: {raw_ver}");
            }
            urls.push(format!(
                "https://github.com/{repo}/releases/download/v{raw_ver}/{asset}"
            ));
        }
        urls.push(format!(
            "https://github.com/{repo}/releases/latest/download/{asset}"
        ));

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .user_agent(format!("orbit-daemon/{}", env!("CARGO_PKG_VERSION")))
            .build()?;

        let mut downloaded = false;
        let mut last_err = anyhow::anyhow!("Failed to download Orbit binary");
        for url in &urls {
            match client.get(url).send().await {
                Ok(mut resp) if resp.status().is_success() => {
                    let mut file = tokio::fs::File::create(&tmp_path).await?;
                    let mut stream_err: Option<anyhow::Error> = None;
                    loop {
                        match resp.chunk().await {
                            Ok(Some(bytes)) => {
                                if let Err(e) = file.write_all(&bytes).await {
                                    stream_err = Some(e.into());
                                    break;
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                stream_err = Some(e.into());
                                break;
                            }
                        }
                    }
                    if let Some(err) = stream_err {
                        let _ = tokio::fs::remove_file(&tmp_path).await;
                        last_err = err;
                        continue;
                    }
                    if let Err(e) = file.flush().await {
                        let _ = tokio::fs::remove_file(&tmp_path).await;
                        return Err(e.into());
                    }
                    drop(file);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Err(e) = std::fs::set_permissions(
                            &tmp_path,
                            std::fs::Permissions::from_mode(0o755),
                        ) {
                            let _ = std::fs::remove_file(&tmp_path);
                            return Err(e.into());
                        }
                    }
                    downloaded = true;
                    break;
                }
                Ok(resp) => {
                    last_err = anyhow::anyhow!("HTTP {} downloading {url}", resp.status());
                }
                Err(e) => {
                    last_err = anyhow::anyhow!("Network error downloading {url}: {e}");
                }
            }
        }

        if !downloaded {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(last_err);
        }

        let check = tokio::process::Command::new(&tmp_path)
            .arg("--version")
            .output()
            .await;
        match check {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if !stdout.trim().to_ascii_lowercase().starts_with("orbit ") {
                    let _ = std::fs::remove_file(&tmp_path);
                    anyhow::bail!("Downloaded binary returned unexpected --version output");
                }
            }
            Ok(out) => {
                let _ = std::fs::remove_file(&tmp_path);
                anyhow::bail!("Downloaded binary failed --version check ({})", out.status);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                anyhow::bail!("Downloaded binary could not be executed: {e}");
            }
        }

        if let Err(e) = std::fs::rename(&tmp_path, &install_path) {
            let _ = std::fs::remove_file(&tmp_path);
            anyhow::bail!("Failed to replace {}: {e}", install_path.display());
        }

        let port = *self.listen_port.read().await;
        let default_home = home.join(".orbit");
        let is_default_home = std::fs::canonicalize(&self.home_dir)
            .unwrap_or_else(|_| self.home_dir.clone())
            == std::fs::canonicalize(&default_home).unwrap_or(default_home);

        let sq_bin = shell_single_quote(&install_path.to_string_lossy());
        let home_flag = if is_default_home {
            String::new()
        } else {
            format!(
                " --home {}",
                shell_single_quote(&self.home_dir.to_string_lossy())
            )
        };
        let port_flag = if port > 0 {
            format!(" --port {port}")
        } else {
            String::new()
        };
        let restart_script = format!("sleep 0.35 && {sq_bin}{home_flag} daemon up{port_flag}");

        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", &restart_script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn daemon hot-restart: {e}"))?;

        tracing::info!(
            install_path = %install_path.display(),
            port,
            "Scheduled self-upgrade hot-restart via `orbit daemon up`"
        );
        Ok(())
    }
}

/// Strictly validate a release semver tag (`X.Y.Z` with optional `-prerelease`) before
/// interpolating it into a GitHub Releases download URL, preventing path traversal (`../`).
fn is_valid_release_semver(ver: &str) -> bool {
    if ver.is_empty() || ver.len() > 64 {
        return false;
    }
    let (core, prerelease) = match ver.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (ver, None),
    };
    let mut parts = core.split('.');
    let valid_core = matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(a), Some(b), Some(c), None)
            if !a.is_empty()
                && !b.is_empty()
                && !c.is_empty()
                && a.bytes().all(|ch| ch.is_ascii_digit())
                && b.bytes().all(|ch| ch.is_ascii_digit())
                && c.bytes().all(|ch| ch.is_ascii_digit())
    );
    if !valid_core {
        return false;
    }
    if let Some(pre) = prerelease {
        if pre.is_empty() || pre.contains("..") {
            return false;
        }
        return pre
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'.' || ch == b'-');
    }
    true
}
