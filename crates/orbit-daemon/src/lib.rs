//! Host-local Orbit Daemon (`orbit daemon`).
//!
//! Runs on each target machine (Devbox, Linux workstation, Mac mini), manages SQLite WAL state,
//! supervises detached Shims, serves direct WebSocket clients, exposes the built-in Streamable
//! HTTP MCP orchestration server (`/mcp/agents`), runs the Cron/Heartbeat scheduler, maintains
//! Daemon-to-Daemon Connected Host links, and optionally registers outbound to Relay with full
//! `Noise_NK_25519_ChaChaPoly_SHA256` end-to-end encryption.

pub mod artifacts;
pub mod attachments;
pub mod browser;
pub mod connected_hosts;
pub mod cron;
pub mod discovery;
pub mod fast_copy;
pub mod finish_subscriptions;
pub mod git;
pub mod goals;
pub mod guardian;
pub mod hooks;
pub mod idle_cleanup;
pub mod im_channels;
pub mod instructions;
pub mod keep_awake;
pub mod lsp;
pub mod manager;
pub mod mcp;
pub mod mcp_servers;
pub mod messaging;
pub mod platform;
pub mod push;
pub mod rules;
pub mod scheduler;
pub mod skills;
pub mod skills_manager;
pub mod symbols;
pub mod timers;
pub mod tools;
pub mod triggers;
pub mod usage;
pub mod workflows;
pub mod workspace_board;
pub mod workspace_cluster;
pub mod workspace_files;
pub mod workspace_organizer;
pub mod workspace_runtime;
pub mod worktree_handoff;
pub mod worktree_pool;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Router,
    body::Body,
    extract::{
        DefaultBodyLimit, State,
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
use crate::browser::{BrowserManager, CreateBrowserSessionSpec};
use crate::connected_hosts::ConnectedHostsManager;
use crate::finish_subscriptions::FinishSubscriptionManager;
use crate::git::{
    compute_git_diff, inspect_git_status, list_workspace_files,
    read_workspace_file_with_attachments_dir,
};
use crate::goals::{GoalMonitorEngine, StartGoalSpec, StartMonitorSpec};
use crate::hooks::HooksEngine;
use crate::idle_cleanup::IdleRuntimeCleanup;
use crate::im_channels::{ImChannelBridge, UpsertImChannelSpec};
use crate::instructions::{
    discover_instruction_hierarchy, read_instruction_file, save_instruction_file,
};
use crate::keep_awake::KeepAwakeManager;
use crate::lsp::LspManager;
use crate::manager::{
    AgentOrchestrator, CreateAgentSpec, CreateWorkspaceSpec, DaemonBroadcast, PtyBroadcast,
};
use crate::messaging::{
    AgentMessageDelivery, AgentPromptSource, DEFAULT_COALESCE_WINDOW, QueuedAgentMessage,
};
use crate::push::PushManager;
use crate::scheduler::ScheduleEngine;
use crate::symbols::WorkspaceSymbolIndex;
use crate::timers::ConditionalTimerEngine;
use crate::triggers::{EventTriggerEngine, UpsertTriggerRequest};
use crate::workflows::{
    StartBatchFanoutSpec, StartBestOfNSpec, StartDagWorkflowSpec, WorkflowEngine,
};
use crate::workspace_files::{
    canonicalize_workspace_root, create_workspace_entry, delete_workspace_entry,
    rename_workspace_entry, write_workspace_file,
};
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
    pub conditional_timers: Arc<ConditionalTimerEngine>,
    pub idle_cleanup: Arc<IdleRuntimeCleanup>,
    pub keep_awake: Arc<KeepAwakeManager>,
    pub goals: Arc<GoalMonitorEngine>,
    pub triggers: Arc<EventTriggerEngine>,
    pub workflows: Arc<WorkflowEngine>,
    pub symbol_index: Arc<WorkspaceSymbolIndex>,
    pub lsp: Arc<LspManager>,
    pub browser: Arc<BrowserManager>,
    pub hooks: Arc<HooksEngine>,
    pub im_channels: Arc<ImChannelBridge>,
    pub push: Arc<PushManager>,
    provider_catalogs: Arc<RwLock<Option<Vec<ProviderModelCatalog>>>>,
    pub mcp_probe_cache: Arc<RwLock<HashMap<String, orbit_protocol::McpProbeResult>>>,
    pub mcp_config_lock: Arc<AsyncMutex<()>>,
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
        orbit_providers::set_snapshot_orbit_home(home_dir);

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
        let keep_awake_enabled = persisted.orchestration.keep_awake_while_running;
        let orch_config = Arc::new(RwLock::new(persisted.orchestration));

        let host_id = keypair.host_id()?;
        let store = Arc::new(OrbitStore::open(home_dir.join("orbit.db"))?);
        let scripts = WorkspaceScriptRegistry::new();
        let keep_awake = Arc::new(KeepAwakeManager::new(keep_awake_enabled));
        let provider_catalogs = Arc::new(RwLock::new(None));
        let hooks = HooksEngine::new(home_dir.to_path_buf());
        let orchestrator = AgentOrchestrator::new(
            home_dir.to_path_buf(),
            host_id.clone(),
            mcp_token.clone(),
            listen_port.clone(),
            orch_config.clone(),
            store.clone(),
            scripts,
            keep_awake.clone(),
            provider_catalogs.clone(),
            hooks.clone(),
        )
        .await;
        let ka_bcast_tx = orchestrator.broadcast_tx.clone();
        keep_awake.set_on_status_change(move |status| {
            let _ = ka_bcast_tx.send(DaemonBroadcast::KeepAwakeStatusUpdated(status));
        });
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
        let conditional_timers =
            ConditionalTimerEngine::new(store.clone(), orchestrator.clone(), delivery.clone());
        orchestrator
            .set_conditional_timers(conditional_timers.clone())
            .await;
        let timers_for_admission = conditional_timers.clone();
        delivery
            .set_on_admitted(Arc::new(move |recipient_id, kind, sender_id| {
                let timers = Arc::clone(&timers_for_admission);
                Box::pin(async move {
                    if matches!(
                        kind,
                        AgentMessageKind::DirectMessage | AgentMessageKind::FinishNotification
                    ) {
                        timers
                            .notify_agent_message(&recipient_id, sender_id.as_deref())
                            .await;
                    }
                })
            }))
            .await;
        let goals = GoalMonitorEngine::new(
            store.clone(),
            &orchestrator,
            delivery.clone(),
            keep_awake.clone(),
        );
        let triggers = EventTriggerEngine::new(
            store.clone(),
            orchestrator.clone(),
            delivery.clone(),
            listen_port.clone(),
            home_dir,
        );
        let workflows = WorkflowEngine::new(store.clone(), &orchestrator, keep_awake.clone());
        let idle_cleanup = IdleRuntimeCleanup::new(
            store.clone(),
            orchestrator.clone(),
            delivery.clone(),
            finish_subs.clone(),
            orch_config.clone(),
        );
        let lsp = LspManager::new(home_dir.to_path_buf());
        let browser = BrowserManager::new(home_dir.to_path_buf());
        let im_channels =
            ImChannelBridge::new(store.clone(), orchestrator.clone(), listen_port.clone());
        let host_display_name = hostname_string();
        let push = PushManager::new(
            store.clone(),
            orchestrator.clone(),
            host_id.clone(),
            host_display_name.clone(),
        );

        let home_for_probe = home_dir.to_path_buf();
        let available_providers = tokio::task::spawn_blocking(move || {
            let discovered = discover_installed_providers();
            let mut providers: Vec<orbit_protocol::ProviderKind> =
                discovered.into_iter().map(|d| d.kind).collect();
            if orbit_providers::has_any_installed_acp_agent(&home_for_probe) {
                providers.push(orbit_protocol::ProviderKind::Acp);
            }
            providers
        })
        .await
        .map_err(|e| anyhow::anyhow!("provider discovery task failed: {e}"))?;

        let host_info = HostInfo {
            host_id,
            hostname: host_display_name,
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
            conditional_timers,
            idle_cleanup,
            keep_awake,
            goals,
            triggers,
            workflows,
            symbol_index: Arc::new(WorkspaceSymbolIndex::with_external_provider(Arc::new(
                crate::lsp::LspExternalSymbolProvider::new(Arc::clone(&lsp)),
            ))),
            lsp,
            browser,
            hooks,
            im_channels,
            push,
            provider_catalogs,
            mcp_probe_cache: Arc::new(RwLock::new(HashMap::new())),
            mcp_config_lock: Arc::new(AsyncMutex::new(())),
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
        // 3. Resume any persisted finish subscriptions & armed conditional timers across restarts
        self.finish_subs.start_recovery().await;
        self.conditional_timers.start_recovery().await;
        // 3b. Recover active autonomous Goals, streaming Monitor watches & interrupted Workflows
        self.goals.start_recovery().await;
        self.workflows.start_recovery().await;
        // 3c. Start GitHub PR/Issue/CI polling fallback loop for enabled event triggers
        self.triggers.spawn_background_poller().await;
        // 3d. Start Bidirectional IM Channels bridge & NAT-friendly pollers
        self.im_channels.start().await;
        // 3e. Start Native Mobile Background Push dispatcher (APNs / FCM / HarmonyOS Push Kit)
        self.push.start().await;
        // 4. Start Cron/Heartbeat scheduler, Idle Runtime Cleanup, LSP & Browser Idle Reaper loops
        drop(self.scheduler.spawn_background());
        drop(self.idle_cleanup.spawn_background());
        drop(self.lsp.spawn_idle_reaper());
        drop(self.browser.spawn_idle_reaper());

        // 5. Warm login-shell environment snapshot & provider model catalogs in background (non-blocking)
        orbit_providers::set_snapshot_orbit_home(&self.home_dir);
        let rt_warm = Arc::clone(self);
        tokio::spawn(async move {
            let env_map = orbit_providers::resolve_login_shell_env().await;
            let stats = orbit_providers::shell_snapshot_stats();
            tracing::debug!(
                shell = %stats.shell,
                var_count = env_map.len(),
                hits = stats.hits,
                misses = stats.misses,
                disk_hits = stats.disk_hits,
                "Login shell environment snapshot warmed"
            );
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
    crate::platform::resolve_user_home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn build_http_router(runtime: Arc<DaemonRuntime>) -> Router {
    let im_webhook_routes = Router::new()
        .route(
            "/im/lark/{channel_id}",
            post(crate::im_channels::lark_webhook_handler),
        )
        .route(
            "/im/slack/{channel_id}",
            post(crate::im_channels::slack_webhook_handler),
        )
        .route(
            "/im/telegram/{channel_id}",
            post(crate::im_channels::telegram_webhook_handler),
        )
        .route(
            "/im/discord/{channel_id}",
            post(crate::im_channels::discord_webhook_handler),
        )
        .layer(axum::extract::DefaultBodyLimit::max(
            crate::im_channels::MAX_WEBHOOK_BODY_BYTES,
        ));

    Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/ws", get(ws_handler))
        .route("/mcp/agents", post(crate::mcp::mcp_agents_handler))
        .route(
            "/webhooks/{trigger_id}",
            post(crate::triggers::webhook_ingress_handler).layer(DefaultBodyLimit::max(
                crate::triggers::MAX_WEBHOOK_BODY_BYTES,
            )),
        )
        .merge(im_webhook_routes)
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

    let runtime_shutdown = runtime.clone();
    let app = build_http_router(runtime);
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            wait_daemon_shutdown_signal().await;
            runtime_shutdown.orchestrator.flush_all_shim_cursors().await;
            runtime_shutdown.orchestrator.shutdown_worktree_pool().await;
            runtime_shutdown.conditional_timers.shutdown().await;
            runtime_shutdown.goals.shutdown_all().await;
            runtime_shutdown.triggers.shutdown().await;
            runtime_shutdown.workflows.shutdown_all().await;
            runtime_shutdown.im_channels.shutdown_all().await;
            runtime_shutdown.push.shutdown_all().await;
            runtime_shutdown.lsp.shutdown_all().await;
            runtime_shutdown.browser.shutdown_all().await;
            runtime_shutdown.keep_awake.shutdown();
        })
        .await?;
    Ok(())
}

async fn wait_daemon_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate()).ok();
        let mut sigint = signal(SignalKind::interrupt()).ok();
        match (sigterm.as_mut(), sigint.as_mut()) {
            (Some(t), Some(i)) => {
                tokio::select! {
                    _ = t.recv() => {}
                    _ = i.recv() => {}
                }
            }
            (Some(t), None) => {
                let _ = t.recv().await;
            }
            (None, Some(i)) => {
                let _ = i.recv().await;
            }
            (None, None) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
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
                            let summary = format_concise_wire_decode_error(&err);
                            tracing::warn!(error = %summary, bytes = plain.len(), "failed to decode E2EE zstd wire message");
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
    ws.max_frame_size(orbit_protocol::MAX_WS_FRAME_BYTES)
        .max_message_size(orbit_protocol::MAX_WS_FRAME_BYTES)
        .on_upgrade(move |socket| handle_direct_socket(socket, runtime))
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
                                let summary = format_concise_wire_decode_error(&err);
                                tracing::warn!(error = %summary, bytes = bin.len(), "failed to decode direct zstd wire message");
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

fn format_concise_wire_decode_error(err: &impl std::fmt::Display) -> String {
    const MAX_LEN: usize = 240;
    let raw = err.to_string();
    let condensed = if let Some(idx) = raw.find(", expected one of") {
        format!("{}, expected known WireMessage variant", &raw[..idx])
    } else {
        raw
    };
    if condensed.len() <= MAX_LEN {
        return condensed;
    }
    let ellipsis = "…";
    let mut cut = MAX_LEN.saturating_sub(ellipsis.len());
    while cut > 0 && !condensed.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{}", &condensed[..cut], ellipsis)
}

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
                    DaemonBroadcast::KeepAwakeStatusUpdated(status) => {
                        Some(WireMessage::KeepAwakeStatusUpdated { status })
                    }
                    DaemonBroadcast::AcpAgentsUpdated(agents) => {
                        Some(WireMessage::AcpAgentsSnapshot {
                            request_id: Uuid::nil(),
                            agents,
                            warning: None,
                        })
                    }
                    DaemonBroadcast::ArtifactUpdated(artifact) => {
                        Some(WireMessage::ArtifactUpdated {
                            request_id: None,
                            artifact,
                        })
                    }
                    DaemonBroadcast::ArtifactDeleted(artifact_id) => {
                        Some(WireMessage::ArtifactDeleted {
                            request_id: None,
                            artifact_id,
                        })
                    }
                    DaemonBroadcast::ConditionalTimerEvent(timer) => {
                        Some(WireMessage::ConditionalTimerEvent { timer })
                    }
                    DaemonBroadcast::GoalRunEvent(goal) => Some(WireMessage::GoalRunEvent { goal }),
                    DaemonBroadcast::MonitorWatchEvent(monitor) => {
                        Some(WireMessage::MonitorWatchEvent { monitor })
                    }
                    DaemonBroadcast::WorkflowRunEvent(run) => {
                        Some(WireMessage::WorkflowRunEvent { run })
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
        WireMessage::PreviewWorktreeHandoff {
            request_id,
            workspace_id,
            direction,
            target_workspace_id,
            include_committed,
        } => {
            let orch = Arc::clone(&runtime.orchestrator);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match orch
                    .preview_worktree_handoff(
                        &workspace_id,
                        direction,
                        target_workspace_id.as_deref(),
                        include_committed,
                    )
                    .await
                {
                    Ok(preview) => WireMessage::WorktreeHandoffPreviewResult {
                        request_id,
                        preview,
                    },
                    Err(e) => {
                        let msg = e.to_string();
                        let code = if msg.contains("unrelated_git_repo") {
                            "unrelated_git_repo"
                        } else if msg.contains("not_a_git_repo") {
                            "not_a_git_repo"
                        } else if msg.contains("not_a_worktree") {
                            "not_a_worktree"
                        } else if msg.contains("same_directory") {
                            "same_directory"
                        } else if msg.contains("directory_not_found") {
                            "directory_not_found"
                        } else {
                            "handoff_preview_failed"
                        };
                        rpc_err(request_id, code, e)
                    }
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ExecuteWorktreeHandoff {
            request_id,
            workspace_id,
            direction,
            mode,
            target_workspace_id,
            include_committed,
            branch,
            allow_three_way_conflicts,
        } => {
            let orch = Arc::clone(&runtime.orchestrator);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match orch
                    .execute_worktree_handoff(crate::manager::ExecuteWorktreeHandoffSpec {
                        workspace_id,
                        direction,
                        mode,
                        target_workspace_id,
                        include_committed,
                        branch,
                        allow_three_way_conflicts,
                    })
                    .await
                {
                    Ok(outcome) => WireMessage::WorktreeHandoffExecuted {
                        request_id,
                        outcome,
                    },
                    Err(e) => {
                        let msg = e.to_string();
                        let code = if msg.contains("handoff_conflict") {
                            "handoff_conflict"
                        } else if msg.contains("escaping_symlink") {
                            "escaping_symlink"
                        } else if msg.contains("unrelated_git_repo") {
                            "unrelated_git_repo"
                        } else if msg.contains("invalid_git_ref") {
                            "invalid_git_ref"
                        } else if msg.contains("not_a_git_repo") {
                            "not_a_git_repo"
                        } else if msg.contains("not_a_worktree") {
                            "not_a_worktree"
                        } else if msg.contains("same_directory") {
                            "same_directory"
                        } else if msg.contains("directory_not_found") {
                            "directory_not_found"
                        } else {
                            "handoff_execute_failed"
                        };
                        rpc_err(request_id, code, e)
                    }
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
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
        WireMessage::SearchWorkspaceSymbols {
            request_id,
            cwd,
            query,
            limit,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(authorized_cwd) => {
                        match symbol_index
                            .search(&authorized_cwd, &query, limit as usize)
                            .await
                        {
                            Ok(res) => WireMessage::WorkspaceSymbolsResponse {
                                request_id,
                                cwd,
                                symbols: res.symbols,
                                truncated: res.truncated,
                            },
                            Err(e) => rpc_err(request_id, "search_symbols_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "search_symbols_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
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
            let attachments_dir = runtime.attachments.attachments_dir().to_path_buf();
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match read_workspace_file_with_attachments_dir(
                    &resolved_cwd,
                    &path,
                    max_bytes,
                    Some(&attachments_dir),
                )
                .await
                {
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
                        etag: file_data.etag,
                        mtime_unix_ms: file_data.mtime_unix_ms,
                        writable: file_data.writable,
                        line_ending: file_data.line_ending,
                    },
                    Err(e) => rpc_err(request_id, "read_file_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::WriteWorkspaceFile {
            request_id,
            cwd,
            path,
            content,
            expected_etag,
            force,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            let lsp = Arc::clone(&runtime.lsp);
            let hooks = Arc::clone(&runtime.hooks);
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => match write_workspace_file(
                        &authorized_cwd,
                        &path,
                        &content,
                        expected_etag.as_deref(),
                        force,
                    )
                    .await
                    {
                        Ok(outcome) => {
                            if outcome.conflict.is_none() {
                                symbol_index.invalidate(&authorized_cwd);
                                let lsp_cwd = authorized_cwd.clone();
                                let lsp_path = path.clone();
                                let lsp_content = content.clone();
                                tokio::spawn(async move {
                                    lsp.notify_file_saved(&lsp_cwd, &lsp_path, &lsp_content)
                                        .await;
                                });
                                let hook_cwd = authorized_cwd.clone();
                                let hook_path = path.clone();
                                let hook_payload = serde_json::json!({
                                    "event": "post_file_edit",
                                    "cwd": hook_cwd.to_string_lossy().to_string(),
                                    "path": &hook_path,
                                    "source": "workspace_editor",
                                });
                                tokio::spawn(async move {
                                    let _ = hooks
                                        .dispatch_event(
                                            orbit_protocol::HookEventKind::PostFileEdit,
                                            Some(&hook_cwd),
                                            Some(&hook_path),
                                            hook_payload,
                                        )
                                        .await;
                                });
                            }
                            WireMessage::WorkspaceFileWritten {
                                request_id,
                                cwd,
                                path,
                                etag: outcome.etag,
                                size_bytes: outcome.size_bytes,
                                mtime_unix_ms: outcome.mtime_unix_ms,
                                conflict: outcome.conflict,
                            }
                        }
                        Err(e) => rpc_err(request_id, "write_file_failed", e),
                    },
                    Err(e) => rpc_err(request_id, "write_file_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::CreateWorkspaceEntry {
            request_id,
            cwd,
            path,
            is_dir,
            initial_content,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => match create_workspace_entry(
                        &authorized_cwd,
                        &path,
                        is_dir,
                        initial_content.as_deref(),
                    )
                    .await
                    {
                        Ok(outcome) => {
                            symbol_index.invalidate(&authorized_cwd);
                            WireMessage::WorkspaceEntryMutated {
                                request_id,
                                cwd,
                                path: outcome.path,
                                new_path: outcome.new_path,
                                action: outcome.action,
                            }
                        }
                        Err(e) => rpc_err(request_id, "create_entry_failed", e),
                    },
                    Err(e) => rpc_err(request_id, "create_entry_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::RenameWorkspaceEntry {
            request_id,
            cwd,
            old_path,
            new_path,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match rename_workspace_entry(&authorized_cwd, &old_path, &new_path).await {
                            Ok(outcome) => {
                                symbol_index.invalidate(&authorized_cwd);
                                WireMessage::WorkspaceEntryMutated {
                                    request_id,
                                    cwd,
                                    path: outcome.path,
                                    new_path: outcome.new_path,
                                    action: outcome.action,
                                }
                            }
                            Err(e) => rpc_err(request_id, "rename_entry_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "rename_entry_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::DeleteWorkspaceEntry {
            request_id,
            cwd,
            path,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match delete_workspace_entry(&authorized_cwd, &path).await {
                            Ok(outcome) => {
                                symbol_index.invalidate(&authorized_cwd);
                                WireMessage::WorkspaceEntryMutated {
                                    request_id,
                                    cwd,
                                    path: outcome.path,
                                    new_path: outcome.new_path,
                                    action: outcome.action,
                                }
                            }
                            Err(e) => rpc_err(request_id, "delete_entry_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "delete_entry_denied", e),
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
            fast_mode,
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
            acp_agent_id,
        } => {
            let mut reserved_key: Option<String> = None;
            if let Some(ref idem) = idempotency_key
                && !idem.trim().is_empty()
            {
                let hash = hash_payload(&serde_json::json!({
                    "cwd": cwd,
                    "title": title,
                    "provider": provider,
                    "acp_agent_id": acp_agent_id,
                    "model": model,
                    "fast_mode": fast_mode,
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
                    fast_mode,
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
                    acp_agent_id,
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
        WireMessage::ForkAgent {
            request_id,
            source_agent_id,
            up_to_seq,
            target,
            title,
            initial_prompt,
        } => match runtime
            .orchestrator
            .fork_agent(crate::manager::ForkAgentSpec {
                source_agent_id,
                up_to_seq,
                target,
                title,
                initial_prompt,
            })
            .await
        {
            Ok(outcome) => {
                subscribed_agents.insert(outcome.agent.agent_id.clone());
                Some(WireMessage::AgentForked {
                    request_id,
                    agent: outcome.agent,
                    workspace: outcome.workspace,
                })
            }
            Err(e) => {
                let err_code = if e.to_string().contains("not_a_git_repo") {
                    "not_a_git_repo"
                } else {
                    "fork_agent_failed"
                };
                Some(rpc_err(request_id, err_code, e))
            }
        },
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
            acp_agent_id,
        } => match runtime
            .orchestrator
            .switch_agent_provider(&agent_id, provider, model, effort, acp_agent_id)
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
            fast_mode,
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
                fast_mode,
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
        } => match runtime.store.fetch_timeline_since_budgeted(
            &agent_id,
            since_seq,
            2000,
            orbit_protocol::MAX_WIRE_TIMELINE_PAGE_BYTES,
        ) {
            Ok((events, has_more)) => {
                // Only enable live broadcast streaming after the final catchup page is fetched so
                // TimelineLive frames cannot race ahead of paginated catchup history.
                if !has_more {
                    subscribed_agents.insert(agent_id.clone());
                }
                Some(WireMessage::TimelineCatchup {
                    request_id,
                    agent_id,
                    events,
                    has_more,
                })
            }
            Err(e) => Some(rpc_err(request_id, "timeline_error", e)),
        },

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

        // Universal ACP Agent Registry & Live Probing
        WireMessage::ListAcpAgents { request_id } => {
            let home = runtime.home_dir.clone();
            match tokio::task::spawn_blocking(move || orbit_providers::list_acp_agents(&home)).await
            {
                Ok(agents) => Some(WireMessage::AcpAgentsSnapshot {
                    request_id,
                    agents,
                    warning: None,
                }),
                Err(e) => Some(rpc_err(request_id, "list_acp_agents_failed", e)),
            }
        }
        WireMessage::UpsertCustomAcpAgent { request_id, spec } => {
            let home = runtime.home_dir.clone();
            match tokio::task::spawn_blocking(move || {
                orbit_providers::upsert_custom_acp_agent(&home, spec)?;
                Ok::<_, String>(orbit_providers::list_acp_agents(&home))
            })
            .await
            {
                Ok(Ok(agents)) => {
                    let _ = runtime
                        .orchestrator
                        .broadcast_tx
                        .send(DaemonBroadcast::AcpAgentsUpdated(agents.clone()));
                    Some(WireMessage::AcpAgentsSnapshot {
                        request_id,
                        agents,
                        warning: None,
                    })
                }
                Ok(Err(e)) => Some(rpc_err(request_id, "upsert_acp_agent_failed", e)),
                Err(e) => Some(rpc_err(request_id, "upsert_acp_agent_failed", e)),
            }
        }
        WireMessage::DeleteCustomAcpAgent {
            request_id,
            agent_id,
        } => {
            let home = runtime.home_dir.clone();
            let id_for_task = agent_id.clone();
            match tokio::task::spawn_blocking(move || {
                let removed = orbit_providers::delete_custom_acp_agent(&home, &id_for_task)?;
                Ok::<_, String>((removed, orbit_providers::list_acp_agents(&home)))
            })
            .await
            {
                Ok(Ok((true, agents))) => {
                    let _ = runtime
                        .orchestrator
                        .broadcast_tx
                        .send(DaemonBroadcast::AcpAgentsUpdated(agents.clone()));
                    Some(WireMessage::AcpAgentsSnapshot {
                        request_id,
                        agents,
                        warning: None,
                    })
                }
                Ok(Ok((false, _))) => Some(rpc_err(
                    request_id,
                    "not_found",
                    format!("custom ACP agent `{agent_id}` not found"),
                )),
                Ok(Err(e)) => Some(rpc_err(request_id, "delete_acp_agent_failed", e)),
                Err(e) => Some(rpc_err(request_id, "delete_acp_agent_failed", e)),
            }
        }
        WireMessage::TestAcpAgent {
            request_id,
            agent_id,
            cwd,
        } => {
            let home = runtime.home_dir.clone();
            let id_clone = agent_id.clone();
            let spec_opt = tokio::task::spawn_blocking(move || {
                orbit_providers::get_acp_agent_spec(&home, &id_clone)
            })
            .await
            .ok()
            .flatten();
            let result = match spec_opt {
                Some(spec) => {
                    let cwd_path = cwd
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(expand_host_path);
                    orbit_providers::probe_acp_agent(&spec, cwd_path.as_deref()).await
                }
                None => orbit_protocol::AcpProbeResult {
                    agent_id,
                    ok: false,
                    protocol_version: None,
                    server_name: None,
                    server_version: None,
                    capabilities_summary: Vec::new(),
                    auth_methods: Vec::new(),
                    latency_ms: 0,
                    error: Some("ACP agent not found in registry on this host".to_string()),
                },
            };
            Some(WireMessage::AcpAgentProbeResult { request_id, result })
        }
        WireMessage::RefreshAcpRegistry {
            request_id,
            registry_url,
        } => {
            match orbit_providers::refresh_acp_registry(&runtime.home_dir, registry_url.as_deref())
                .await
            {
                Ok((agents, warning)) => {
                    let _ = runtime
                        .orchestrator
                        .broadcast_tx
                        .send(DaemonBroadcast::AcpAgentsUpdated(agents.clone()));
                    Some(WireMessage::AcpAgentsSnapshot {
                        request_id,
                        agents,
                        warning,
                    })
                }
                Err(e) => Some(rpc_err(request_id, "refresh_acp_registry_failed", e)),
            }
        }

        // External MCP Server Manager (stdio + Streamable HTTP/SSE + live probe)
        WireMessage::ListExternalMcpServers { request_id, cwd } => {
            let home = runtime.home_dir.clone();
            let store = Arc::clone(&runtime.store);
            let cache_lock = Arc::clone(&runtime.mcp_probe_cache);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let authorized_cwd = match authorize_optional_mcp_cwd(&store, cwd).await {
                    Ok(dir) => dir,
                    Err(e) => {
                        let _ = tx
                            .send(SessionFrame::Wire {
                                msg: rpc_err(request_id, "unauthorized_workspace_cwd", e),
                                zstd,
                            })
                            .await;
                        return;
                    }
                };
                let cache = cache_lock.read().await.clone();
                let reply = match tokio::task::spawn_blocking(move || {
                    crate::mcp_servers::list_external_mcp_servers(
                        &home,
                        authorized_cwd.as_deref(),
                        &cache,
                    )
                })
                .await
                {
                    Ok(servers) => WireMessage::ExternalMcpServersSnapshot {
                        request_id,
                        servers,
                    },
                    Err(e) => rpc_err(request_id, "list_external_mcp_servers_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::UpsertExternalMcpServer {
            request_id,
            scope,
            cwd,
            name,
            transport,
            env,
            headers,
            keep_existing_secrets,
            enabled,
        } => {
            let home = runtime.home_dir.clone();
            let store = Arc::clone(&runtime.store);
            let cache_lock = Arc::clone(&runtime.mcp_probe_cache);
            let config_lock = Arc::clone(&runtime.mcp_config_lock);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let server_id_to_clear = match scope {
                orbit_protocol::McpWriteScope::User => format!("orbit_user:{}", name.trim()),
                orbit_protocol::McpWriteScope::Project => {
                    format!("project_dot_mcp:{}", name.trim())
                }
            };
            tokio::spawn(async move {
                let authorized_cwd = match authorize_optional_mcp_cwd(&store, cwd).await {
                    Ok(dir) => dir,
                    Err(e) => {
                        let _ = tx
                            .send(SessionFrame::Wire {
                                msg: rpc_err(request_id, "unauthorized_workspace_cwd", e),
                                zstd,
                            })
                            .await;
                        return;
                    }
                };
                let _config_guard = config_lock.lock().await;
                let cache_key = crate::mcp_servers::probe_cache_key(
                    &server_id_to_clear,
                    authorized_cwd.as_deref(),
                );
                let mut cache_snapshot = cache_lock.read().await.clone();
                cache_snapshot.remove(&cache_key);
                let input = crate::mcp_servers::UpsertExternalMcpInput {
                    scope,
                    cwd: authorized_cwd,
                    name,
                    transport,
                    env,
                    headers,
                    keep_existing_secrets,
                    enabled,
                };
                let reply = match tokio::task::spawn_blocking(move || {
                    crate::mcp_servers::upsert_external_mcp_server(&home, input, &cache_snapshot)
                })
                .await
                {
                    Ok(Ok(servers)) => {
                        cache_lock.write().await.remove(&cache_key);
                        WireMessage::ExternalMcpServersSnapshot {
                            request_id,
                            servers,
                        }
                    }
                    Ok(Err(e)) => rpc_err(request_id, "upsert_external_mcp_server_failed", e),
                    Err(e) => rpc_err(request_id, "upsert_external_mcp_server_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ToggleExternalMcpServer {
            request_id,
            server_id,
            enabled,
            cwd,
        } => {
            let home = runtime.home_dir.clone();
            let store = Arc::clone(&runtime.store);
            let cache_lock = Arc::clone(&runtime.mcp_probe_cache);
            let config_lock = Arc::clone(&runtime.mcp_config_lock);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let authorized_cwd = match authorize_optional_mcp_cwd(&store, cwd).await {
                    Ok(dir) => dir,
                    Err(e) => {
                        let _ = tx
                            .send(SessionFrame::Wire {
                                msg: rpc_err(request_id, "unauthorized_workspace_cwd", e),
                                zstd,
                            })
                            .await;
                        return;
                    }
                };
                let _config_guard = config_lock.lock().await;
                let cache = cache_lock.read().await.clone();
                let reply = match tokio::task::spawn_blocking(move || {
                    crate::mcp_servers::toggle_external_mcp_server(
                        &home,
                        &server_id,
                        enabled,
                        authorized_cwd.as_deref(),
                        &cache,
                    )
                })
                .await
                {
                    Ok(Ok(servers)) => WireMessage::ExternalMcpServersSnapshot {
                        request_id,
                        servers,
                    },
                    Ok(Err(e)) => rpc_err(request_id, "toggle_external_mcp_server_failed", e),
                    Err(e) => rpc_err(request_id, "toggle_external_mcp_server_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::DeleteExternalMcpServer {
            request_id,
            server_id,
            cwd,
        } => {
            let home = runtime.home_dir.clone();
            let store = Arc::clone(&runtime.store);
            let cache_lock = Arc::clone(&runtime.mcp_probe_cache);
            let config_lock = Arc::clone(&runtime.mcp_config_lock);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let authorized_cwd = match authorize_optional_mcp_cwd(&store, cwd).await {
                    Ok(dir) => dir,
                    Err(e) => {
                        let _ = tx
                            .send(SessionFrame::Wire {
                                msg: rpc_err(request_id, "unauthorized_workspace_cwd", e),
                                zstd,
                            })
                            .await;
                        return;
                    }
                };
                let _config_guard = config_lock.lock().await;
                let cache_key =
                    crate::mcp_servers::probe_cache_key(&server_id, authorized_cwd.as_deref());
                let mut cache_snapshot = cache_lock.read().await.clone();
                cache_snapshot.remove(&cache_key);
                let reply = match tokio::task::spawn_blocking(move || {
                    crate::mcp_servers::delete_external_mcp_server(
                        &home,
                        &server_id,
                        authorized_cwd.as_deref(),
                        &cache_snapshot,
                    )
                })
                .await
                {
                    Ok(Ok(servers)) => {
                        cache_lock.write().await.remove(&cache_key);
                        WireMessage::ExternalMcpServersSnapshot {
                            request_id,
                            servers,
                        }
                    }
                    Ok(Err(e)) => rpc_err(request_id, "delete_external_mcp_server_failed", e),
                    Err(e) => rpc_err(request_id, "delete_external_mcp_server_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ProbeExternalMcpServer {
            request_id,
            server_id,
            cwd,
        } => {
            let home = runtime.home_dir.clone();
            let store = Arc::clone(&runtime.store);
            let cache_lock = Arc::clone(&runtime.mcp_probe_cache);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let authorized_cwd = match authorize_optional_mcp_cwd(&store, cwd).await {
                    Ok(dir) => dir,
                    Err(e) => {
                        let _ = tx
                            .send(SessionFrame::Wire {
                                msg: rpc_err(request_id, "unauthorized_workspace_cwd", e),
                                zstd,
                            })
                            .await;
                        return;
                    }
                };
                let result = crate::mcp_servers::probe_external_mcp_server(
                    &home,
                    &server_id,
                    authorized_cwd.as_deref(),
                )
                .await;
                let cache_key =
                    crate::mcp_servers::probe_cache_key(&server_id, authorized_cwd.as_deref());
                {
                    let mut guard = cache_lock.write().await;
                    crate::mcp_servers::insert_probe_cache(&mut guard, cache_key, result.clone());
                }
                let reply = WireMessage::ExternalMcpProbeResponse {
                    request_id,
                    server_id,
                    result,
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // Pure-Rust LSP Language Server Integration (offloaded async + strict workspace cwd authorization)
        WireMessage::ListLspServers { request_id, cwd } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => match lsp.list_servers(&authorized_cwd).await {
                        Ok(servers) => WireMessage::LspServersSnapshot {
                            request_id,
                            cwd,
                            servers,
                        },
                        Err(e) => rpc_err(request_id, "list_lsp_servers_failed", e),
                    },
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::LspQuerySymbols {
            request_id,
            cwd,
            query,
            path,
            limit,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => match lsp
                        .query_symbols(&authorized_cwd, &query, path.as_deref(), limit)
                        .await
                    {
                        Ok(symbols) => WireMessage::LspSymbolsResponse {
                            request_id,
                            cwd,
                            symbols,
                        },
                        Err(e) => rpc_err(request_id, "lsp_query_symbols_failed", e),
                    },
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::LspGetDiagnostics {
            request_id,
            cwd,
            path,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match lsp.get_diagnostics(&authorized_cwd, path.as_deref()).await {
                            Ok(diagnostics) => WireMessage::LspDiagnosticsResponse {
                                request_id,
                                cwd,
                                diagnostics,
                            },
                            Err(e) => rpc_err(request_id, "lsp_diagnostics_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::LspGotoDefinition {
            request_id,
            cwd,
            path,
            line,
            col,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match lsp.goto_definition(&authorized_cwd, &path, line, col).await {
                            Ok(locations) => WireMessage::LspLocationsResponse {
                                request_id,
                                cwd,
                                locations,
                            },
                            Err(e) => rpc_err(request_id, "lsp_goto_definition_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::RestartLspServer {
            request_id,
            cwd,
            server_id,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match lsp.restart_server(&authorized_cwd, &server_id).await {
                            Ok(servers) => WireMessage::LspServersSnapshot {
                                request_id,
                                cwd,
                                servers,
                            },
                            Err(e) => rpc_err(request_id, "restart_lsp_server_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // Early-Wakeup One-Shot Conditional Timers
        WireMessage::CreateConditionalTimer {
            request_id,
            agent_id,
            duration_seconds,
            prompt,
            condition,
        } => {
            let timers = Arc::clone(&runtime.conditional_timers);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match timers
                    .create_timer(&agent_id, duration_seconds, prompt, condition)
                    .await
                {
                    Ok(timer) => WireMessage::ConditionalTimerCreated { request_id, timer },
                    Err(e) => rpc_err(request_id, "create_conditional_timer_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ListConditionalTimers {
            request_id,
            agent_id,
            include_resolved,
        } => {
            let timers = Arc::clone(&runtime.conditional_timers);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || {
                    timers.list_timers(agent_id.as_deref(), include_resolved)
                })
                .await
                {
                    Ok(Ok(timers)) => WireMessage::ConditionalTimersSnapshot { request_id, timers },
                    Ok(Err(e)) => rpc_err(request_id, "list_conditional_timers_failed", e),
                    Err(e) => rpc_err(request_id, "list_conditional_timers_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::CancelConditionalTimer {
            request_id,
            timer_id,
        } => {
            let timers = Arc::clone(&runtime.conditional_timers);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match timers.cancel_timer(&timer_id).await {
                    Ok(timer) => WireMessage::ConditionalTimerUpdated { request_id, timer },
                    Err(e) => rpc_err(request_id, "cancel_conditional_timer_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::FireConditionalTimerNow {
            request_id,
            timer_id,
        } => {
            let timers = Arc::clone(&runtime.conditional_timers);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match timers.fire_timer_now(&timer_id).await {
                    Ok(timer) => WireMessage::ConditionalTimerUpdated { request_id, timer },
                    Err(e) => rpc_err(request_id, "fire_conditional_timer_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // Guardian AI Risk-Assessment Auto-Approval Policy & Audit Log
        WireMessage::GetGuardianConfig { request_id } => {
            let (config, recent_audits) = runtime.orchestrator.guardian.snapshot().await;
            Some(WireMessage::GuardianConfigSnapshot {
                request_id,
                config,
                recent_audits,
            })
        }
        WireMessage::UpdateGuardianConfig { request_id, config } => {
            match runtime.orchestrator.guardian.update_config(config).await {
                Ok((saved, recent_audits)) => Some(WireMessage::GuardianConfigSnapshot {
                    request_id,
                    config: saved,
                    recent_audits,
                }),
                Err(e) => Some(rpc_err(request_id, "update_guardian_config_failed", e)),
            }
        }

        // Unattended Autonomous Goal Mode & Streaming Monitor Engine
        WireMessage::StartGoalRun {
            request_id,
            agent_id,
            objective,
            criteria,
            verify_command,
            max_iterations,
            max_duration_secs,
        } => {
            let goals = Arc::clone(&runtime.goals);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match goals
                    .start_goal(StartGoalSpec {
                        agent_id,
                        objective,
                        criteria,
                        verify_command,
                        max_iterations,
                        max_duration_secs,
                        dispatch_initial_prompt: true,
                    })
                    .await
                {
                    Ok(goal) => WireMessage::GoalRunUpdated { request_id, goal },
                    Err(e) => rpc_err(request_id, "start_goal_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::ListGoalRuns {
            request_id,
            agent_id,
        } => {
            let goals = Arc::clone(&runtime.goals);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || {
                    goals.list_goals(agent_id.as_deref())
                })
                .await
                {
                    Ok(Ok(goals)) => WireMessage::GoalRunsSnapshot { request_id, goals },
                    Ok(Err(e)) => rpc_err(request_id, "list_goals_failed", e),
                    Err(e) => rpc_err(request_id, "list_goals_failed", e.to_string()),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::ControlGoalRun {
            request_id,
            goal_id,
            action,
        } => {
            let goals = Arc::clone(&runtime.goals);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match goals.control_goal(&goal_id, action).await {
                    Ok(goal) => WireMessage::GoalRunUpdated { request_id, goal },
                    Err(e) => rpc_err(request_id, "control_goal_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::StartMonitorWatch {
            request_id,
            agent_id,
            name,
            kind,
            expires_in_secs,
        } => {
            let goals = Arc::clone(&runtime.goals);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match goals
                    .start_monitor(StartMonitorSpec {
                        agent_id,
                        name,
                        kind,
                        expires_in_secs,
                    })
                    .await
                {
                    Ok(monitor) => WireMessage::MonitorWatchUpdated {
                        request_id,
                        monitor,
                    },
                    Err(e) => rpc_err(request_id, "start_monitor_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::ListMonitorWatches {
            request_id,
            agent_id,
        } => {
            let goals = Arc::clone(&runtime.goals);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || {
                    goals.list_monitors(agent_id.as_deref())
                })
                .await
                {
                    Ok(Ok(monitors)) => WireMessage::MonitorWatchesSnapshot {
                        request_id,
                        monitors,
                    },
                    Ok(Err(e)) => rpc_err(request_id, "list_monitors_failed", e),
                    Err(e) => rpc_err(request_id, "list_monitors_failed", e.to_string()),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::StopMonitorWatch {
            request_id,
            monitor_id,
        } => {
            let goals = Arc::clone(&runtime.goals);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match goals.stop_monitor(&monitor_id).await {
                    Ok(monitor) => WireMessage::MonitorWatchUpdated {
                        request_id,
                        monitor,
                    },
                    Err(e) => rpc_err(request_id, "stop_monitor_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // Pure-Rust CDP Browser Automation & Guarded Computer Use
        WireMessage::ListBrowserSessions {
            request_id,
            include_screenshots,
        } => {
            let browser = Arc::clone(&runtime.browser);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let snap = browser
                    .list_snapshot_with_options(include_screenshots)
                    .await;
                let reply = WireMessage::BrowserSessionsSnapshot {
                    request_id,
                    sessions: snap.sessions,
                    policy: snap.policy,
                    chrome_binary: snap.chrome_binary,
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::CreateBrowserSession {
            request_id,
            agent_id,
            initial_url,
            headless,
            viewport_width,
            viewport_height,
        } => {
            let browser = Arc::clone(&runtime.browser);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match browser
                    .create_session(CreateBrowserSessionSpec {
                        agent_id,
                        initial_url,
                        headless,
                        viewport_width,
                        viewport_height,
                    })
                    .await
                {
                    Ok(session) => WireMessage::BrowserSessionUpdated {
                        request_id,
                        session,
                    },
                    Err(e) => rpc_err(request_id, "create_browser_session_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::ExecuteBrowserAction {
            request_id,
            session_id,
            action,
        } => {
            let browser = Arc::clone(&runtime.browser);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match browser.execute_action(&session_id, action).await {
                    Ok(outcome) => WireMessage::BrowserActionResponse {
                        request_id,
                        session: outcome.session,
                        elements: outcome.elements,
                        screenshot_base64: outcome.screenshot_base64,
                        text_excerpt: outcome.text_excerpt,
                        captcha_detected: outcome.captcha_detected,
                    },
                    Err(e) => rpc_err(request_id, "execute_browser_action_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::CloseBrowserSession {
            request_id,
            session_id,
        } => {
            let browser = Arc::clone(&runtime.browser);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match browser.close_session(&session_id).await {
                    Ok(snap) => WireMessage::BrowserSessionsSnapshot {
                        request_id,
                        sessions: snap.sessions,
                        policy: snap.policy,
                        chrome_binary: snap.chrome_binary,
                    },
                    Err(e) => rpc_err(request_id, "close_browser_session_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::UpdateBrowserPolicy { request_id, policy } => {
            let browser = Arc::clone(&runtime.browser);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match browser.update_policy(policy).await {
                    Ok(snap) => WireMessage::BrowserSessionsSnapshot {
                        request_id,
                        sessions: snap.sessions,
                        policy: snap.policy,
                        chrome_binary: snap.chrome_binary,
                    },
                    Err(e) => rpc_err(request_id, "update_browser_policy_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // Batch CSV Fan-Out, Programmable DAG Workflows & Best-of-N Parallel Arena
        WireMessage::StartBatchFanout {
            request_id,
            name,
            cwd,
            csv_content,
            prompt_template,
            provider,
            model,
            mode,
            max_concurrency,
            isolated_worktree,
        } => {
            let workflows = Arc::clone(&runtime.workflows);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match workflows
                    .start_batch_fanout(StartBatchFanoutSpec {
                        name,
                        cwd,
                        owner_agent_id: None,
                        csv_content,
                        prompt_template,
                        provider,
                        model,
                        mode,
                        max_concurrency,
                        isolated_worktree,
                    })
                    .await
                {
                    Ok(run) => WireMessage::WorkflowRunUpdated { request_id, run },
                    Err(e) => rpc_err(request_id, "start_batch_fanout_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::StartDagWorkflow {
            request_id,
            name,
            cwd,
            steps,
            default_provider,
            default_mode,
            max_concurrency,
        } => {
            let workflows = Arc::clone(&runtime.workflows);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match workflows
                    .start_dag_workflow(StartDagWorkflowSpec {
                        name,
                        cwd,
                        owner_agent_id: None,
                        steps,
                        default_provider,
                        default_mode,
                        max_concurrency,
                    })
                    .await
                {
                    Ok(run) => WireMessage::WorkflowRunUpdated { request_id, run },
                    Err(e) => rpc_err(request_id, "start_dag_workflow_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::StartBestOfN {
            request_id,
            name,
            cwd,
            prompt,
            candidates,
            mode,
            verify_command,
        } => {
            let workflows = Arc::clone(&runtime.workflows);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match workflows
                    .start_best_of_n(StartBestOfNSpec {
                        name,
                        cwd,
                        owner_agent_id: None,
                        prompt,
                        candidates,
                        mode,
                        verify_command,
                    })
                    .await
                {
                    Ok(run) => WireMessage::WorkflowRunUpdated { request_id, run },
                    Err(e) => rpc_err(request_id, "start_best_of_n_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::ListWorkflowRuns {
            request_id,
            include_completed,
        } => {
            let workflows = Arc::clone(&runtime.workflows);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || {
                    workflows.list_runs(include_completed)
                })
                .await
                {
                    Ok(Ok(runs)) => WireMessage::WorkflowRunsSnapshot { request_id, runs },
                    Ok(Err(e)) => rpc_err(request_id, "list_workflow_runs_failed", e),
                    Err(e) => rpc_err(request_id, "list_workflow_runs_failed", e.to_string()),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::CancelWorkflowRun { request_id, run_id } => {
            let workflows = Arc::clone(&runtime.workflows);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match workflows.cancel_run(&run_id).await {
                    Ok(run) => WireMessage::WorkflowRunUpdated { request_id, run },
                    Err(e) => rpc_err(request_id, "cancel_workflow_run_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        WireMessage::SelectBestOfNWinner {
            request_id,
            run_id,
            winner_step_id,
            apply_to_workspace,
        } => {
            let workflows = Arc::clone(&runtime.workflows);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match workflows
                    .select_best_of_n_winner(&run_id, &winner_step_id, apply_to_workspace)
                    .await
                {
                    Ok(run) => WireMessage::WorkflowRunUpdated { request_id, run },
                    Err(e) => rpc_err(request_id, "select_best_of_n_winner_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // First-Class Artifacts & SEP-1865 MCP Apps (offloaded via spawn_blocking)
        WireMessage::ListArtifacts {
            request_id,
            workspace_id,
            agent_id,
        } => {
            let orch = Arc::clone(&runtime.orchestrator);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || {
                    orch.list_artifacts(workspace_id.as_deref(), agent_id.as_deref())
                })
                .await
                {
                    Ok(Ok(artifacts)) => WireMessage::ArtifactsSnapshot {
                        request_id,
                        artifacts,
                    },
                    Ok(Err(e)) => rpc_err(request_id, "list_artifacts_failed", e),
                    Err(e) => rpc_err(request_id, "list_artifacts_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GetArtifact {
            request_id,
            artifact_id,
            version,
        } => {
            let orch = Arc::clone(&runtime.orchestrator);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let id_for_err = artifact_id.clone();
                let reply = match tokio::task::spawn_blocking(move || {
                    orch.get_artifact(&artifact_id, version)
                })
                .await
                {
                    Ok(Ok(Some(artifact))) => WireMessage::ArtifactContent {
                        request_id,
                        artifact,
                    },
                    Ok(Ok(None)) => rpc_err(
                        request_id,
                        "artifact_not_found",
                        format!("artifact `{id_for_err}` not found"),
                    ),
                    Ok(Err(e)) => rpc_err(request_id, "get_artifact_failed", e),
                    Err(e) => rpc_err(request_id, "get_artifact_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::UpsertArtifact {
            request_id,
            artifact_id,
            workspace_id,
            agent_id,
            title,
            kind,
            language,
            mime_type,
            resource_uri,
            content,
            summary_note,
        } => {
            let orch = Arc::clone(&runtime.orchestrator);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || {
                    orch.upsert_artifact(orbit_store::UpsertArtifactInput {
                        artifact_id,
                        workspace_id,
                        agent_id,
                        title,
                        kind,
                        language,
                        mime_type,
                        resource_uri,
                        content,
                        summary_note,
                    })
                })
                .await
                {
                    Ok(Ok(artifact)) => WireMessage::ArtifactUpdated {
                        request_id: Some(request_id),
                        artifact,
                    },
                    Ok(Err(e)) => rpc_err(request_id, "upsert_artifact_failed", e),
                    Err(e) => rpc_err(request_id, "upsert_artifact_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::DeleteArtifact {
            request_id,
            artifact_id,
        } => {
            let orch = Arc::clone(&runtime.orchestrator);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let id_for_reply = artifact_id.clone();
                let reply =
                    match tokio::task::spawn_blocking(move || orch.delete_artifact(&artifact_id))
                        .await
                    {
                        Ok(Ok(true)) => WireMessage::ArtifactDeleted {
                            request_id: Some(request_id),
                            artifact_id: id_for_reply,
                        },
                        Ok(Ok(false)) => rpc_err(
                            request_id,
                            "artifact_not_found",
                            format!("artifact `{id_for_reply}` not found"),
                        ),
                        Ok(Err(e)) => rpc_err(request_id, "delete_artifact_failed", e),
                        Err(e) => rpc_err(request_id, "delete_artifact_failed", e),
                    };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // Project Instruction Hierarchy Discovery & Atomic Editing
        WireMessage::ListInstructionFiles { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match discover_instruction_hierarchy(&authorized_cwd).await {
                            Ok(res) => WireMessage::InstructionFilesSnapshot {
                                request_id,
                                cwd,
                                files: res.files,
                                total_tokens: res.total_tokens,
                                merged_preview: res.merged_preview,
                            },
                            Err(e) => rpc_err(request_id, e.rpc_code(), e.to_string()),
                        }
                    }
                    Err(e) => rpc_err(request_id, "instruction_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ReadInstructionFile {
            request_id,
            cwd,
            path,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => match read_instruction_file(&authorized_cwd, &path).await
                    {
                        Ok(res) => WireMessage::InstructionFileContentResponse {
                            request_id,
                            cwd,
                            path: res.path,
                            content: res.content,
                            etag: res.etag,
                            exists: res.exists,
                        },
                        Err(e) => rpc_err(request_id, e.rpc_code(), e.to_string()),
                    },
                    Err(e) => rpc_err(request_id, "instruction_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::SaveInstructionFile {
            request_id,
            cwd,
            path,
            content,
            expected_etag,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => match save_instruction_file(
                        &authorized_cwd,
                        &path,
                        &content,
                        expected_etag.as_deref(),
                    )
                    .await
                    {
                        Ok(res) => WireMessage::InstructionFileSavedResponse {
                            request_id,
                            cwd,
                            path: res.path,
                            etag: res.etag,
                            size_bytes: res.size_bytes,
                            approx_tokens: res.approx_tokens,
                        },
                        Err(e) => rpc_err(request_id, e.rpc_code(), e.to_string()),
                    },
                    Err(e) => rpc_err(request_id, "instruction_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // Path-Scoped Glob Rules (.orbit/rules/*.md, .claude/rules/*.md, .cursor/rules/*.mdc)
        WireMessage::ListPathRules { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let home_dir = runtime.home_dir.clone();
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let auth_res = authorize_path_rules_cwd(&store, &cwd).await;
                let reply = match auth_res {
                    Ok(authorized_cwd) => match tokio::task::spawn_blocking(move || {
                        crate::rules::discover_path_rules(&home_dir, authorized_cwd.as_deref())
                    })
                    .await
                    {
                        Ok(snap) => WireMessage::PathRulesSnapshot {
                            request_id,
                            cwd,
                            rules: snap.rules,
                            total_always_tokens: snap.total_always_tokens,
                            token_budget_warning: snap.token_budget_warning,
                        },
                        Err(e) => rpc_err(request_id, "list_path_rules_failed", e),
                    },
                    Err(e) => rpc_err(request_id, "list_path_rules_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::UpsertPathRule {
            request_id,
            cwd,
            scope,
            name,
            description,
            trigger_mode,
            globs,
            enabled,
            body,
            expected_etag,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let home_dir = runtime.home_dir.clone();
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let auth_res = authorize_path_rules_cwd(&store, &cwd).await;
                let reply = match auth_res {
                    Ok(authorized_cwd) => {
                        let spec = crate::rules::UpsertRuleSpec {
                            scope,
                            name,
                            description,
                            trigger_mode,
                            globs,
                            enabled,
                            body,
                            expected_etag,
                        };
                        match tokio::task::spawn_blocking(move || {
                            crate::rules::upsert_path_rule(
                                &home_dir,
                                authorized_cwd.as_deref(),
                                &spec,
                            )
                        })
                        .await
                        {
                            Ok(Ok(snap)) => WireMessage::PathRulesSnapshot {
                                request_id,
                                cwd,
                                rules: snap.rules,
                                total_always_tokens: snap.total_always_tokens,
                                token_budget_warning: snap.token_budget_warning,
                            },
                            Ok(Err(e)) => rpc_err(request_id, "upsert_path_rule_failed", e),
                            Err(e) => rpc_err(request_id, "upsert_path_rule_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "upsert_path_rule_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::DeletePathRule {
            request_id,
            cwd,
            path,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let home_dir = runtime.home_dir.clone();
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let auth_res = authorize_path_rules_cwd(&store, &cwd).await;
                let reply = match auth_res {
                    Ok(authorized_cwd) => match tokio::task::spawn_blocking(move || {
                        crate::rules::delete_path_rule(&home_dir, authorized_cwd.as_deref(), &path)
                    })
                    .await
                    {
                        Ok(Ok(snap)) => WireMessage::PathRulesSnapshot {
                            request_id,
                            cwd,
                            rules: snap.rules,
                            total_always_tokens: snap.total_always_tokens,
                            token_budget_warning: snap.token_budget_warning,
                        },
                        Ok(Err(e)) => rpc_err(request_id, "delete_path_rule_failed", e),
                        Err(e) => rpc_err(request_id, "delete_path_rule_failed", e),
                    },
                    Err(e) => rpc_err(request_id, "delete_path_rule_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::MatchPathRules {
            request_id,
            cwd,
            active_paths,
            manual_rule_names,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let home_dir = runtime.home_dir.clone();
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let auth_res = authorize_path_rules_cwd(&store, &cwd).await;
                let reply = match auth_res {
                    Ok(authorized_cwd) => match tokio::task::spawn_blocking(move || {
                        crate::rules::match_rules_for_workspace(
                            &home_dir,
                            authorized_cwd.as_deref(),
                            &active_paths,
                            &manual_rule_names,
                        )
                    })
                    .await
                    {
                        Ok(res) => WireMessage::MatchedPathRulesResponse {
                            request_id,
                            cwd,
                            matched: res.matched,
                            injected_markdown: res.injected_markdown,
                            total_tokens: res.total_tokens,
                        },
                        Err(e) => rpc_err(request_id, "match_path_rules_failed", e),
                    },
                    Err(e) => rpc_err(request_id, "match_path_rules_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // Deterministic Lifecycle Hooks (`$ORBIT_HOME/hooks.json` & `<cwd>/.orbit/hooks.json`)
        WireMessage::ListLifecycleHooks { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let hooks = Arc::clone(&runtime.hooks);
            tokio::spawn(async move {
                let auth_res = authorize_optional_hooks_cwd(&store, cwd).await;
                let reply = match auth_res {
                    Ok(authorized_cwd) => {
                        match hooks.list_snapshot(authorized_cwd.as_deref()).await {
                            Ok(snap) => WireMessage::LifecycleHooksSnapshot {
                                request_id,
                                hooks: snap.hooks,
                                recent_executions: snap.recent_executions,
                                project_hooks_trusted: snap.project_hooks_trusted,
                            },
                            Err(e) => rpc_err(request_id, "list_lifecycle_hooks_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "list_lifecycle_hooks_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::UpsertLifecycleHook {
            request_id,
            cwd,
            hook,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let hooks = Arc::clone(&runtime.hooks);
            tokio::spawn(async move {
                let auth_res = authorize_optional_hooks_cwd(&store, cwd).await;
                let reply = match auth_res {
                    Ok(authorized_cwd) => {
                        match hooks.upsert_hook(authorized_cwd.as_deref(), hook).await {
                            Ok(snap) => WireMessage::LifecycleHooksSnapshot {
                                request_id,
                                hooks: snap.hooks,
                                recent_executions: snap.recent_executions,
                                project_hooks_trusted: snap.project_hooks_trusted,
                            },
                            Err(e) => rpc_err(request_id, "upsert_lifecycle_hook_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "upsert_lifecycle_hook_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::DeleteLifecycleHook {
            request_id,
            cwd,
            hook_id,
            scope,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let hooks = Arc::clone(&runtime.hooks);
            tokio::spawn(async move {
                let auth_res = authorize_optional_hooks_cwd(&store, cwd).await;
                let reply = match auth_res {
                    Ok(authorized_cwd) => match hooks
                        .delete_hook(authorized_cwd.as_deref(), &hook_id, scope)
                        .await
                    {
                        Ok(snap) => WireMessage::LifecycleHooksSnapshot {
                            request_id,
                            hooks: snap.hooks,
                            recent_executions: snap.recent_executions,
                            project_hooks_trusted: snap.project_hooks_trusted,
                        },
                        Err(e) => rpc_err(request_id, "delete_lifecycle_hook_failed", e),
                    },
                    Err(e) => rpc_err(request_id, "delete_lifecycle_hook_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::SetProjectHooksTrust {
            request_id,
            cwd,
            trusted,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let hooks = Arc::clone(&runtime.hooks);
            tokio::spawn(async move {
                let auth_res = match authorize_optional_hooks_cwd(&store, Some(cwd)).await {
                    Ok(Some(dir)) => Ok(dir),
                    Ok(None) => Err(
                        "A valid workspace directory is required to update project hook trust"
                            .to_string(),
                    ),
                    Err(e) => Err(e),
                };
                let reply = match auth_res {
                    Ok(authorized_cwd) => {
                        match hooks.set_project_trust(&authorized_cwd, trusted).await {
                            Ok(snap) => WireMessage::LifecycleHooksSnapshot {
                                request_id,
                                hooks: snap.hooks,
                                recent_executions: snap.recent_executions,
                                project_hooks_trusted: snap.project_hooks_trusted,
                            },
                            Err(e) => rpc_err(request_id, "set_project_hooks_trust_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "set_project_hooks_trust_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::TestLifecycleHook {
            request_id,
            cwd,
            hook_id,
            sample_payload,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let hooks = Arc::clone(&runtime.hooks);
            tokio::spawn(async move {
                let auth_res = authorize_optional_hooks_cwd(&store, cwd).await;
                let reply = match auth_res {
                    Ok(authorized_cwd) => match hooks
                        .test_hook(authorized_cwd.as_deref(), &hook_id, sample_payload)
                        .await
                    {
                        Ok(record) => WireMessage::HookTestResultResponse { request_id, record },
                        Err(e) => rpc_err(request_id, "test_lifecycle_hook_failed", e),
                    },
                    Err(e) => rpc_err(request_id, "test_lifecycle_hook_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

        // External Event Triggers (GitHub, Slack, Linear, Generic Webhook + GitHub PR Polling)
        WireMessage::ListEventTriggers { request_id } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let triggers = Arc::clone(&runtime.triggers);
            tokio::spawn(async move {
                let reply = match triggers.list_snapshot().await {
                    Ok(snap) => WireMessage::EventTriggersSnapshot {
                        request_id,
                        triggers: snap.triggers,
                        recent_deliveries: snap.recent_deliveries,
                        base_webhook_url: snap.base_webhook_url,
                    },
                    Err(e) => rpc_err(request_id, "list_event_triggers_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::UpsertEventTrigger {
            request_id,
            trigger_id,
            name,
            source,
            event_filters,
            prompt_template,
            target,
            secret,
            keep_existing_secret,
            github_poll,
            github_token,
            keep_existing_token,
            enabled,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let triggers = Arc::clone(&runtime.triggers);
            tokio::spawn(async move {
                let req = UpsertTriggerRequest {
                    trigger_id,
                    name,
                    source,
                    event_filters,
                    prompt_template,
                    target,
                    secret,
                    keep_existing_secret,
                    github_poll,
                    github_token,
                    keep_existing_token,
                    enabled,
                };
                let reply = match triggers.upsert_trigger(req).await {
                    Ok(snap) => WireMessage::EventTriggersSnapshot {
                        request_id,
                        triggers: snap.triggers,
                        recent_deliveries: snap.recent_deliveries,
                        base_webhook_url: snap.base_webhook_url,
                    },
                    Err(e) => rpc_err(request_id, "upsert_event_trigger_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::DeleteEventTrigger {
            request_id,
            trigger_id,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let triggers = Arc::clone(&runtime.triggers);
            tokio::spawn(async move {
                let reply = match triggers.delete_trigger(&trigger_id).await {
                    Ok(snap) => WireMessage::EventTriggersSnapshot {
                        request_id,
                        triggers: snap.triggers,
                        recent_deliveries: snap.recent_deliveries,
                        base_webhook_url: snap.base_webhook_url,
                    },
                    Err(e) => rpc_err(request_id, "delete_event_trigger_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::TestEventTrigger {
            request_id,
            trigger_id,
            sample_event_type,
            sample_payload,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let triggers = Arc::clone(&runtime.triggers);
            tokio::spawn(async move {
                let reply = match triggers
                    .test_trigger(&trigger_id, &sample_event_type, sample_payload)
                    .await
                {
                    Ok(delivery) => WireMessage::EventTriggerTestResponse {
                        request_id,
                        delivery,
                    },
                    Err(e) => rpc_err(request_id, "test_event_trigger_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }

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
            timezone,
            prompt,
            max_runs,
            expires_at,
            archive_on_finish,
        } => match runtime.scheduler.update_schedule(
            &schedule_id,
            name,
            cron_expr.as_deref(),
            timezone.as_deref(),
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
            runtime
                .keep_awake
                .set_enabled(config.keep_awake_while_running);
            let mut persisted = DaemonPersistedConfig::load(&runtime.home_dir);
            persisted.orchestration = config.clone();
            match persisted.save(&runtime.home_dir) {
                Ok(()) => Some(WireMessage::OrchestrationConfigSnapshot { request_id, config }),
                Err(e) => Some(rpc_err(request_id, "save_config_failed", e)),
            }
        }
        WireMessage::GetKeepAwakeStatus { request_id } => {
            let status = runtime.keep_awake.status();
            Some(WireMessage::KeepAwakeStatusSnapshot { request_id, status })
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
        WireMessage::ListManagedSkills { request_id, cwd } => {
            let authorized_cwd = match authorize_optional_skills_cwd(&runtime.store, cwd).await {
                Ok(c) => c,
                Err(e) => return Some(rpc_err(request_id, "skill_permission_denied", e)),
            };
            let orbit_home = runtime.home_dir.clone();
            let user_home = resolve_user_home();
            match tokio::task::spawn_blocking(move || {
                crate::skills_manager::list_managed_skills(
                    &orbit_home,
                    &user_home,
                    authorized_cwd.as_deref(),
                )
            })
            .await
            {
                Ok(skills) => Some(WireMessage::ManagedSkillsSnapshot { request_id, skills }),
                Err(e) => Some(rpc_err(request_id, "list_managed_skills_failed", e)),
            }
        }
        WireMessage::ReadManagedSkill {
            request_id,
            skill_id,
            cwd,
        } => {
            let authorized_cwd = match authorize_optional_skills_cwd(&runtime.store, cwd).await {
                Ok(c) => c,
                Err(e) => return Some(rpc_err(request_id, "skill_permission_denied", e)),
            };
            let orbit_home = runtime.home_dir.clone();
            let user_home = resolve_user_home();
            match tokio::task::spawn_blocking(move || {
                crate::skills_manager::read_managed_skill(
                    &orbit_home,
                    &user_home,
                    &skill_id,
                    authorized_cwd.as_deref(),
                )
            })
            .await
            {
                Ok(Ok(outcome)) => Some(WireMessage::ManagedSkillContentResponse {
                    request_id,
                    skill_id: outcome.skill_id,
                    content: outcome.content,
                    etag: outcome.etag,
                }),
                Ok(Err(e)) => Some(rpc_err(request_id, e.rpc_code(), e)),
                Err(e) => Some(rpc_err(request_id, "read_managed_skill_failed", e)),
            }
        }
        WireMessage::UpsertManagedSkill {
            request_id,
            scope,
            cwd,
            slug,
            name,
            description,
            content,
            enabled,
            expected_etag,
        } => {
            let authorized_cwd = match authorize_optional_skills_cwd(&runtime.store, cwd).await {
                Ok(c) => c,
                Err(e) => return Some(rpc_err(request_id, "skill_permission_denied", e)),
            };
            let orbit_home = runtime.home_dir.clone();
            let user_home = resolve_user_home();
            let params = crate::skills_manager::UpsertSkillParams {
                scope,
                cwd: authorized_cwd,
                slug,
                name,
                description,
                content,
                enabled,
                expected_etag,
            };
            match tokio::task::spawn_blocking(move || {
                crate::skills_manager::upsert_managed_skill(&orbit_home, &user_home, params)
            })
            .await
            {
                Ok(Ok(skills)) => Some(WireMessage::ManagedSkillsSnapshot { request_id, skills }),
                Ok(Err(e)) => Some(rpc_err(request_id, e.rpc_code(), e)),
                Err(e) => Some(rpc_err(request_id, "upsert_managed_skill_failed", e)),
            }
        }
        WireMessage::ToggleManagedSkill {
            request_id,
            skill_id,
            enabled,
            cwd,
        } => {
            let authorized_cwd = match authorize_optional_skills_cwd(&runtime.store, cwd).await {
                Ok(c) => c,
                Err(e) => return Some(rpc_err(request_id, "skill_permission_denied", e)),
            };
            let orbit_home = runtime.home_dir.clone();
            let user_home = resolve_user_home();
            match tokio::task::spawn_blocking(move || {
                crate::skills_manager::toggle_managed_skill(
                    &orbit_home,
                    &user_home,
                    &skill_id,
                    enabled,
                    authorized_cwd.as_deref(),
                )
            })
            .await
            {
                Ok(Ok(skills)) => Some(WireMessage::ManagedSkillsSnapshot { request_id, skills }),
                Ok(Err(e)) => Some(rpc_err(request_id, e.rpc_code(), e)),
                Err(e) => Some(rpc_err(request_id, "toggle_managed_skill_failed", e)),
            }
        }
        WireMessage::DeleteManagedSkill {
            request_id,
            skill_id,
            cwd,
        } => {
            let authorized_cwd = match authorize_optional_skills_cwd(&runtime.store, cwd).await {
                Ok(c) => c,
                Err(e) => return Some(rpc_err(request_id, "skill_permission_denied", e)),
            };
            let orbit_home = runtime.home_dir.clone();
            let user_home = resolve_user_home();
            match tokio::task::spawn_blocking(move || {
                crate::skills_manager::delete_managed_skill(
                    &orbit_home,
                    &user_home,
                    &skill_id,
                    authorized_cwd.as_deref(),
                )
            })
            .await
            {
                Ok(Ok(skills)) => Some(WireMessage::ManagedSkillsSnapshot { request_id, skills }),
                Ok(Err(e)) => Some(rpc_err(request_id, e.rpc_code(), e)),
                Err(e) => Some(rpc_err(request_id, "delete_managed_skill_failed", e)),
            }
        }
        WireMessage::ImportManagedSkill {
            request_id,
            scope,
            cwd,
            source_markdown_or_path,
            slug_override,
        } => {
            let authorized_cwd = match authorize_optional_skills_cwd(&runtime.store, cwd).await {
                Ok(c) => c,
                Err(e) => return Some(rpc_err(request_id, "skill_permission_denied", e)),
            };
            let orbit_home = runtime.home_dir.clone();
            let user_home = resolve_user_home();
            match tokio::task::spawn_blocking(move || {
                crate::skills_manager::import_managed_skill(
                    &orbit_home,
                    &user_home,
                    scope,
                    authorized_cwd.as_deref(),
                    &source_markdown_or_path,
                    slug_override.as_deref(),
                )
            })
            .await
            {
                Ok(Ok(skills)) => Some(WireMessage::ManagedSkillsSnapshot { request_id, skills }),
                Ok(Err(e)) => Some(rpc_err(request_id, e.rpc_code(), e)),
                Err(e) => Some(rpc_err(request_id, "import_managed_skill_failed", e)),
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
            let rt = Arc::clone(runtime);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let snapshots = probe_all_provider_quotas().await;
                let samples = crate::usage::quota_snapshots_to_samples(&snapshots);
                if !samples.is_empty() {
                    let store = Arc::clone(&rt.store);
                    let _ =
                        tokio::task::spawn_blocking(move || store.record_quota_samples(&samples))
                            .await;
                }
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
        WireMessage::GetUsageInsights { request_id, days } => {
            let store = Arc::clone(&runtime.store);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || {
                    crate::usage::build_usage_insights_snapshot(&store, days, chrono::Utc::now())
                })
                .await
                {
                    Ok(Ok(snapshot)) => WireMessage::UsageInsightsResponse {
                        request_id,
                        snapshot,
                    },
                    Ok(Err(e)) => rpc_err(request_id, "usage_insights_failed", e),
                    Err(e) => rpc_err(request_id, "usage_insights_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GetContextOccupancy {
            request_id,
            agent_id,
        } => {
            let store = Arc::clone(&runtime.store);
            let orchestrator = Arc::clone(&runtime.orchestrator);
            let orbit_home = runtime.home_dir.clone();
            let user_home = resolve_user_home();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let single_call_prompt = orchestrator
                    .latest_single_call_prompt_tokens(&agent_id)
                    .await;
                let reply = match tokio::task::spawn_blocking(move || {
                    crate::usage::compute_context_occupancy(
                        &store,
                        &orbit_home,
                        &user_home,
                        &agent_id,
                        single_call_prompt,
                    )
                })
                .await
                {
                    Ok(Ok(breakdown)) => WireMessage::ContextOccupancyResponse {
                        request_id,
                        breakdown,
                    },
                    Ok(Err(e)) => rpc_err(request_id, "context_occupancy_failed", e),
                    Err(e) => rpc_err(request_id, "context_occupancy_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
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
            let keep_awake = runtime.keep_awake.clone();
            let handle = tokio::spawn(async move {
                let _keep_awake_lease =
                    keep_awake.acquire_lease(format!("side_question:{request_id}"));
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

        // Bidirectional IM Channels (Lark / Feishu, Telegram, Slack, Discord + Remote Approvals)
        WireMessage::ListImChannels { request_id } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let im = Arc::clone(&runtime.im_channels);
            tokio::spawn(async move {
                let reply = match im.list_snapshot().await {
                    Ok(snap) => WireMessage::ImChannelsSnapshot {
                        request_id,
                        channels: snap.channels,
                        bindings: snap.bindings,
                        base_webhook_url: snap.base_webhook_url,
                    },
                    Err(e) => rpc_err(request_id, "list_im_channels_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::UpsertImChannel {
            request_id,
            channel_id,
            name,
            platform,
            enabled,
            app_id,
            primary_secret,
            signing_secret,
            keep_existing_secrets,
            allowed_chat_ids,
            allowed_user_ids,
            default_workspace_cwd,
            default_provider,
            default_mode,
            forward_approvals,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let im = Arc::clone(&runtime.im_channels);
            tokio::spawn(async move {
                let spec = UpsertImChannelSpec {
                    channel_id,
                    name,
                    platform,
                    enabled,
                    app_id,
                    primary_secret,
                    signing_secret,
                    keep_existing_secrets,
                    allowed_chat_ids,
                    allowed_user_ids,
                    default_workspace_cwd,
                    default_provider,
                    default_mode,
                    forward_approvals,
                };
                let reply = match im.upsert_channel(spec).await {
                    Ok(snap) => WireMessage::ImChannelsSnapshot {
                        request_id,
                        channels: snap.channels,
                        bindings: snap.bindings,
                        base_webhook_url: snap.base_webhook_url,
                    },
                    Err(e) => rpc_err(request_id, "upsert_im_channel_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::DeleteImChannel {
            request_id,
            channel_id,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let im = Arc::clone(&runtime.im_channels);
            tokio::spawn(async move {
                let reply = match im.delete_channel(&channel_id).await {
                    Ok(snap) => WireMessage::ImChannelsSnapshot {
                        request_id,
                        channels: snap.channels,
                        bindings: snap.bindings,
                        base_webhook_url: snap.base_webhook_url,
                    },
                    Err(e) => rpc_err(request_id, "delete_im_channel_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::TestImChannel {
            request_id,
            channel_id,
            target_chat_id,
            message,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let im = Arc::clone(&runtime.im_channels);
            tokio::spawn(async move {
                let (ok, detail) = im
                    .test_channel(&channel_id, &target_chat_id, &message)
                    .await;
                let reply = WireMessage::ImChannelTestResponse {
                    request_id,
                    ok,
                    detail,
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ListPushDevices { request_id } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let push = Arc::clone(&runtime.push);
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || push.list_snapshot()).await {
                    Ok(Ok(snap)) => WireMessage::PushDevicesSnapshot {
                        request_id,
                        devices: snap.devices,
                        preferences: snap.preferences,
                    },
                    Ok(Err(e)) => rpc_err(request_id, "list_push_devices_failed", e),
                    Err(e) => rpc_err(request_id, "list_push_devices_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::RegisterPushDevice {
            request_id,
            device_id,
            platform,
            device_name,
            device_token,
            sandbox,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let push = Arc::clone(&runtime.push);
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || {
                    push.register_device(device_id, platform, device_name, device_token, sandbox)
                })
                .await
                {
                    Ok(Ok(snap)) => WireMessage::PushDevicesSnapshot {
                        request_id,
                        devices: snap.devices,
                        preferences: snap.preferences,
                    },
                    Ok(Err(e)) => rpc_err(request_id, "register_push_device_failed", e),
                    Err(e) => rpc_err(request_id, "register_push_device_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::UnregisterPushDevice {
            request_id,
            device_id,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let push = Arc::clone(&runtime.push);
            tokio::spawn(async move {
                let reply =
                    match tokio::task::spawn_blocking(move || push.unregister_device(&device_id))
                        .await
                    {
                        Ok(Ok(snap)) => WireMessage::PushDevicesSnapshot {
                            request_id,
                            devices: snap.devices,
                            preferences: snap.preferences,
                        },
                        Ok(Err(e)) => rpc_err(request_id, "unregister_push_device_failed", e),
                        Err(e) => rpc_err(request_id, "unregister_push_device_failed", e),
                    };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::UpdatePushPreferences {
            request_id,
            preferences,
            gateway_auth_token,
            keep_existing_gateway_token,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let push = Arc::clone(&runtime.push);
            tokio::spawn(async move {
                let reply = match tokio::task::spawn_blocking(move || {
                    push.update_preferences(
                        preferences,
                        gateway_auth_token,
                        keep_existing_gateway_token,
                    )
                })
                .await
                {
                    Ok(Ok(snap)) => WireMessage::PushDevicesSnapshot {
                        request_id,
                        devices: snap.devices,
                        preferences: snap.preferences,
                    },
                    Ok(Err(e)) => rpc_err(request_id, "update_push_preferences_failed", e),
                    Err(e) => rpc_err(request_id, "update_push_preferences_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::TestPushNotification {
            request_id,
            device_id,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let push = Arc::clone(&runtime.push);
            tokio::spawn(async move {
                let res = push.test_push(device_id.as_deref()).await;
                let reply = WireMessage::PushTestResponse {
                    request_id,
                    attempted: res.attempted,
                    delivered: res.delivered,
                    detail: res.detail,
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
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
        return crate::platform::resolve_user_home_dir().unwrap_or_else(|| PathBuf::from("."));
    }
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix(r"~\"))
        && let Some(home) = crate::platform::resolve_user_home_dir()
    {
        return home.join(rest);
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
        .or_else(|| std::env::var("COMPUTERNAME").ok())
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

#[allow(dead_code)]
fn ps_single_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "''"))
}

#[allow(dead_code)]
fn build_windows_self_upgrade_ps_script(
    install_path: &Path,
    custom_home: Option<&Path>,
    port: u16,
    restart_relay: bool,
) -> String {
    let exe_q = ps_single_quote(&install_path.to_string_lossy());
    let home_prefix = match custom_home {
        Some(h) => format!("'--home', {}, ", ps_single_quote(&h.to_string_lossy())),
        None => String::new(),
    };
    let port_suffix = if port > 0 {
        format!(", '--port', '{port}'")
    } else {
        String::new()
    };
    let mut script = format!(
        "Start-Sleep -Milliseconds 400; & {exe_q} @({home_prefix}'daemon', 'up'{port_suffix}) *>$null"
    );
    if restart_relay {
        script.push_str(&format!(
            "; & {exe_q} @({home_prefix}'relay', 'up') *>$null"
        ));
    }
    script
}

fn daemon_release_asset_name() -> anyhow::Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("orbit-cli-macos-arm64"),
        ("linux", "x86_64") => Ok("orbit-cli-linux-x64"),
        ("linux", "aarch64") => Ok("orbit-cli-linux-arm64"),
        ("windows", "x86_64") => Ok("orbit-windows-x86_64.exe"),
        (os, arch) => anyhow::bail!("Unsupported platform for self-upgrade: {os}/{arch}"),
    }
}

impl DaemonRuntime {
    /// Download the matching Orbit release binary, verify `--version`, atomically install to
    /// `~/.local/bin/orbit` (or `%USERPROFILE%\.local\bin\orbit.exe` on Windows), and trigger a
    /// detached `orbit daemon up` hot-restart so all `orbit shim` agent & PTY sessions stay alive.
    pub async fn self_upgrade_and_restart(
        &self,
        target_version: Option<String>,
    ) -> anyhow::Result<()> {
        use tokio::io::AsyncWriteExt;

        let _upgrade_guard = self.upgrade_lock.lock().await;

        let asset = daemon_release_asset_name()?;
        let home = crate::platform::resolve_user_home_dir()
            .ok_or_else(|| anyhow::anyhow!("Unable to resolve user home directory"))?;
        let install_dir = home.join(".local").join("bin");
        std::fs::create_dir_all(&install_dir)?;
        let exe_name = if cfg!(windows) { "orbit.exe" } else { "orbit" };
        let install_path = install_dir.join(exe_name);
        let tmp_name = if cfg!(windows) {
            format!(
                "orbit.upgrade.{}.{}.exe",
                std::process::id(),
                Uuid::new_v4().simple()
            )
        } else {
            format!(
                "orbit.upgrade.{}.{}",
                std::process::id(),
                Uuid::new_v4().simple()
            )
        };
        let tmp_path = install_dir.join(tmp_name);

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

        if let Err(e) = crate::platform::replace_executable_safely(&tmp_path, &install_path) {
            let _ = std::fs::remove_file(&tmp_path);
            anyhow::bail!("Failed to replace {}: {e}", install_path.display());
        }

        let port = *self.listen_port.read().await;
        let default_home = home.join(".orbit");
        let is_default_home = crate::platform::canonicalize_clean(&self.home_dir)
            .unwrap_or_else(|_| self.home_dir.clone())
            == crate::platform::canonicalize_clean(&default_home).unwrap_or(default_home);

        let restart_relay = detect_running_local_relay(&self.home_dir).await;
        let custom_home = (!is_default_home).then_some(self.home_dir.as_path());
        let restart_script =
            build_self_upgrade_restart_script(&install_path, custom_home, port, restart_relay);

        // Persist exact shim frame cursors before launching the hot-restart script so reattached
        // shims resume strictly after the last consumed frame with zero history replay.
        self.orchestrator.flush_all_shim_cursors().await;

        #[cfg(unix)]
        {
            let mut cmd = std::process::Command::new("sh");
            cmd.args(["-c", &restart_script])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
            cmd.spawn()
                .map_err(|e| anyhow::anyhow!("Failed to spawn daemon hot-restart: {e}"))?;
        }
        #[cfg(windows)]
        {
            let _ = restart_script;
            use std::os::windows::process::CommandExt;
            // Delay 400ms in a detached background PowerShell helper so the current RPC ack flushes
            // cleanly over WebSocket/Relay before `orbit.exe daemon up` restarts the daemon (and
            // optionally `orbit.exe relay up`), matching Unix hot-upgrade behavior.
            let ps_script = build_windows_self_upgrade_ps_script(
                &install_path,
                custom_home,
                port,
                restart_relay,
            );
            let mut cmd = std::process::Command::new("powershell.exe");
            cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &ps_script,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(
                crate::platform::WIN_CREATE_NEW_PROCESS_GROUP
                    | crate::platform::WIN_CREATE_NO_WINDOW,
            );
            cmd.spawn()
                .map_err(|e| anyhow::anyhow!("Failed to spawn Windows daemon hot-restart: {e}"))?;
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = restart_script;
        }

        tracing::info!(
            install_path = %install_path.display(),
            port,
            restart_relay,
            "Scheduled self-upgrade hot-restart via `orbit daemon up`"
        );
        Ok(())
    }
}

fn parse_relay_port_from_json(raw: &str) -> Option<u16> {
    let val: serde_json::Value = serde_json::from_str(raw).ok()?;
    let addr_str = val.get("listen_addr")?.as_str()?;
    addr_str.parse::<SocketAddr>().ok().map(|a| a.port())
}

async fn detect_running_local_relay(home_dir: &Path) -> bool {
    let relay_cfg_path = home_dir.join("relay.json");
    let Ok(raw) = std::fs::read_to_string(&relay_cfg_path) else {
        return false;
    };
    let port = parse_relay_port_from_json(&raw).unwrap_or(7878);
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
    else {
        return false;
    };
    client
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

fn build_self_upgrade_restart_script(
    install_path: &Path,
    custom_home_dir: Option<&Path>,
    port: u16,
    restart_relay: bool,
) -> String {
    let sq_bin = shell_single_quote(&install_path.to_string_lossy());
    let home_flag = custom_home_dir
        .map(|h| format!(" --home {}", shell_single_quote(&h.to_string_lossy())))
        .unwrap_or_default();
    let port_flag = if port > 0 {
        format!(" --port {port}")
    } else {
        String::new()
    };
    let relay_suffix = if restart_relay {
        format!(" && ({sq_bin}{home_flag} relay up >/dev/null 2>&1 || true)")
    } else {
        String::new()
    };
    format!("sleep 0.35 && {sq_bin}{home_flag} daemon up{port_flag}{relay_suffix}")
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

async fn authorize_optional_skills_cwd(
    store: &Arc<OrbitStore>,
    cwd: Option<String>,
) -> Result<Option<String>, String> {
    let Some(raw) = cwd else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "~" {
        return Ok(None);
    }
    let expanded = expand_host_path(trimmed);
    let canonical = authorize_workspace_mutation_cwd(store, &expanded).await?;
    Ok(Some(canonical.to_string_lossy().to_string()))
}

/// Authorize `cwd` for Path Rules RPCs:
/// - Empty or `"~"` maps to `Ok(None)` (`UserGlobal`-only `~/.orbit/rules/*.md`).
/// - Any other path MUST pass `authorize_workspace_mutation_cwd` BEFORE any filesystem access.
async fn authorize_path_rules_cwd(
    store: &Arc<OrbitStore>,
    raw_cwd: &str,
) -> Result<Option<PathBuf>, String> {
    let trimmed = raw_cwd.trim();
    if trimmed.is_empty() || trimmed == "~" {
        return Ok(None);
    }
    let resolved = expand_host_path(trimmed);
    authorize_workspace_mutation_cwd(store, &resolved)
        .await
        .map(Some)
}

/// Verify that a workspace mutation `cwd` is strictly contained within (equals or is a
/// subdirectory of) a registered workspace, agent `cwd`, synced host project directory, or
/// discovered project root on this host — preventing mutations in arbitrary host directories
/// such as `~/.ssh`, `$HOME`, `/`, or `/etc`.
pub(crate) async fn authorize_workspace_mutation_cwd(
    store: &Arc<OrbitStore>,
    resolved_cwd: &Path,
) -> Result<PathBuf, String> {
    let store_owned = Arc::clone(store);
    let cwd_owned = resolved_cwd.to_path_buf();
    tokio::task::spawn_blocking(move || {
        authorize_workspace_mutation_cwd_sync(&store_owned, &cwd_owned)
    })
    .await
    .map_err(|_| "Workspace authorization task cancelled".to_string())?
}

/// Authorize an optional External MCP workspace `cwd` against registered/discovered host
/// workspaces, rejecting raw `..` segments or arbitrary directories outside registered projects.
async fn authorize_optional_mcp_cwd(
    store: &Arc<OrbitStore>,
    cwd: Option<String>,
) -> Result<Option<PathBuf>, String> {
    let Some(raw) = cwd.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let expanded = expand_host_path(raw);
    if expanded
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Workspace path must not contain `..` traversal segments".to_string());
    }
    let authorized = authorize_workspace_mutation_cwd(store, &expanded).await?;
    Ok(Some(authorized))
}

/// Authorize an optional Lifecycle Hooks workspace `cwd` against registered/discovered host
/// workspaces (`None`, `""`, or `"~"` maps to user-global hooks only).
async fn authorize_optional_hooks_cwd(
    store: &Arc<OrbitStore>,
    cwd: Option<String>,
) -> Result<Option<PathBuf>, String> {
    let Some(raw) = cwd
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "~")
    else {
        return Ok(None);
    };
    let expanded = expand_host_path(raw);
    if expanded
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Workspace path must not contain `..` traversal segments".to_string());
    }
    let authorized = authorize_workspace_mutation_cwd(store, &expanded).await?;
    Ok(Some(authorized))
}

fn authorize_workspace_mutation_cwd_sync(
    store: &OrbitStore,
    resolved_cwd: &Path,
) -> Result<PathBuf, String> {
    let canon_cwd = canonicalize_workspace_root(resolved_cwd)?;
    let mut candidate_roots: Vec<String> = Vec::new();

    if let Ok(workspaces) = store.list_workspaces() {
        for ws in workspaces {
            candidate_roots.push(ws.cwd);
            if let Some(root) = ws.project_root {
                candidate_roots.push(root);
            }
            if let Some(src) = ws.source_cwd {
                candidate_roots.push(src);
            }
        }
    }
    if let Ok(agents) = store.list_agents(None) {
        for ag in agents {
            candidate_roots.push(ag.cwd);
        }
    }
    if let Ok(sync_state) = store.get_host_sync_state() {
        for proj in sync_state.projects {
            candidate_roots.extend(proj.directories);
        }
    }
    for dp in crate::discovery::discover_host_projects(store) {
        candidate_roots.push(dp.path);
    }

    for raw_root in candidate_roots {
        let trimmed = raw_root.trim();
        if trimmed.is_empty() {
            continue;
        }
        let expanded = expand_host_path(trimmed);
        // `canonicalize_workspace_root` automatically filters out `/`, `$HOME`, `.git`, and missing dirs.
        if let Ok(canon_root) = canonicalize_workspace_root(&expanded)
            && (canon_cwd == canon_root || canon_cwd.starts_with(&canon_root))
        {
            return Ok(canon_cwd);
        }
    }

    Err(format!(
        "Workspace `{}` is not a registered or discovered project directory on this host",
        canon_cwd.display()
    ))
}

#[cfg(test)]
mod self_upgrade_tests {
    use super::*;

    #[test]
    fn authorize_workspace_mutation_cwd_allows_registered_and_rejects_unregistered() {
        let root = std::env::temp_dir().join(format!("orbit_auth_cwd_{}", Uuid::new_v4()));
        let registered = root.join("proj_a");
        let nested = registered.join("packages/web");
        let unregistered = root.join("unregistered_secret");
        std::fs::create_dir_all(&nested).expect("mkdir nested");
        std::fs::create_dir_all(&unregistered).expect("mkdir unregistered");
        std::fs::create_dir_all(registered.join(".git")).expect("mkdir .git");

        let db_path = root.join("orbit.db");
        let store = OrbitStore::open(&db_path).expect("open store");
        store
            .upsert_host_project(&orbit_protocol::HostProjectEntry {
                project_id: "proj_a".into(),
                name: "Project A".into(),
                directories: vec![registered.to_string_lossy().to_string()],
                sort_order: 0,
                hidden: false,
                updated_at_ms: 1,
            })
            .expect("upsert project");

        assert!(authorize_workspace_mutation_cwd_sync(&store, &registered).is_ok());
        assert!(authorize_workspace_mutation_cwd_sync(&store, &nested).is_ok());
        assert!(authorize_workspace_mutation_cwd_sync(&store, &unregistered).is_err());
        assert!(authorize_workspace_mutation_cwd_sync(&store, &registered.join(".git")).is_err());
        assert!(authorize_workspace_mutation_cwd_sync(&store, Path::new("/")).is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn search_workspace_symbols_authorization_denies_unregistered_and_allows_registered() {
        let root = std::env::temp_dir().join(format!("orbit_sym_auth_{}", Uuid::new_v4()));
        let registered = root.join("repo_ok");
        let unregistered = root.join("repo_unregistered");
        std::fs::create_dir_all(&registered).expect("mkdir registered");
        std::fs::create_dir_all(&unregistered).expect("mkdir unregistered");
        std::fs::write(registered.join("lib.rs"), "pub fn authorized_symbol() {}\n")
            .expect("write lib.rs");
        std::fs::write(
            unregistered.join("secret.rs"),
            "pub fn forbidden_symbol() {}\n",
        )
        .expect("write secret.rs");

        let store = Arc::new(OrbitStore::open(root.join("orbit.db")).expect("open store"));
        store
            .upsert_host_project(&orbit_protocol::HostProjectEntry {
                project_id: "repo_ok".into(),
                name: "Repo OK".into(),
                directories: vec![registered.to_string_lossy().to_string()],
                sort_order: 0,
                hidden: false,
                updated_at_ms: 1,
            })
            .expect("upsert project");

        let index = WorkspaceSymbolIndex::new();
        assert!(
            authorize_workspace_mutation_cwd(&store, &unregistered)
                .await
                .is_err()
        );
        let auth_dir = authorize_workspace_mutation_cwd(&store, &registered)
            .await
            .expect("registered project authorized");
        let res = index
            .search(&auth_dir, "authorized", 10)
            .await
            .expect("search authorized dir");
        assert_eq!(res.symbols.len(), 1);
        assert_eq!(res.symbols[0].name, "authorized_symbol");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn authorize_optional_mcp_cwd_allows_none_and_registered_and_rejects_unregistered() {
        let root = std::env::temp_dir().join(format!("orbit_mcp_auth_cwd_{}", Uuid::new_v4()));
        let registered = root.join("proj_mcp");
        let unregistered = root.join("outside_mcp");
        std::fs::create_dir_all(&registered).expect("mkdir registered");
        std::fs::create_dir_all(&unregistered).expect("mkdir unregistered");

        let store = Arc::new(OrbitStore::open(root.join("orbit.db")).expect("open store"));
        store
            .upsert_host_project(&orbit_protocol::HostProjectEntry {
                project_id: "proj_mcp".into(),
                name: "MCP Project".into(),
                directories: vec![registered.to_string_lossy().to_string()],
                sort_order: 0,
                hidden: false,
                updated_at_ms: 1,
            })
            .expect("upsert project");

        assert!(
            authorize_optional_mcp_cwd(&store, None)
                .await
                .expect("none")
                .is_none()
        );
        assert!(
            authorize_optional_mcp_cwd(&store, Some("   ".into()))
                .await
                .expect("blank")
                .is_none()
        );
        assert!(
            authorize_optional_mcp_cwd(&store, Some(registered.to_string_lossy().to_string()))
                .await
                .is_ok()
        );
        assert!(
            authorize_optional_mcp_cwd(&store, Some(unregistered.to_string_lossy().to_string()))
                .await
                .is_err()
        );
        let traversal = format!("{}/../proj_mcp", registered.display());
        assert!(
            authorize_optional_mcp_cwd(&store, Some(traversal))
                .await
                .is_err()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn authorize_path_rules_cwd_maps_global_and_rejects_unregistered_dirs() {
        let root = std::env::temp_dir().join(format!(
            "orbit-auth-rules-cwd-test-{}",
            Uuid::new_v4().simple()
        ));
        let registered = root.join("registered");
        let unregistered = root.join("unregistered");
        std::fs::create_dir_all(&registered).expect("mkdir registered");
        std::fs::create_dir_all(&unregistered).expect("mkdir unregistered");
        let store = Arc::new(OrbitStore::open(root.join("orbit.db")).expect("open store"));
        store
            .upsert_host_project(&orbit_protocol::HostProjectEntry {
                project_id: "proj_r".into(),
                name: "Project R".into(),
                directories: vec![registered.to_string_lossy().to_string()],
                sort_order: 0,
                hidden: false,
                updated_at_ms: 1,
            })
            .expect("upsert project");

        assert_eq!(authorize_path_rules_cwd(&store, "").await, Ok(None));
        assert_eq!(authorize_path_rules_cwd(&store, "~").await, Ok(None));
        assert!(
            authorize_path_rules_cwd(&store, &registered.to_string_lossy())
                .await
                .expect("registered cwd allowed")
                .is_some()
        );
        assert!(
            authorize_path_rules_cwd(&store, &unregistered.to_string_lossy())
                .await
                .is_err()
        );
        assert!(authorize_path_rules_cwd(&store, "/").await.is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_relay_port_from_json_reads_listen_addr() {
        assert_eq!(
            parse_relay_port_from_json(r#"{"listen_addr":"0.0.0.0:7878","allowed_pubkeys":[]}"#),
            Some(7878)
        );
        assert_eq!(
            parse_relay_port_from_json(r#"{"listen_addr":"127.0.0.1:9001"}"#),
            Some(9001)
        );
        assert_eq!(parse_relay_port_from_json("not json"), None);
    }

    #[test]
    fn build_self_upgrade_restart_script_includes_relay_only_when_running() {
        let bin = Path::new("/home/u/.local/bin/orbit");
        let without_relay = build_self_upgrade_restart_script(bin, None, 7474, false);
        assert_eq!(
            without_relay,
            "sleep 0.35 && '/home/u/.local/bin/orbit' daemon up --port 7474"
        );
        let with_relay =
            build_self_upgrade_restart_script(bin, Some(Path::new("/tmp/o home")), 7474, true);
        assert!(with_relay.contains("--home '/tmp/o home' daemon up --port 7474"));
        assert!(with_relay.contains("relay up >/dev/null 2>&1 || true"));
    }

    #[tokio::test]
    async fn websocket_48mib_frame_limit_roundtrips_18mib_binary_payload() {
        use axum::{Router, extract::ws::WebSocketUpgrade, routing::get};
        use futures_util::{SinkExt, StreamExt};
        use orbit_relay_transport::RelayConnector;

        let app = Router::new().route(
            "/ws",
            get(|ws: WebSocketUpgrade| async move {
                ws.max_frame_size(orbit_protocol::MAX_WS_FRAME_BYTES)
                    .max_message_size(orbit_protocol::MAX_WS_FRAME_BYTES)
                    .on_upgrade(|mut socket| async move {
                        if let Some(Ok(axum::extract::ws::Message::Binary(data))) =
                            socket.recv().await
                        {
                            let _ = socket.send(axum::extract::ws::Message::Binary(data)).await;
                        }
                    })
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let connector = RelayConnector::connect(&format!("ws://127.0.0.1:{}", addr.port()))
            .await
            .expect("connect");
        let mut client_ws = connector.open_ws("/ws").await.expect("connect ws");

        // 18 MiB exceeds the default 16 MiB Axum/tungstenite frame cap and must round-trip cleanly.
        let payload_len = 18 * 1024 * 1024;
        let payload = vec![0x5Au8; payload_len];
        client_ws
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                payload.into(),
            ))
            .await
            .expect("send 18 MiB frame");

        let reply = client_ws
            .next()
            .await
            .expect("echo frame present")
            .expect("echo frame ok");
        match reply {
            tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
                assert_eq!(bytes.len(), payload_len);
                assert_eq!(bytes[0], 0x5A);
                assert_eq!(bytes[payload_len - 1], 0x5A);
            }
            other => panic!("expected binary echo frame, got {other:?}"),
        }

        server.abort();
    }

    #[test]
    fn concise_wire_decode_error_strips_enum_variant_dump_and_caps_at_240_chars() {
        let giant_variants: String = (0..100)
            .map(|i| format!("`variant_name_{i:03}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let raw_err = format!(
            "unknown variant `FutureClientFeature`, expected one of {giant_variants} at line 1 column 28"
        );
        assert!(raw_err.len() > 2000);
        let summary = format_concise_wire_decode_error(&raw_err);
        assert!(
            summary.len() <= 240,
            "summary must be <= 240 bytes, got {}",
            summary.len()
        );
        assert!(summary.contains("FutureClientFeature"));
        assert!(!summary.contains("variant_name_050"));

        let long_generic = "解码错误: ".to_string() + &"a".repeat(500);
        let truncated = format_concise_wire_decode_error(&long_generic);
        assert!(truncated.len() <= 240);
        assert!(truncated.ends_with('…'));
    }

    #[tokio::test]
    async fn authorize_optional_skills_cwd_enforces_workspace_boundary() {
        let base =
            std::env::temp_dir().join(format!("orbit_skills_cwd_auth_{}", Uuid::new_v4().simple()));
        let registered = base.join("registered_repo");
        let unregistered = base.join("unregistered_dir");
        std::fs::create_dir_all(&registered).expect("mkdir registered");
        std::fs::create_dir_all(&unregistered).expect("mkdir unregistered");
        let store = Arc::new(OrbitStore::open(base.join("orbit.db")).expect("open store"));
        store
            .upsert_host_project(&orbit_protocol::HostProjectEntry {
                project_id: "ws-skills-test".to_string(),
                name: "Skills Test".to_string(),
                directories: vec![registered.to_string_lossy().to_string()],
                sort_order: 0,
                hidden: false,
                updated_at_ms: 1,
            })
            .expect("insert host project");

        // None / empty / "~" map cleanly to None (user-only scope)
        assert_eq!(
            authorize_optional_skills_cwd(&store, None)
                .await
                .expect("none ok"),
            None
        );
        assert_eq!(
            authorize_optional_skills_cwd(&store, Some("  ".to_string()))
                .await
                .expect("empty ok"),
            None
        );
        assert_eq!(
            authorize_optional_skills_cwd(&store, Some("~".to_string()))
                .await
                .expect("tilde ok"),
            None
        );

        // Registered workspace directory is authorized
        let ok_cwd =
            authorize_optional_skills_cwd(&store, Some(registered.to_string_lossy().to_string()))
                .await
                .expect("registered cwd authorized");
        assert!(ok_cwd.is_some());

        // Root `/`, `$HOME`, `~/.ssh`, and unregistered directories MUST be rejected
        assert!(
            authorize_optional_skills_cwd(&store, Some("/".to_string()))
                .await
                .is_err()
        );
        assert!(
            authorize_optional_skills_cwd(&store, Some("~/.ssh".to_string()))
                .await
                .is_err()
        );
        assert!(
            authorize_optional_skills_cwd(
                &store,
                Some(unregistered.to_string_lossy().to_string()),
            )
            .await
            .is_err()
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn lsp_mcp_tools_reject_unregistered_host_cwd() {
        let home = std::env::temp_dir().join(format!("orbit-lsp-auth-home-{}", Uuid::new_v4()));
        let rogue_dir =
            std::env::temp_dir().join(format!("orbit-lsp-auth-rogue-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&home).expect("create home");
        std::fs::create_dir_all(&rogue_dir).expect("create rogue");
        std::fs::write(rogue_dir.join("main.rs"), "fn main() {}\n").expect("write rs");

        let runtime = DaemonRuntime::bootstrap(&home)
            .await
            .expect("bootstrap daemon");

        for (tool, args) in [
            (
                "lsp_diagnostics",
                serde_json::json!({ "cwd": rogue_dir.to_string_lossy(), "path": "main.rs" }),
            ),
            (
                "lsp_symbols",
                serde_json::json!({ "cwd": rogue_dir.to_string_lossy(), "query": "main" }),
            ),
            (
                "lsp_goto_definition",
                serde_json::json!({
                    "cwd": rogue_dir.to_string_lossy(),
                    "path": "main.rs",
                    "line": 1,
                    "col": 4
                }),
            ),
        ] {
            let res = crate::tools::execute_tool(&runtime, None, tool, args).await;
            assert!(
                res.is_error,
                "expected `{tool}` to reject unregistered cwd, got: {}",
                res.text
            );
            assert!(
                res.text.contains("not a registered or discovered project"),
                "unexpected error text for `{tool}`: {}",
                res.text
            );
        }

        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&rogue_dir);
    }

    #[test]
    fn windows_self_upgrade_ps_script_delays_and_handles_custom_home_and_relay() {
        let exe = Path::new(r"C:\Users\O'Connor\.local\bin\orbit.exe");
        let default_script = build_windows_self_upgrade_ps_script(exe, None, 17890, false);
        assert!(default_script.starts_with("Start-Sleep -Milliseconds 400; "));
        assert!(default_script.contains(r"'C:\Users\O''Connor\.local\bin\orbit.exe'"));
        assert!(default_script.contains("@('daemon', 'up', '--port', '17890')"));
        assert!(!default_script.contains("'relay', 'up'"));

        let custom_home = Path::new(r"C:\Orbit's Home");
        let full_script = build_windows_self_upgrade_ps_script(exe, Some(custom_home), 0, true);
        assert!(full_script.contains(r"'--home', 'C:\Orbit''s Home', 'daemon', 'up'"));
        assert!(!full_script.contains("'--port'"));
        assert!(full_script.contains(r"'--home', 'C:\Orbit''s Home', 'relay', 'up'"));
    }
}
