//! Host-local Orbit Daemon (`orbit daemon`).
//!
//! Runs on each target machine (Devbox, Linux workstation, Mac mini), manages SQLite WAL state,
//! supervises detached Shims, serves direct WebSocket clients, exposes the built-in Streamable
//! HTTP MCP orchestration server (`/mcp/agents`), runs the Cron/Heartbeat scheduler, maintains
//! Daemon-to-Daemon Connected Host links, and optionally registers outbound to Relay with full
//! `Noise_NKpsk0_25519_ChaChaPoly_SHA256` mutually authenticated end-to-end encryption.

pub mod artifacts;
pub mod attachments;
pub mod browser;
pub mod connected_hosts;
pub mod cron;
pub mod discovery;
pub mod fast_copy;
pub mod finish_subscriptions;
pub mod git;
pub mod git_collab;
pub mod git_write;
pub mod goals;
pub mod guardian;
pub mod guardian_llm;
pub mod hooks;
pub mod idle_cleanup;
pub mod im_channels;
pub mod im_longconn;
pub mod instructions;
pub mod keep_awake;
pub mod lsp;
pub mod manager;
pub mod mcp;
pub mod mcp_servers;
mod memory;
pub mod messaging;
pub mod permission_rules;
pub mod personal_router;
pub mod plan_approval;
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
pub mod workspace_watcher;
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
        ws::{
            Message as AxumWsMsg, WebSocket, WebSocketUpgrade, rejection::WebSocketUpgradeRejection,
        },
    },
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use orbit_crypto::{
    DaemonHandshake, HostIdentityKeypair, PairingOffer, RelayPopResponder, SecureChannel,
};
use orbit_protocol::{
    AgentMessageKind, HostInfo, OrchestrationConfig, PROTOCOL_VERSION, ProviderModelCatalog,
    ScheduleStatus, TerminalBinaryFrame, WireMessage,
};
use orbit_providers::{
    SideQuestionSpec, catalogs_to_model_map, discover_installed_providers,
    probe_all_provider_models, probe_all_provider_quotas, run_side_question,
};
use orbit_relay_transport::{
    MAX_RELAY_CONTROL_WS_FRAME_BYTES, RelayConnector, normalize_relay_url, redact_relay_url,
};
use orbit_store::{OrbitStore, ReceiptOutcome};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, RwLock, broadcast, mpsc};
use tokio_tungstenite::tungstenite::{
    Message as TungMsg,
    protocol::{CloseFrame as TungCloseFrame, frame::coding::CloseCode as TungCloseCode},
};
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

/// Filename inside `$ORBIT_HOME` storing the host's persisted 256-bit WS pairing secret (`0600`).
pub const PAIRING_SECRET_FILENAME: &str = "pairing-secret";
const MIN_PAIRING_SECRET_LEN: usize = 32;

/// Explicit browser/webview origins allowed to connect to local daemon `/ws` and `/mcp/agents`
/// (native Rust/CLI/mobile clients omit `Origin` and are allowed; any other `Origin` is rejected
/// with `403 Forbidden` to prevent Cross-Site WebSocket Hijacking / CSRF).
const ALLOWED_WEB_ORIGINS: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost:5173",
    "http://127.0.0.1:5173",
];

fn is_valid_pairing_secret(raw: &str) -> bool {
    raw.len() >= MIN_PAIRING_SECRET_LEN
        && !raw
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
}

pub const DAEMON_ADDR_FILENAME: &str = "daemon.addr";

#[cfg(unix)]
fn enforce_unix_permissions(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

fn write_temp_secret_file_0600(home_dir: &Path, secret: &str) -> std::io::Result<PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(home_dir)?;
    #[cfg(unix)]
    enforce_unix_permissions(home_dir, 0o700)?;
    let tmp_path = home_dir.join(format!(
        ".{PAIRING_SECRET_FILENAME}.tmp.{}",
        Uuid::new_v4().simple()
    ));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp_path)?;
    if let Err(err) = file
        .write_all(secret.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    drop(file);
    #[cfg(unix)]
    if let Err(err) = enforce_unix_permissions(&tmp_path, 0o600) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(tmp_path)
}

fn read_pairing_secret_strict(home_dir: &Path) -> anyhow::Result<String> {
    let secret_path = home_dir.join(PAIRING_SECRET_FILENAME);
    let raw = std::fs::read_to_string(&secret_path).map_err(|e| {
        anyhow::anyhow!(
            "failed to read persisted pairing secret at {}: {e}",
            secret_path.display()
        )
    })?;
    let trimmed = raw.trim();
    if is_valid_pairing_secret(trimmed) {
        Ok(trimmed.to_string())
    } else {
        anyhow::bail!(
            "persisted pairing secret at {} is malformed (fail-closed)",
            secret_path.display()
        )
    }
}

/// Read the persisted pairing secret from `$ORBIT_HOME/pairing-secret` without creating it.
pub fn read_pairing_secret(home_dir: &Path) -> Option<String> {
    let secret_path = home_dir.join(PAIRING_SECRET_FILENAME);
    if !secret_path.is_file() {
        return None;
    }
    let raw = std::fs::read_to_string(&secret_path).ok()?;
    let trimmed = raw.trim();
    if is_valid_pairing_secret(trimmed) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Load or atomically initialize the 256-bit host pairing secret (`$ORBIT_HOME/pairing-secret`, mode `0600`).
///
/// Always writes the complete secret to a `0600` temp file (`create_new` + `write_all` + `sync_all`)
/// and publishes atomically via `hard_link` so concurrent first-time boots converge on a single
/// secret with zero partial-write or world-readable window. Fails closed on corrupt/unreadable
/// files or permission-hardening failures.
pub fn load_or_create_pairing_secret(home_dir: &Path) -> anyhow::Result<String> {
    let secret_path = home_dir.join(PAIRING_SECRET_FILENAME);
    if secret_path.exists() {
        let existing = read_pairing_secret_strict(home_dir)?;
        #[cfg(unix)]
        {
            enforce_unix_permissions(home_dir, 0o700)?;
            enforce_unix_permissions(&secret_path, 0o600)?;
        }
        return Ok(existing);
    }

    let generated = orbit_crypto::generate_pairing_secret();
    let tmp_path = write_temp_secret_file_0600(home_dir, &generated)?;
    let link_res = std::fs::hard_link(&tmp_path, &secret_path);
    let _ = std::fs::remove_file(&tmp_path);
    match link_res {
        Ok(()) => Ok(generated),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            #[cfg(unix)]
            enforce_unix_permissions(&secret_path, 0o600)?;
            read_pairing_secret_strict(home_dir)
        }
        Err(err) => Err(err.into()),
    }
}

/// Generate a fresh 32-byte CSPRNG URL-safe base64 pairing secret and atomically replace
/// `$ORBIT_HOME/pairing-secret` (`0600`) via temp-file + rename (never truncating in place).
pub fn rotate_pairing_secret(home_dir: &Path) -> anyhow::Result<String> {
    let generated = orbit_crypto::generate_pairing_secret();
    let tmp_path = write_temp_secret_file_0600(home_dir, &generated)?;
    let target = home_dir.join(PAIRING_SECRET_FILENAME);
    if let Err(err) = std::fs::rename(&tmp_path, &target) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err.into());
    }
    Ok(generated)
}

/// Enforce anti-CSWSH `Origin` validation:
/// - Missing `Origin` header -> allowed (native Rust/CLI/mobile clients).
/// - Explicit Tauri/dev `Origin` in [`ALLOWED_WEB_ORIGINS`] -> allowed.
/// - Multiple `Origin` headers or any other `Origin` -> `403 Forbidden`.
pub(crate) fn validate_request_origin(headers: &HeaderMap) -> Result<(), StatusCode> {
    let mut origins = headers.get_all(axum::http::header::ORIGIN).iter();
    let Some(first) = origins.next() else {
        return Ok(());
    };
    if origins.next().is_some() {
        return Err(StatusCode::FORBIDDEN);
    }
    let Ok(raw) = first.to_str() else {
        return Err(StatusCode::FORBIDDEN);
    };
    let origin = raw.trim();
    if ALLOWED_WEB_ORIGINS
        .iter()
        .any(|allowed| origin.eq_ignore_ascii_case(allowed))
    {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Validate `Authorization: Bearer <token>` in constant time (fail-closed on empty secret or duplicate headers).
pub(crate) fn verify_bearer_token(headers: &HeaderMap, expected_secret: &str) -> bool {
    if expected_secret.is_empty() {
        return false;
    }
    let mut values = headers.get_all(axum::http::header::AUTHORIZATION).iter();
    let Some(first) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let Ok(raw) = first.to_str() else {
        return false;
    };
    let Some((scheme, rest)) = raw.trim().split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return false;
    }
    let provided = rest.trim();
    if provided.is_empty() {
        return false;
    }
    orbit_crypto::constant_time_eq(expected_secret.as_bytes(), provided.as_bytes())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonPersistedConfig {
    pub listen_addr: Option<SocketAddr>,
    pub relay_url: Option<String>,
    #[serde(default)]
    pub orchestration: OrchestrationConfig,
    #[serde(default)]
    pub memory_embedding: orbit_protocol::MemoryEmbeddingConfig,
}

fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    orbit_shim::ensure_private_dir(dir)
}

impl DaemonPersistedConfig {
    pub fn load(home_dir: &Path) -> Self {
        std::fs::read_to_string(home_dir.join("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, home_dir: &Path) -> std::io::Result<()> {
        ensure_private_dir(home_dir)?;
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
    pub pairing_secret: String,
    pub pairing_secret_rotated_tx: broadcast::Sender<()>,
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
    pub workspace_watcher: Arc<crate::workspace_watcher::WorkspaceWatcherManager>,
    pub lsp: Arc<LspManager>,
    pub browser: Arc<BrowserManager>,
    pub hooks: Arc<HooksEngine>,
    pub im_channels: Arc<ImChannelBridge>,
    pub personal_router: Arc<crate::personal_router::PersonalRouterManager>,
    pub push: Arc<PushManager>,
    provider_catalogs: Arc<RwLock<Option<Vec<ProviderModelCatalog>>>>,
    pub mcp_probe_cache: Arc<RwLock<HashMap<String, orbit_protocol::McpProbeResult>>>,
    pub mcp_config_lock: Arc<AsyncMutex<()>>,
    pub relay_url: Arc<RwLock<Option<String>>>,
    relay_task: AsyncMutex<Option<tokio::task::JoinHandle<()>>>,
    catalog_refresh_lock: AsyncMutex<()>,
    upgrade_lock: AsyncMutex<()>,
}

impl DaemonRuntime {
    fn spawn_memory_rpc<F: Future<Output = anyhow::Result<WireMessage>> + Send + 'static>(
        &self,
        request_id: Uuid,
        path: PathBuf,
        out_tx: &mpsc::Sender<SessionFrame>,
        zstd: bool,
        run: impl FnOnce(memory::MemoryEngine, String) -> F + Send + 'static,
    ) {
        let store = self.store.clone();
        let engine = self.orchestrator.memory.clone();
        let tx = out_tx.clone();
        tokio::spawn(async move {
            let msg = match authorize_workspace_mutation_cwd(&store, &path).await {
                Ok(root) => match root.to_str() {
                    Some(root) => run(engine, root.to_owned())
                        .await
                        .unwrap_or_else(|e| rpc_err(request_id, "project_memory_error", e)),
                    None => rpc_err(
                        request_id,
                        "project_memory_error",
                        "workspace path must be UTF-8",
                    ),
                },
                Err(e) => rpc_err(request_id, "workspace_denied", e),
            };
            let _ = tx.send(SessionFrame::Wire { msg, zstd }).await;
        });
    }

    /// Lightweight, side-effect-free runtime bootstrap (safe for `orbit pair`, `orbit status`,
    /// and tests). Call [`DaemonRuntime::activate_server`] after binding the TCP listener to start
    /// shim recovery, connected hosts, finish subscriptions, scheduler, and idle cleanup.
    pub async fn bootstrap(home_dir: &Path) -> anyhow::Result<Arc<Self>> {
        ensure_private_dir(home_dir)?;
        ensure_private_dir(&home_dir.join("shims"))?;
        ensure_private_dir(&home_dir.join("runtime"))?;
        orbit_providers::set_snapshot_orbit_home(home_dir);

        let keypair_path = home_dir.join("host-keypair.json");
        let keypair = if keypair_path.exists() {
            let raw = std::fs::read_to_string(&keypair_path)?;
            #[cfg(unix)]
            enforce_unix_permissions(&keypair_path, 0o600)?;
            serde_json::from_str::<HostIdentityKeypair>(&raw)?
        } else {
            let generated = HostIdentityKeypair::generate()?;
            let raw = serde_json::to_string_pretty(&generated)?;
            std::fs::write(&keypair_path, raw)?;
            #[cfg(unix)]
            enforce_unix_permissions(&keypair_path, 0o600)?;
            generated
        };

        let persisted = DaemonPersistedConfig::load(home_dir);
        let pairing_secret = load_or_create_pairing_secret(home_dir)?;
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
        orchestrator.set_message_delivery(delivery.clone()).await;

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

        let persisted_relay = persisted.relay_url.clone();
        let host_info = HostInfo {
            host_id,
            hostname: host_display_name,
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            public_key_b64: keypair.public_key_b64.clone(),
            protocol_version: PROTOCOL_VERSION,
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
            available_providers,
            relay_url: persisted_relay.clone(),
        };

        let (pairing_secret_rotated_tx, _) = broadcast::channel(16);
        let personal_router = crate::personal_router::PersonalRouterManager::new(
            home_dir.to_path_buf(),
            host_info.clone(),
            listen_port.clone(),
            mcp_token.clone(),
            store.clone(),
            orchestrator.clone(),
            connected_hosts.clone(),
        );
        orchestrator
            .set_personal_router(personal_router.clone())
            .await;
        im_channels
            .set_personal_router(personal_router.clone())
            .await;

        let symbol_index = Arc::new(WorkspaceSymbolIndex::with_external_provider(Arc::new(
            crate::lsp::LspExternalSymbolProvider::new(Arc::clone(&lsp)),
        )));
        let sym_for_watch = Arc::clone(&symbol_index);
        let orch_for_watch = Arc::clone(&orchestrator);
        let bcast_for_watch = orchestrator.broadcast_tx.clone();
        let workspace_watcher =
            crate::workspace_watcher::WorkspaceWatcherManager::new(move |batch| {
                if batch.is_primary_root_event {
                    sym_for_watch.invalidate(&batch.canonical_root);
                    let memory = orch_for_watch.memory.clone();
                    let root = batch.canonical_root.clone();
                    let paths = if batch.revalidation_paths.is_empty() {
                        batch.changed_paths.clone()
                    } else {
                        batch.revalidation_paths
                    };
                    tokio::spawn(async move {
                        if let Some(cwd) = root.to_str()
                            && let Err(error) = memory.changed(cwd, paths).await
                        {
                            tracing::warn!(%error, "project memory watcher reconciliation failed");
                        }
                    });
                }
                if batch.skills_or_rules_changed {
                    let orch_bg = Arc::clone(&orch_for_watch);
                    let ws_id = batch.workspace_id.clone();
                    tokio::spawn(async move {
                        orch_bg.invalidate_workspace_rules_cache(&ws_id).await;
                    });
                }
                let _ = bcast_for_watch.send(DaemonBroadcast::WorkspaceFilesChanged {
                    workspace_id: batch.workspace_id,
                    cwd: batch.cwd,
                    changed_paths: batch.changed_paths,
                    git_changed: batch.git_changed,
                    skills_or_rules_changed: batch.skills_or_rules_changed,
                });
            });
        orchestrator
            .set_workspace_watcher(Arc::clone(&workspace_watcher))
            .await;

        Ok(Arc::new(Self {
            host_info,
            keypair,
            store,
            orchestrator,
            attachments,
            home_dir: home_dir.to_path_buf(),
            pairing_secret,
            pairing_secret_rotated_tx,
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
            symbol_index,
            workspace_watcher,
            lsp,
            browser,
            hooks,
            im_channels,
            personal_router,
            push,
            provider_catalogs,
            mcp_probe_cache: Arc::new(RwLock::new(HashMap::new())),
            mcp_config_lock: Arc::new(AsyncMutex::new(())),
            relay_url: Arc::new(RwLock::new(persisted_relay)),
            relay_task: AsyncMutex::new(None),
            catalog_refresh_lock: AsyncMutex::new(()),
            upgrade_lock: AsyncMutex::new(()),
        }))
    }

    /// Record the active bound TCP socket address and activate all background orchestration loops.
    pub async fn activate_server(self: &Arc<Self>, bound_addr: SocketAddr) {
        *self.listen_port.write().await = bound_addr.port();
        write_live_addr_files(&self.home_dir, bound_addr);

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

        // 4a. Start OS-native debounced file watching for all active (non-archived) workspace roots
        if let Ok(active_workspaces) = crate::manager::with_store(&self.store, |s| {
            s.list_workspaces_filtered(false, true)
                .unwrap_or_default()
                .into_iter()
                .map(|w| (w.workspace_id, PathBuf::from(w.cwd)))
                .collect::<Vec<_>>()
        })
        .await
        {
            self.workspace_watcher
                .watch_active_workspaces(&active_workspaces);
        }

        // 4b. Incrementally backfill legacy daemon `timeline_events` into `timeline_events_fts`
        //     without blocking startup so full-history remote search covers pre-v6 histories.
        if !self.store.is_daemon_fts_backfill_done() {
            let store_bg = Arc::clone(&self.store);
            tokio::spawn(async move {
                const BACKFILL_BATCH_SIZE: usize = 200;
                const MAX_CONSECUTIVE_ERRORS: u32 = 5;
                let mut consecutive_errors: u32 = 0;
                loop {
                    let store_step = Arc::clone(&store_bg);
                    let step = tokio::task::spawn_blocking(move || {
                        store_step.backfill_daemon_fts_batch(BACKFILL_BATCH_SIZE)
                    })
                    .await;
                    match step {
                        Ok(Ok(progress)) => {
                            consecutive_errors = 0;
                            if progress.done {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(15)).await;
                        }
                        Ok(Err(err)) => {
                            consecutive_errors += 1;
                            warn!(
                                error = %err,
                                attempt = consecutive_errors,
                                "daemon FTS5 backfill batch failed"
                            );
                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                warn!(
                                    "aborting daemon FTS5 background backfill after repeated errors"
                                );
                                break;
                            }
                            let backoff_ms = 50u64
                                .saturating_mul(1u64 << (consecutive_errors - 1).min(4))
                                .min(1000);
                            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                        }
                        Err(join_err) => {
                            warn!(
                                error = %join_err,
                                "daemon FTS5 backfill worker task aborted"
                            );
                            break;
                        }
                    }
                }
            });
        }

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

    /// Read the currently active persisted pairing secret from `$ORBIT_HOME/pairing-secret`.
    /// Fails closed (`None`) if the file is missing, unreadable, or malformed so a deleted or
    /// rotated secret can never fall back to a stale in-memory bootstrap value.
    pub fn active_pairing_secret(&self) -> Option<String> {
        read_pairing_secret(&self.home_dir)
    }

    /// Atomically rotate `$ORBIT_HOME/pairing-secret` and immediately notify active client sessions
    /// (direct `/ws` and Relay E2EE) to terminate if their authenticated secret is no longer current.
    pub fn rotate_pairing_secret(&self) -> anyhow::Result<String> {
        let rotated = rotate_pairing_secret(&self.home_dir)?;
        let _ = self.pairing_secret_rotated_tx.send(());
        Ok(rotated)
    }

    pub fn build_pairing_offer(
        &self,
        direct_endpoints: Vec<String>,
        relay_url: Option<String>,
    ) -> anyhow::Result<PairingOffer> {
        let pairing_secret = read_pairing_secret_strict(&self.home_dir)?;
        Ok(PairingOffer {
            host_id: self.host_info.host_id.clone(),
            hostname: self.host_info.hostname.clone(),
            daemon_pubkey_b64: self.keypair.public_key_b64.clone(),
            direct_endpoints,
            relay_url,
            pairing_secret,
        })
    }

    pub async fn effective_host_info(&self) -> HostInfo {
        let mut info = self.host_info.clone();
        info.relay_url = self.relay_url.read().await.clone();
        info
    }

    /// Atomically configure or clear the daemon's outbound E2EE Relay registration,
    /// restarting the background `outbound_relay_loop` without disrupting active agents or clients.
    pub fn set_outbound_relay<'a>(
        self: &'a Arc<Self>,
        raw_relay_url: Option<String>,
        persist: bool,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + 'a>,
    > {
        Box::pin(async move {
            let normalized = match raw_relay_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(raw) => Some(normalize_relay_url(raw)?),
                None => None,
            };

            if persist {
                let mut persisted = DaemonPersistedConfig::load(&self.home_dir);
                persisted.relay_url = normalized.clone();
                persisted.save(&self.home_dir)?;
            }

            let mut task_guard = self.relay_task.lock().await;
            let current = self.relay_url.read().await.clone();
            if current == normalized
                && task_guard
                    .as_ref()
                    .is_some_and(|handle| !handle.is_finished())
            {
                return Ok(normalized);
            }

            if let Some(prev) = task_guard.take() {
                prev.abort();
            }
            *self.relay_url.write().await = normalized.clone();

            if let Some(ref relay_url) = normalized {
                let rt_relay = Arc::clone(self);
                let relay_owned = relay_url.clone();
                *task_guard = Some(tokio::spawn(async move {
                    outbound_relay_loop(rt_relay, relay_owned).await;
                }));
            }

            Ok(normalized)
        })
    }

    async fn shutdown_outbound_relay(&self) {
        if let Some(handle) = self.relay_task.lock().await.take() {
            handle.abort();
        }
    }
}

fn write_live_addr_files(home_dir: &Path, bound_addr: SocketAddr) {
    let addr_file = home_dir.join(DAEMON_ADDR_FILENAME);
    let _ = std::fs::write(&addr_file, format!("{bound_addr}\n"));
    let port_file = home_dir.join("daemon-port");
    let _ = std::fs::write(&port_file, format!("{}\n", bound_addr.port()));
    #[cfg(unix)]
    {
        let _ = enforce_unix_permissions(&addr_file, 0o600);
        let _ = enforce_unix_permissions(&port_file, 0o600);
    }
}

/// Read the active bound local Daemon `SocketAddr` from `$ORBIT_HOME/daemon.addr`.
pub fn read_live_daemon_addr(home_dir: &Path) -> Option<SocketAddr> {
    let raw = std::fs::read_to_string(home_dir.join(DAEMON_ADDR_FILENAME)).ok()?;
    let addr = raw.trim().parse::<SocketAddr>().ok()?;
    (addr.port() > 0).then_some(addr)
}

/// Read the active local Daemon port (prefers `$ORBIT_HOME/daemon.addr`, then `daemon-port`, `config.json`, or `6868`).
pub fn read_live_daemon_port(home_dir: &Path) -> u16 {
    if let Some(addr) = read_live_daemon_addr(home_dir) {
        return addr.port();
    }
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
        .route("/ws/e2ee", get(ws_e2ee_handler))
        .route("/mcp/agents", post(crate::mcp::mcp_agents_handler))
        .route(
            "/webhooks/{trigger_id}",
            post(crate::triggers::webhook_ingress_handler).layer(DefaultBodyLimit::max(
                crate::triggers::MAX_WEBHOOK_BODY_BYTES,
            )),
        )
        .merge(im_webhook_routes)
        .fallback(not_found_fallback)
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&runtime),
            service_reverse_proxy_host_middleware,
        ))
        .with_state(runtime)
}

/// Intercept requests targeting a registered `*.localhost` workspace service BEFORE daemon routes
/// (`/ws`, `/healthz`, etc.) so Webpack/Vite/Next.js HMR endpoints on `/ws` proxy cleanly.
async fn service_reverse_proxy_host_middleware(
    State(runtime): State<Arc<DaemonRuntime>>,
    req: Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    let mut host_iter = req.headers().get_all(axum::http::header::HOST).iter();
    let first_host = host_iter.next();
    if host_iter.next().is_some() {
        return (
            StatusCode::BAD_REQUEST,
            "Multiple Host headers are not allowed",
        )
            .into_response();
    }
    if let Some(host_val) = first_host.and_then(|v| v.to_str().ok())
        && let Some(auth) = runtime
            .orchestrator
            .scripts
            .resolve_proxy_service(host_val)
            .await
    {
        let listen_port = *runtime.listen_port.read().await;
        let expected_port = auth
            .advertised_port
            .or_else(|| (listen_port > 0).then_some(listen_port));
        let expected_host = auth.proxy_host.clone().unwrap_or_else(|| {
            crate::workspace_runtime::parse_http_host_header(host_val)
                .map(|(h, _)| h)
                .unwrap_or_else(|| host_val.to_ascii_lowercase())
        });
        let authority = crate::workspace_runtime::PreviewAuthority::DotLocalhost {
            host: expected_host,
            expected_port,
        };
        return runtime
            .orchestrator
            .scripts
            .proxy_http_request_authorized(&auth, &authority, req)
            .await;
    }
    next.run(req).await
}

async fn not_found_fallback() -> Response {
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
    runtime.activate_server(bound_addr).await;

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
    info!(
        host_id = %runtime.host_info.host_id,
        "Run `orbit pair` to print this host's Multi-Host pairing URI"
    );

    if let Some(relay_url) = effective_relay {
        let _ = runtime.set_outbound_relay(Some(relay_url), false).await;
    }

    let runtime_shutdown = runtime.clone();
    let app = build_http_router(runtime);
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            wait_daemon_shutdown_signal().await;
            runtime_shutdown.shutdown_outbound_relay().await;
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
    runtime.activate_server(addr).await;

    if let Some(r_url) = relay_url {
        let _ = runtime.set_outbound_relay(Some(r_url), false).await;
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

const RELAY_CONTROL_POP_TIMEOUT: Duration = Duration::from_secs(5);

fn describe_tung_frame(msg: &TungMsg) -> (&'static str, usize) {
    match msg {
        TungMsg::Text(t) => ("text", t.len()),
        TungMsg::Binary(b) => ("binary", b.len()),
        TungMsg::Ping(p) => ("ping", p.len()),
        TungMsg::Pong(p) => ("pong", p.len()),
        TungMsg::Close(frame) => ("close", frame.as_ref().map_or(0, |f| f.reason.len())),
        TungMsg::Frame(_) => ("frame", 0),
    }
}

async fn complete_relay_control_pop<S, R>(
    sink: &mut S,
    stream: &mut R,
    priv_bytes: &[u8; 32],
) -> anyhow::Result<SecureChannel>
where
    S: futures_util::Sink<TungMsg, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    R: futures_util::Stream<Item = Result<TungMsg, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let msg1 = tokio::time::timeout(RELAY_CONTROL_POP_TIMEOUT, async {
        while let Some(item) = stream.next().await {
            match item? {
                TungMsg::Binary(b) => return Ok::<_, anyhow::Error>(b),
                TungMsg::Ping(p) => {
                    sink.send(TungMsg::Pong(p)).await?;
                }
                TungMsg::Pong(_) => {}
                TungMsg::Close(frame) => {
                    let code = frame.as_ref().map(|f| u16::from(f.code));
                    anyhow::bail!(
                        "relay closed control socket before PoP challenge (code={code:?})"
                    );
                }
                other => {
                    let (kind, bytes) = describe_tung_frame(&other);
                    anyhow::bail!(
                        "unexpected relay control frame before PoP challenge (kind={kind}, bytes={bytes})"
                    );
                }
            }
        }
        anyhow::bail!("relay control stream closed before PoP challenge")
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for relay PoP challenge"))??;

    let (mut control_chan, msg2) = RelayPopResponder::accept(priv_bytes, &msg1)?;
    tokio::time::timeout(
        RELAY_CONTROL_POP_TIMEOUT,
        sink.send(TungMsg::Binary(msg2.into())),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timed out sending relay PoP response"))??;

    tokio::time::timeout(RELAY_CONTROL_POP_TIMEOUT, async {
        while let Some(item) = stream.next().await {
            match item? {
                TungMsg::Binary(ct) => {
                    let plain = control_chan.open_record(&ct)?;
                    let v: serde_json::Value = serde_json::from_slice(&plain)?;
                    if v.get("op").and_then(|x| x.as_str()) == Some("registered") {
                        return Ok::<_, anyhow::Error>(());
                    }
                    anyhow::bail!(
                        "unexpected decrypted relay control message during PoP ack (bytes={})",
                        plain.len()
                    );
                }
                TungMsg::Ping(p) => {
                    sink.send(TungMsg::Pong(p)).await?;
                }
                TungMsg::Pong(_) => {}
                TungMsg::Close(frame) => {
                    let code = frame.as_ref().map(|f| u16::from(f.code));
                    anyhow::bail!(
                        "relay rejected daemon PoP and closed control socket (code={code:?})"
                    );
                }
                other => {
                    let (kind, bytes) = describe_tung_frame(&other);
                    anyhow::bail!(
                        "unexpected relay control frame waiting for PoP confirmation (kind={kind}, bytes={bytes})"
                    );
                }
            }
        }
        anyhow::bail!("relay control stream closed before PoP confirmation")
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for relay PoP confirmation"))??;

    Ok(control_chan)
}

async fn outbound_relay_loop(runtime: Arc<DaemonRuntime>, relay_base: String) {
    const CONTROL_PING_INTERVAL: Duration = Duration::from_secs(20);
    const CONTROL_DEADLINE: Duration = Duration::from_secs(65);

    let display_relay = redact_relay_url(&relay_base);
    let pubkey = runtime.keypair.public_key_b64.clone();
    let priv_bytes = match runtime.keypair.private_bytes() {
        Ok(b) => zeroize::Zeroizing::new(b),
        Err(e) => {
            warn!(relay = %display_relay, error = %e, "Invalid daemon host private key; cannot register with Relay");
            return;
        }
    };
    let ctrl_path = format!("/v1/daemon/control?pubkey={pubkey}");

    loop {
        let mut backoff = Duration::from_secs(2);
        match RelayConnector::connect(&relay_base).await {
            Ok(connector) => {
                let connector = Arc::new(connector);
                match connector
                    .open_ws_with_limit(&ctrl_path, MAX_RELAY_CONTROL_WS_FRAME_BYTES)
                    .await
                {
                    Ok(ws) => {
                        let (mut sink, mut stream) = ws.split();
                        match complete_relay_control_pop(&mut sink, &mut stream, &priv_bytes).await
                        {
                            Ok(mut control_chan) => {
                                info!(
                                    relay = %display_relay,
                                    "Authenticated (PoP) and connected outbound to Orbit Relay"
                                );
                                let mut ping_interval =
                                    tokio::time::interval(CONTROL_PING_INTERVAL);
                                ping_interval.set_missed_tick_behavior(
                                    tokio::time::MissedTickBehavior::Delay,
                                );
                                ping_interval.tick().await;
                                let mut last_seen = tokio::time::Instant::now();

                                loop {
                                    tokio::select! {
                                        _ = ping_interval.tick() => {
                                            if last_seen.elapsed() > CONTROL_DEADLINE {
                                                warn!(relay = %display_relay, "Relay control heartbeat timed out, reconnecting...");
                                                break;
                                            }
                                            if sink.send(TungMsg::Ping(b"hb".to_vec().into())).await.is_err() {
                                                break;
                                            }
                                        }
                                        maybe_msg = stream.next() => {
                                            match maybe_msg {
                                                Some(Ok(TungMsg::Binary(ct))) => {
                                                    last_seen = tokio::time::Instant::now();
                                                    let Ok(plain) = control_chan.open_record(&ct) else {
                                                        warn!(
                                                            relay = %display_relay,
                                                            bytes = ct.len(),
                                                            "Failed to decrypt Relay control frame; reconnecting"
                                                        );
                                                        break;
                                                    };
                                                    if let Ok(v) =
                                                        serde_json::from_slice::<serde_json::Value>(&plain)
                                                        && v.get("op").and_then(|x| x.as_str())
                                                            == Some("open_tunnel")
                                                        && let Some(tid) =
                                                            v.get("tunnel_id").and_then(|x| x.as_str())
                                                        && let Some(secret_b64) =
                                                            v.get("attach_secret").and_then(|x| x.as_str())
                                                        && let Ok(attach_secret) =
                                                            orbit_crypto::decode_key_32(secret_b64)
                                                    {
                                                        let rt_tun = runtime.clone();
                                                        let conn_tun = Arc::clone(&connector);
                                                        let tid_owned = tid.to_string();
                                                        let secret_owned =
                                                            zeroize::Zeroizing::new(attach_secret);
                                                        tokio::spawn(async move {
                                                            let _ = serve_e2ee_relay_tunnel(
                                                                rt_tun,
                                                                &conn_tun,
                                                                &tid_owned,
                                                                secret_owned,
                                                            )
                                                            .await;
                                                        });
                                                    }
                                                }
                                                Some(Ok(TungMsg::Ping(p))) => {
                                                    last_seen = tokio::time::Instant::now();
                                                    if sink.send(TungMsg::Pong(p)).await.is_err() {
                                                        break;
                                                    }
                                                }
                                                Some(Ok(TungMsg::Close(_))) | Some(Err(_)) | None => break,
                                                Some(Ok(TungMsg::Pong(_))) => {
                                                    last_seen = tokio::time::Instant::now();
                                                }
                                                Some(Ok(other)) => {
                                                    last_seen = tokio::time::Instant::now();
                                                    let (kind, bytes) = describe_tung_frame(&other);
                                                    warn!(
                                                        relay = %display_relay,
                                                        frame_kind = kind,
                                                        bytes,
                                                        "Ignored unexpected unencrypted Relay control frame"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                                warn!(relay = %display_relay, "Disconnected from Relay, reconnecting...");
                            }
                            Err(e) => {
                                warn!(
                                    relay = %display_relay,
                                    error = %e,
                                    "Relay control Proof-of-Possession handshake failed, retrying in 2s..."
                                );
                            }
                        }
                    }
                    Err(e) => {
                        let detail = e.to_string();
                        if detail.contains("403") {
                            backoff = Duration::from_secs(10);
                            warn!(
                                relay = %display_relay,
                                pubkey = %pubkey,
                                "Relay rejected host daemon public key (HTTP 403 Forbidden); authorize it with `orbit relay allow {pubkey}`"
                            );
                        } else {
                            warn!(relay = %display_relay, error = %e, "Relay control stream failed, retrying in 2s...");
                        }
                    }
                }
            }
            Err(e) => {
                warn!(relay = %display_relay, error = %e, "Relay connection failed, retrying in 2s...");
            }
        }
        tokio::time::sleep(backoff).await;
    }
}

/// Bounded deadline for a Relay E2EE client to prove fresh session key possession by sending
/// its first authenticated encrypted `WireMessage` after receiving Noise `msg2`.
const E2EE_FIRST_RECORD_TIMEOUT: Duration = Duration::from_secs(10);

async fn close_tung_sink_bounded<S, E>(sink: &mut S, frame: Option<TungCloseFrame>)
where
    S: futures_util::Sink<TungMsg, Error = E> + Unpin,
{
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        let _ = sink.send(TungMsg::Close(frame)).await;
        let _ = sink.close().await;
    })
    .await;
}

/// Wait for the client's first encrypted `WireMessage` under a strict bounded deadline BEFORE
/// subscribing to daemon broadcasts or spawning session tasks. This blocks untrusted relays from
/// holding half-open sessions or receiving broadcasts by replaying a captured Noise `msg1`.
async fn receive_first_e2ee_wire_message<Si, St, E>(
    chan: &mut SecureChannel,
    sink: &mut Si,
    stream: &mut St,
    timeout: Duration,
) -> anyhow::Result<WireMessage>
where
    Si: futures_util::Sink<TungMsg, Error = E> + Unpin,
    St: futures_util::Stream<Item = Result<TungMsg, E>> + Unpin,
    E: Into<anyhow::Error>,
{
    let first_wire = tokio::time::timeout(timeout, async {
        while let Some(item) = stream.next().await {
            match item.map_err(Into::into)? {
                TungMsg::Binary(ct) => {
                    let plain = chan
                        .open_record(&ct)
                        .map_err(|_| anyhow::anyhow!("invalid first E2EE transport ciphertext"))?;
                    if plain.is_empty() || plain[0] != E2EE_FRAME_WIRE {
                        anyhow::bail!("unexpected first E2EE frame discriminator");
                    }
                    let wire = WireMessage::decode_zstd(&plain[1..]).map_err(|err| {
                        anyhow::anyhow!(
                            "failed to decode first E2EE wire message: {}",
                            format_concise_wire_decode_error(&err)
                        )
                    })?;
                    return Ok::<_, anyhow::Error>(wire);
                }
                TungMsg::Ping(p) => {
                    sink.send(TungMsg::Pong(p)).await.map_err(Into::into)?;
                }
                TungMsg::Pong(_) => {}
                TungMsg::Close(frame) => {
                    let code = frame.as_ref().map(|f| u16::from(f.code));
                    anyhow::bail!(
                        "tunnel closed before first authenticated E2EE frame (code={code:?})"
                    );
                }
                other => {
                    let (kind, bytes) = describe_tung_frame(&other);
                    anyhow::bail!(
                        "unexpected frame waiting for first E2EE record (kind={kind}, bytes={bytes})"
                    );
                }
            }
        }
        anyhow::bail!("tunnel closed before first authenticated E2EE frame")
    })
    .await;

    match first_wire {
        Ok(Ok(wire)) => Ok(wire),
        Ok(Err(err)) => {
            close_tung_sink_bounded(
                sink,
                Some(TungCloseFrame {
                    code: TungCloseCode::Policy,
                    reason: "unauthorized".into(),
                }),
            )
            .await;
            Err(err)
        }
        Err(_) => {
            close_tung_sink_bounded(
                sink,
                Some(TungCloseFrame {
                    code: TungCloseCode::Policy,
                    reason: "handshake timeout".into(),
                }),
            )
            .await;
            anyhow::bail!("timed out waiting for first authenticated E2EE frame")
        }
    }
}

async fn serve_e2ee_relay_tunnel(
    runtime: Arc<DaemonRuntime>,
    connector: &Arc<RelayConnector>,
    tunnel_id: &str,
    attach_secret: zeroize::Zeroizing<[u8; 32]>,
) -> anyhow::Result<()> {
    let pubkey = &runtime.keypair.public_key_b64;
    let priv_bytes = runtime.keypair.private_bytes()?;
    let tun_path = format!("/v1/daemon/tunnel?tunnel_id={tunnel_id}&pubkey={pubkey}");

    let ws = connector.open_ws(&tun_path).await?;
    let (mut sink, mut stream) = ws.split();

    // Authenticate /v1/daemon/tunnel: receive 16-byte attach_nonce from Relay and reply with
    // HMAC-SHA256(attach_secret, attach_nonce || tunnel_id) before bridging client ciphertext.
    let attach_nonce: [u8; orbit_crypto::TUNNEL_ATTACH_NONCE_LEN] =
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(item) = stream.next().await {
                match item? {
                    TungMsg::Binary(b) => {
                        let arr: [u8; orbit_crypto::TUNNEL_ATTACH_NONCE_LEN] =
                            b.as_ref().try_into().map_err(|_| {
                                anyhow::anyhow!(
                                    "invalid relay tunnel attach nonce length (bytes={})",
                                    b.len()
                                )
                            })?;
                        return Ok::<_, anyhow::Error>(arr);
                    }
                    TungMsg::Ping(p) => {
                        sink.send(TungMsg::Pong(p)).await?;
                    }
                    TungMsg::Pong(_) => {}
                    TungMsg::Close(frame) => {
                        let code = frame.as_ref().map(|f| u16::from(f.code));
                        anyhow::bail!("relay closed tunnel before attach nonce (code={code:?})");
                    }
                    other => {
                        let (kind, bytes) = describe_tung_frame(&other);
                        anyhow::bail!(
                            "unexpected frame waiting for tunnel attach nonce (kind={kind}, bytes={bytes})"
                        );
                    }
                }
            }
            anyhow::bail!("tunnel closed before attach nonce")
        })
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for tunnel attach nonce"))??;

    let attach_mac =
        orbit_crypto::compute_tunnel_attach_mac(&attach_secret, &attach_nonce, tunnel_id);
    tokio::time::timeout(
        Duration::from_secs(10),
        sink.send(TungMsg::Binary(attach_mac.to_vec().into())),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timed out sending tunnel attach MAC"))??;

    serve_e2ee_ws_transport(runtime, sink, stream, priv_bytes, tunnel_id, None).await
}

async fn serve_e2ee_ws_transport<Si, St, E>(
    runtime: Arc<DaemonRuntime>,
    mut sink: Si,
    mut stream: St,
    priv_bytes: [u8; 32],
    stream_label: &str,
    handshake_permit: Option<tokio::sync::OwnedSemaphorePermit>,
) -> anyhow::Result<()>
where
    Si: futures_util::Sink<TungMsg, Error = E> + Unpin + Send + 'static,
    St: futures_util::Stream<Item = Result<TungMsg, E>> + Unpin + Send + 'static,
    E: Into<anyhow::Error> + Send + 'static,
{
    // Fail closed before accepting client Noise handshake if persisted pairing-secret is unavailable.
    if runtime.active_pairing_secret().is_none() {
        warn!(
            stream = %stream_label,
            "Rejected E2EE stream: persisted pairing-secret is missing or malformed (fail-closed)"
        );
        close_tung_sink_bounded(
            &mut sink,
            Some(TungCloseFrame {
                code: TungCloseCode::Policy,
                reason: "unauthorized".into(),
            }),
        )
        .await;
        anyhow::bail!("persisted pairing-secret missing or malformed");
    }

    // Receive Noise NKpsk0 Message 1 from Client
    let msg1 = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(item) = stream.next().await {
            match item.map_err(Into::into)? {
                TungMsg::Binary(b) => return Ok::<_, anyhow::Error>(b),
                TungMsg::Ping(p) => {
                    sink.send(TungMsg::Pong(p)).await.map_err(Into::into)?;
                }
                TungMsg::Pong(_) => {}
                TungMsg::Close(frame) => {
                    let code = frame.as_ref().map(|f| u16::from(f.code));
                    anyhow::bail!("tunnel closed before Noise msg1 (code={code:?})");
                }
                other => {
                    let (kind, bytes) = describe_tung_frame(&other);
                    anyhow::bail!(
                        "unexpected frame waiting for Noise msg1 (kind={kind}, bytes={bytes})"
                    );
                }
            }
        }
        anyhow::bail!("tunnel closed before Noise msg1")
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for Noise msg1"))??;

    // Re-read the active persisted pairing secret right before authenticating msg1 so rotations
    // take effect immediately with zero race window.
    let Some(expected_secret) = runtime.active_pairing_secret() else {
        warn!(
            stream = %stream_label,
            "Rejected E2EE stream: persisted pairing-secret is missing or malformed (fail-closed)"
        );
        close_tung_sink_bounded(
            &mut sink,
            Some(TungCloseFrame {
                code: TungCloseCode::Policy,
                reason: "unauthorized".into(),
            }),
        )
        .await;
        anyhow::bail!("persisted pairing-secret missing or malformed");
    };

    let (chan, msg2) = match DaemonHandshake::accept(&priv_bytes, &expected_secret, &msg1) {
        Ok(accepted) => accepted,
        Err(err) => {
            warn!(
                stream = %stream_label,
                error = %err,
                "Rejected unauthorized E2EE client handshake"
            );
            close_tung_sink_bounded(
                &mut sink,
                Some(TungCloseFrame {
                    code: TungCloseCode::Policy,
                    reason: "unauthorized".into(),
                }),
            )
            .await;
            return Err(err.into());
        }
    };

    tokio::time::timeout(
        Duration::from_secs(10),
        sink.send(TungMsg::Binary(msg2.into())),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timed out sending Noise msg2"))
    .map_err(|e| anyhow::anyhow!(e))?
    .map_err(Into::into)?;

    // Require client key confirmation (first valid encrypted WireMessage) under a bounded timeout
    // BEFORE subscribing to daemon broadcasts or spawning full session tasks.
    let mut chan = chan;
    let initial_wire = tokio::select! {
        biased;
        () = wait_for_session_revocation(Arc::clone(&runtime), expected_secret.clone()) => {
            warn!(
                stream = %stream_label,
                "Closing E2EE stream before first frame: pairing_secret rotated or revoked"
            );
            close_tung_sink_bounded(
                &mut sink,
                Some(TungCloseFrame {
                    code: TungCloseCode::Policy,
                    reason: "unauthorized".into(),
                }),
            )
            .await;
            anyhow::bail!("pairing_secret rotated before first E2EE frame");
        }
        res = receive_first_e2ee_wire_message(
            &mut chan,
            &mut sink,
            &mut stream,
            E2EE_FIRST_RECORD_TIMEOUT,
        ) => {
            match res {
                Ok(wire) => wire,
                Err(err) => {
                    warn!(
                        stream = %stream_label,
                        error = %err,
                        "Rejected E2EE stream before session start"
                    );
                    return Err(err);
                }
            }
        }
    };
    drop(handshake_permit);

    let shared_chan = Arc::new(tokio::sync::Mutex::new(chan));
    let (in_tx, in_rx) = mpsc::channel::<SessionFrame>(256);
    let (out_tx, mut out_rx) = mpsc::channel::<SessionFrame>(256);

    let chan_writer = shared_chan.clone();
    let writer_task = tokio::spawn(async move {
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
        let _ = tokio::time::timeout(Duration::from_millis(250), sink.close()).await;
    });

    let chan_reader = shared_chan;
    let reader_task = tokio::spawn(async move {
        while let Some(Ok(m)) = stream.next().await {
            match m {
                TungMsg::Binary(ct) => {
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
                TungMsg::Close(_) => break,
                _ => {}
            }
        }
    });

    let exit_reason =
        run_supervised_client_session(runtime, in_rx, out_tx, expected_secret, Some(initial_wire))
            .await;
    reader_task.abort();
    match exit_reason {
        SessionExitReason::Revoked => {
            // Immediately abort writer_task to discard all queued outbound PTY/wire frames
            // and drop the tunnel without draining stale buffers.
            writer_task.abort();
        }
        SessionExitReason::Normal => {
            let writer_abort = writer_task.abort_handle();
            if tokio::time::timeout(Duration::from_millis(300), writer_task)
                .await
                .is_err()
            {
                writer_abort.abort();
            }
        }
    }
    Ok(())
}

static DIRECT_E2EE_HANDSHAKE_SEM: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(32)));

fn axum_msg_to_tung(msg: AxumWsMsg) -> TungMsg {
    match msg {
        AxumWsMsg::Text(t) => TungMsg::Text(t.as_str().into()),
        AxumWsMsg::Binary(b) => TungMsg::Binary(b),
        AxumWsMsg::Ping(p) => TungMsg::Ping(p),
        AxumWsMsg::Pong(p) => TungMsg::Pong(p),
        AxumWsMsg::Close(c) => TungMsg::Close(c.map(|f| TungCloseFrame {
            code: f.code.into(),
            reason: f.reason.as_str().into(),
        })),
    }
}

fn tung_msg_to_axum(msg: TungMsg) -> AxumWsMsg {
    match msg {
        TungMsg::Text(t) => AxumWsMsg::Text(t.as_str().into()),
        TungMsg::Binary(b) => AxumWsMsg::Binary(b),
        TungMsg::Ping(p) => AxumWsMsg::Ping(p),
        TungMsg::Pong(p) => AxumWsMsg::Pong(p),
        TungMsg::Close(c) => AxumWsMsg::Close(c.map(|f| axum::extract::ws::CloseFrame {
            code: u16::from(f.code),
            reason: f.reason.as_str().into(),
        })),
        TungMsg::Frame(_) => AxumWsMsg::Binary(Default::default()),
    }
}

/// Direct Noise_NKpsk0 E2EE WebSocket endpoint (`/ws/e2ee`) used by Peer-Bridged multi-hop
/// tunneling over LAN / SSH so intermediate bridge Daemons never see plaintext.
async fn ws_e2ee_handler(
    headers: HeaderMap,
    State(runtime): State<Arc<DaemonRuntime>>,
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    if let Err(status) = validate_request_origin(&headers) {
        warn!(
            origin = ?headers.get(axum::http::header::ORIGIN),
            "Rejected /ws/e2ee request due to disallowed Origin header"
        );
        return (status, "Forbidden Origin").into_response();
    }
    if runtime.active_pairing_secret().is_none() {
        warn!("Rejected /ws/e2ee upgrade: persisted pairing-secret is missing (fail-closed)");
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let Ok(permit) = Arc::clone(&DIRECT_E2EE_HANDSHAKE_SEM).try_acquire_owned() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Busy").into_response();
    };
    match ws {
        Ok(upgrade) => upgrade
            .max_frame_size(orbit_protocol::MAX_WS_FRAME_BYTES)
            .max_message_size(orbit_protocol::MAX_WS_FRAME_BYTES)
            .on_upgrade(move |socket| async move {
                handle_direct_e2ee_socket(socket, runtime, permit).await;
            })
            .into_response(),
        Err(rejection) => rejection.into_response(),
    }
}

async fn handle_direct_e2ee_socket(
    socket: WebSocket,
    runtime: Arc<DaemonRuntime>,
    handshake_permit: tokio::sync::OwnedSemaphorePermit,
) {
    let Ok(priv_bytes) = runtime.keypair.private_bytes() else {
        return;
    };
    let (raw_sink, raw_stream) = socket.split();
    let sink = Box::pin(futures_util::sink::unfold(
        raw_sink,
        |mut s, msg: TungMsg| async move {
            s.send(tung_msg_to_axum(msg)).await?;
            Ok::<_, axum::Error>(s)
        },
    ));
    let stream = raw_stream.map(|res| res.map(axum_msg_to_tung));
    if let Err(err) = serve_e2ee_ws_transport(
        runtime,
        sink,
        stream,
        priv_bytes,
        "direct_e2ee",
        Some(handshake_permit),
    )
    .await
    {
        tracing::debug!(error = %err, "direct /ws/e2ee session ended");
    }
}

// --- Direct Axum WebSocket Handler ---

async fn ws_handler(
    headers: HeaderMap,
    State(runtime): State<Arc<DaemonRuntime>>,
    ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Response {
    if let Err(status) = validate_request_origin(&headers) {
        warn!(
            origin = ?headers.get(axum::http::header::ORIGIN),
            "Rejected /ws request due to disallowed Origin header"
        );
        return (status, "Forbidden Origin").into_response();
    }

    let Some(expected_secret) = runtime.active_pairing_secret() else {
        warn!(
            "Rejected /ws upgrade request: persisted pairing-secret is missing or malformed (fail-closed)"
        );
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    };
    if !verify_bearer_token(&headers, &expected_secret) {
        warn!("Rejected unauthenticated /ws upgrade request");
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    match ws {
        Ok(upgrade) => upgrade
            .max_frame_size(orbit_protocol::MAX_WS_FRAME_BYTES)
            .max_message_size(orbit_protocol::MAX_WS_FRAME_BYTES)
            .on_upgrade(move |socket| handle_direct_socket(socket, runtime, expected_secret))
            .into_response(),
        Err(rejection) => rejection.into_response(),
    }
}

async fn handle_direct_socket(socket: WebSocket, runtime: Arc<DaemonRuntime>, auth_secret: String) {
    let (mut sink, mut stream) = socket.split();
    let (in_tx, in_rx) = mpsc::channel::<SessionFrame>(256);
    let (out_tx, mut out_rx) = mpsc::channel::<SessionFrame>(256);

    let writer_task = tokio::spawn(async move {
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
        let _ = sink.close().await;
    });

    let reader_task = tokio::spawn(async move {
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

    let exit_reason =
        run_supervised_client_session(runtime, in_rx, out_tx, auth_secret, None).await;
    reader_task.abort();
    match exit_reason {
        SessionExitReason::Revoked => {
            writer_task.abort();
        }
        SessionExitReason::Normal => {
            let writer_abort = writer_task.abort_handle();
            if tokio::time::timeout(Duration::from_millis(300), writer_task)
                .await
                .is_err()
            {
                writer_abort.abort();
            }
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionExitReason {
    Normal,
    Revoked,
}

const MAX_CONCURRENT_PEER_BRIDGES_PER_SESSION: usize = 16;
const MAX_CONCURRENT_PREVIEW_TUNNELS_PER_SESSION: usize = 64;
const MAX_PREVIEW_TUNNEL_FRAME_BYTES: usize = 512 * 1024;
const MAX_PREVIEW_TUNNEL_INBOUND_FRAMES: usize = 64;
const MAX_PREVIEW_TUNNEL_INBOUND_BYTES: usize = 4 * 1024 * 1024;
const MAX_SESSION_PREVIEW_TUNNEL_BYTES: usize = 32 * 1024 * 1024;

struct ActivePeerBridge {
    inbound_tx: mpsc::Sender<Vec<u8>>,
    task: tokio::task::AbortHandle,
}

/// RAII-accounted preview tunnel payload chunk. Dropping the chunk (either after writing to the
/// duplex stream or when a tunnel/session is aborted) automatically refunds both per-tunnel and
/// per-session byte budgets with zero leaks.
struct QueuedPreviewBytes {
    data: Vec<u8>,
    len: usize,
    tunnel_bytes: Arc<std::sync::atomic::AtomicUsize>,
    session_bytes: Arc<std::sync::atomic::AtomicUsize>,
}

impl Drop for QueuedPreviewBytes {
    fn drop(&mut self) {
        saturating_atomic_sub(&self.tunnel_bytes, self.len);
        saturating_atomic_sub(&self.session_bytes, self.len);
    }
}

fn try_reserve_atomic_budget(
    counter: &std::sync::atomic::AtomicUsize,
    amount: usize,
    limit: usize,
) -> bool {
    let mut cur = counter.load(std::sync::atomic::Ordering::Acquire);
    loop {
        let Some(next) = cur.checked_add(amount).filter(|n| *n <= limit) else {
            return false;
        };
        match counter.compare_exchange_weak(
            cur,
            next,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(actual) => cur = actual,
        }
    }
}

fn saturating_atomic_sub(counter: &std::sync::atomic::AtomicUsize, amount: usize) {
    let mut cur = counter.load(std::sync::atomic::Ordering::Acquire);
    loop {
        let next = cur.saturating_sub(amount);
        match counter.compare_exchange_weak(
            cur,
            next,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        ) {
            Ok(_) => return,
            Err(actual) => cur = actual,
        }
    }
}

struct ActivePreviewTunnel {
    inbound_tx: mpsc::Sender<QueuedPreviewBytes>,
    queued_bytes: Arc<std::sync::atomic::AtomicUsize>,
    task: tokio::task::AbortHandle,
}

struct AbortTaskOnDrop(tokio::task::AbortHandle);

impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Ensures terminal streaming tasks, peer bridges, preview tunnels, and side-question tasks are
/// always aborted immediately even when `run_client_session_loop` is dropped mid-await on secret revocation.
struct SessionCleanupGuard {
    slot_to_terminal: HashMap<u8, (String, tokio::task::AbortHandle)>,
    active_side_questions: Vec<tokio::task::AbortHandle>,
    active_peer_bridges: HashMap<Uuid, ActivePeerBridge>,
    active_preview_tunnels: HashMap<Uuid, ActivePreviewTunnel>,
    preview_session_queued_bytes: Arc<std::sync::atomic::AtomicUsize>,
}

impl SessionCleanupGuard {
    fn new() -> Self {
        Self {
            slot_to_terminal: HashMap::new(),
            active_side_questions: Vec::new(),
            active_peer_bridges: HashMap::new(),
            active_preview_tunnels: HashMap::new(),
            preview_session_queued_bytes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
}

impl Drop for SessionCleanupGuard {
    fn drop(&mut self) {
        for (_, (_, abort_handle)) in self.slot_to_terminal.drain() {
            abort_handle.abort();
        }
        for abort_handle in self.active_side_questions.drain(..) {
            abort_handle.abort();
        }
        for (_, bridge) in self.active_peer_bridges.drain() {
            bridge.task.abort();
        }
        for (_, tunnel) in self.active_preview_tunnels.drain() {
            tunnel.task.abort();
        }
    }
}

fn is_session_secret_still_valid(runtime: &DaemonRuntime, auth_secret: &str) -> bool {
    runtime
        .active_pairing_secret()
        .as_deref()
        .is_some_and(|cur| orbit_crypto::constant_time_eq(cur.as_bytes(), auth_secret.as_bytes()))
}

/// Independently watches for `pairing_secret` rotation/revocation without being blocked by
/// in-flight RPC handlers or outbound channel backpressure.
async fn wait_for_session_revocation(runtime: Arc<DaemonRuntime>, auth_secret: String) {
    let mut secret_rot_rx = runtime.pairing_secret_rotated_tx.subscribe();
    let mut secret_check_interval = tokio::time::interval(Duration::from_secs(1));
    secret_check_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    if !is_session_secret_still_valid(&runtime, &auth_secret) {
        return;
    }

    loop {
        tokio::select! {
            res = secret_rot_rx.recv() => {
                match res {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if !is_session_secret_still_valid(&runtime, &auth_secret) {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
            _ = secret_check_interval.tick() => {
                if !is_session_secret_still_valid(&runtime, &auth_secret) {
                    return;
                }
            }
        }
    }
}

/// Supervise `run_client_session_loop` alongside an independent revocation watcher so rotating or
/// deleting `pairing_secret` cancels in-flight RPCs, drops broadcast subscriptions, and terminates
/// the session immediately even under a blocked RPC or saturated outbound queue.
async fn run_supervised_client_session(
    runtime: Arc<DaemonRuntime>,
    in_rx: mpsc::Receiver<SessionFrame>,
    out_tx: mpsc::Sender<SessionFrame>,
    auth_secret: String,
    initial_msg: Option<WireMessage>,
) -> SessionExitReason {
    let session_id = Uuid::new_v4();
    let revoke_rt = Arc::clone(&runtime);
    let revoke_secret = auth_secret.clone();
    tokio::select! {
        biased;
        () = wait_for_session_revocation(revoke_rt, revoke_secret) => {
            warn!(session_id = %session_id, "Closing client session: pairing_secret rotated or revoked");
            SessionExitReason::Revoked
        }
        () = run_client_session_loop(runtime, session_id, in_rx, out_tx, auth_secret, initial_msg) => {
            SessionExitReason::Normal
        }
    }
}

async fn run_client_session_loop(
    runtime: Arc<DaemonRuntime>,
    session_id: Uuid,
    mut in_rx: mpsc::Receiver<SessionFrame>,
    out_tx: mpsc::Sender<SessionFrame>,
    auth_secret: String,
    initial_msg: Option<WireMessage>,
) {
    let mut use_zstd = false;
    let mut subscribed_agents: HashSet<String> = HashSet::new();
    let mut subscribed_provider_subagents: HashSet<(String, String)> = HashSet::new();
    let mut cleanup = SessionCleanupGuard::new();
    let mut bcast_rx = runtime.orchestrator.broadcast_tx.subscribe();
    let mut host_status_rx = runtime.connected_hosts.status_updates_tx.subscribe();

    if let Some(msg) = initial_msg {
        if !is_session_secret_still_valid(&runtime, &auth_secret) {
            return;
        }
        use_zstd = true;
        if let Some(reply) = Box::pin(handle_wire_rpc(
            &runtime,
            session_id,
            msg,
            &mut use_zstd,
            &mut subscribed_agents,
            &mut subscribed_provider_subagents,
            &mut cleanup.slot_to_terminal,
            &mut cleanup.active_side_questions,
            &out_tx,
        ))
        .await
            && out_tx
                .send(SessionFrame::Wire {
                    msg: reply,
                    zstd: use_zstd,
                })
                .await
                .is_err()
        {
            return;
        }
    }

    loop {
        tokio::select! {
            maybe_in = in_rx.recv() => {
                let Some(frame) = maybe_in else { break; };
                if !is_session_secret_still_valid(&runtime, &auth_secret) {
                    warn!(session_id = %session_id, "Closing client session on frame arrival: pairing_secret rotated or revoked");
                    break;
                }
                match frame {
                    SessionFrame::Terminal(tframe) => {
                        match tframe {
                            TerminalBinaryFrame::Input { slot, data } => {
                                if let Some((tid, _)) = cleanup.slot_to_terminal.get(&slot) {
                                    runtime.orchestrator.write_terminal_input(tid, data).await;
                                }
                            }
                            TerminalBinaryFrame::Resize { slot, cols, rows } => {
                                if let Some((tid, _)) = cleanup.slot_to_terminal.get(&slot) {
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
                        cleanup.active_side_questions.retain(|h| !h.is_finished());
                        cleanup
                            .active_peer_bridges
                            .retain(|_, b| !b.task.is_finished());
                        cleanup
                            .active_preview_tunnels
                            .retain(|_, t| !t.task.is_finished());
                        match msg {
                            WireMessage::PeerBridgeOpen {
                                request_id,
                                bridge_id,
                                target_host_id,
                            } => {
                                handle_peer_bridge_open(
                                    &runtime,
                                    &mut cleanup.active_peer_bridges,
                                    &out_tx,
                                    use_zstd,
                                    request_id,
                                    bridge_id,
                                    &target_host_id,
                                )
                                .await;
                            }
                            WireMessage::PeerBridgeData {
                                bridge_id,
                                payload_b64,
                            } => {
                                handle_peer_bridge_data(
                                    &mut cleanup.active_peer_bridges,
                                    bridge_id,
                                    &payload_b64,
                                )
                                .await;
                            }
                            WireMessage::PeerBridgeClosed { bridge_id, .. } => {
                                if let Some(b) = cleanup.active_peer_bridges.remove(&bridge_id) {
                                    b.task.abort();
                                }
                            }
                            WireMessage::PreviewTunnelOpen {
                                request_id,
                                tunnel_id,
                                target_port,
                                proxy_host,
                            } => {
                                handle_preview_tunnel_open(
                                    &runtime,
                                    &mut cleanup.active_preview_tunnels,
                                    &out_tx,
                                    use_zstd,
                                    request_id,
                                    tunnel_id,
                                    target_port,
                                    proxy_host.as_deref(),
                                )
                                .await;
                            }
                            WireMessage::PreviewTunnelData {
                                tunnel_id,
                                payload_b64,
                            } => {
                                handle_preview_tunnel_data(
                                    &mut cleanup.active_preview_tunnels,
                                    &cleanup.preview_session_queued_bytes,
                                    &out_tx,
                                    use_zstd,
                                    tunnel_id,
                                    &payload_b64,
                                );
                            }
                            WireMessage::PreviewTunnelClose { tunnel_id, .. } => {
                                if let Some(t) = cleanup.active_preview_tunnels.remove(&tunnel_id) {
                                    t.task.abort();
                                }
                            }
                            other => {
                                if let Some(reply) = Box::pin(handle_wire_rpc(
                                    &runtime,
                                    session_id,
                                    other,
                                    &mut use_zstd,
                                    &mut subscribed_agents,
                                    &mut subscribed_provider_subagents,
                                    &mut cleanup.slot_to_terminal,
                                    &mut cleanup.active_side_questions,
                                    &out_tx,
                                ))
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
                    DaemonBroadcast::WorkspaceFilesChanged {
                        workspace_id,
                        cwd,
                        changed_paths,
                        git_changed,
                        skills_or_rules_changed,
                    } => Some(WireMessage::WorkspaceFilesChanged {
                        workspace_id,
                        cwd,
                        changed_paths,
                        git_changed,
                        skills_or_rules_changed,
                    }),
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
                    DaemonBroadcast::FleetVaultChanged {
                        origin_session_id,
                        vault_fingerprint,
                        revision,
                    } => {
                        if origin_session_id != session_id {
                            Some(WireMessage::FleetVaultChanged {
                                vault_fingerprint,
                                revision,
                            })
                        } else {
                            None
                        }
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
}

async fn handle_peer_bridge_open(
    runtime: &Arc<DaemonRuntime>,
    bridges: &mut HashMap<Uuid, ActivePeerBridge>,
    out_tx: &mpsc::Sender<SessionFrame>,
    use_zstd: bool,
    request_id: Uuid,
    bridge_id: Uuid,
    target_host_id: &str,
) {
    if bridges.len() >= MAX_CONCURRENT_PEER_BRIDGES_PER_SESSION {
        let _ = out_tx
            .send(SessionFrame::Wire {
                msg: rpc_err(
                    request_id,
                    "peer_bridge_limit_reached",
                    "maximum concurrent peer bridges reached for this session",
                ),
                zstd: use_zstd,
            })
            .await;
        return;
    }
    if bridges.contains_key(&bridge_id) {
        let _ = out_tx
            .send(SessionFrame::Wire {
                msg: rpc_err(request_id, "peer_bridge_duplicate", "duplicate bridge_id"),
                zstd: use_zstd,
            })
            .await;
        return;
    }

    // SSRF guard: only bridge to an explicitly registered & enabled Connected Host in store.
    let target_id = target_host_id.trim().to_string();
    let row = match runtime.store.get_connected_host_by_host_id(&target_id) {
        Ok(Some(r)) if r.enabled => r,
        Ok(Some(_)) => {
            let _ = out_tx
                .send(SessionFrame::Wire {
                    msg: rpc_err(
                        request_id,
                        "peer_bridge_disabled",
                        "target connected host is disabled on bridge daemon",
                    ),
                    zstd: use_zstd,
                })
                .await;
            return;
        }
        _ => {
            let _ = out_tx
                .send(SessionFrame::Wire {
                    msg: rpc_err(
                        request_id,
                        "peer_bridge_not_found",
                        format!("target host `{target_id}` is not a registered connected host on this daemon"),
                    ),
                    zstd: use_zstd,
                })
                .await;
            return;
        }
    };

    let (inbound_tx, mut inbound_rx) = mpsc::channel::<Vec<u8>>(128);
    let tx_out = out_tx.clone();
    let handle = tokio::spawn(async move {
        use base64::Engine as _;

        let (ws, ssh_guard) = match crate::connected_hosts::connect_peer_e2ee_ws(&row).await {
            Ok(conn) => conn,
            Err(err) => {
                let _ = tx_out
                    .send(SessionFrame::Wire {
                        msg: rpc_err(request_id, "peer_bridge_connect_failed", err),
                        zstd: use_zstd,
                    })
                    .await;
                return;
            }
        };

        if tx_out
            .send(SessionFrame::Wire {
                msg: WireMessage::PeerBridgeOpened {
                    request_id,
                    bridge_id,
                },
                zstd: use_zstd,
            })
            .await
            .is_err()
        {
            return;
        }

        let _keep_ssh = ssh_guard;
        let (mut sink, mut stream) = ws.split();
        let mut close_reason: Option<String> = None;

        loop {
            tokio::select! {
                maybe_in = inbound_rx.recv() => {
                    let Some(bytes) = maybe_in else {
                        break;
                    };
                    if let Err(err) = sink.send(TungMsg::Binary(bytes.into())).await {
                        close_reason = Some(format!("upstream send failed: {err}"));
                        break;
                    }
                }
                maybe_up = stream.next() => {
                    match maybe_up {
                        Some(Ok(TungMsg::Binary(bytes))) => {
                            let payload_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                            if tx_out
                                .send(SessionFrame::Wire {
                                    msg: WireMessage::PeerBridgeData {
                                        bridge_id,
                                        payload_b64,
                                    },
                                    zstd: use_zstd,
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Some(Ok(TungMsg::Ping(p))) => {
                            let _ = sink.send(TungMsg::Pong(p)).await;
                        }
                        Some(Ok(TungMsg::Pong(_))) => {}
                        Some(Ok(TungMsg::Close(frame))) => {
                            close_reason = frame.map(|f| f.reason.to_string()).filter(|s| !s.is_empty());
                            break;
                        }
                        Some(Err(err)) => {
                            close_reason = Some(format!("upstream recv failed: {err}"));
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
            }
        }

        let _ = sink.close().await;
        let _ = tx_out
            .send(SessionFrame::Wire {
                msg: WireMessage::PeerBridgeClosed {
                    bridge_id,
                    reason: close_reason,
                },
                zstd: use_zstd,
            })
            .await;
    });

    bridges.insert(
        bridge_id,
        ActivePeerBridge {
            inbound_tx,
            task: handle.abort_handle(),
        },
    );
}

async fn handle_peer_bridge_data(
    bridges: &mut HashMap<Uuid, ActivePeerBridge>,
    bridge_id: Uuid,
    payload_b64: &str,
) {
    use base64::Engine as _;

    let Some(bridge) = bridges.get(&bridge_id) else {
        return;
    };
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(payload_b64) else {
        if let Some(removed) = bridges.remove(&bridge_id) {
            removed.task.abort();
        }
        return;
    };
    if bridge.inbound_tx.send(bytes).await.is_err()
        && let Some(removed) = bridges.remove(&bridge_id)
    {
        removed.task.abort();
    }
}

fn send_session_frame_nonblocking(out_tx: &mpsc::Sender<SessionFrame>, frame: SessionFrame) {
    match out_tx.try_send(frame) {
        Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {}
        Err(mpsc::error::TrySendError::Full(deferred)) => {
            let tx = out_tx.clone();
            tokio::spawn(async move {
                let _ = tx.send(deferred).await;
            });
        }
    }
}

async fn wait_preview_service_cancelled(cancel_rx: &mut tokio::sync::watch::Receiver<bool>) {
    if *cancel_rx.borrow() {
        return;
    }
    while cancel_rx.changed().await.is_ok() {
        if *cancel_rx.borrow() {
            return;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_preview_tunnel_open(
    runtime: &Arc<DaemonRuntime>,
    tunnels: &mut HashMap<Uuid, ActivePreviewTunnel>,
    out_tx: &mpsc::Sender<SessionFrame>,
    use_zstd: bool,
    request_id: Uuid,
    tunnel_id: Uuid,
    target_port: u16,
    proxy_host: Option<&str>,
) {
    tunnels.retain(|_, t| !t.task.is_finished());

    if tunnels.len() >= MAX_CONCURRENT_PREVIEW_TUNNELS_PER_SESSION {
        send_session_frame_nonblocking(
            out_tx,
            SessionFrame::Wire {
                msg: rpc_err(
                    request_id,
                    "preview_tunnel_limit_reached",
                    "maximum concurrent preview tunnels reached for this session",
                ),
                zstd: use_zstd,
            },
        );
        return;
    }
    if tunnels.contains_key(&tunnel_id) {
        send_session_frame_nonblocking(
            out_tx,
            SessionFrame::Wire {
                msg: rpc_err(
                    request_id,
                    "preview_tunnel_duplicate",
                    "duplicate tunnel_id",
                ),
                zstd: use_zstd,
            },
        );
        return;
    }

    // Strict SSRF allowlist: ONLY loopback ports owned by a currently running registered
    // workspace service instance on this daemon may be tunneled.
    let Some(auth) = runtime
        .orchestrator
        .scripts
        .resolve_allowed_preview_service(target_port, proxy_host)
        .await
    else {
        send_session_frame_nonblocking(
            out_tx,
            SessionFrame::Wire {
                msg: rpc_err(
                    request_id,
                    "preview_tunnel_forbidden",
                    format!(
                        "preview port {target_port} is not owned by a running registered workspace service on this host"
                    ),
                ),
                zstd: use_zstd,
            },
        );
        return;
    };

    let resolved_port = auth.target_port;
    let (inbound_tx, mut inbound_rx) =
        mpsc::channel::<QueuedPreviewBytes>(MAX_PREVIEW_TUNNEL_INBOUND_FRAMES);
    let queued_bytes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let scripts = Arc::clone(&runtime.orchestrator.scripts);
    let tx_out = out_tx.clone();

    let handle = tokio::spawn(async move {
        use base64::Engine as _;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        if tx_out
            .send(SessionFrame::Wire {
                msg: WireMessage::PreviewTunnelOpened {
                    request_id,
                    tunnel_id,
                    target_port: resolved_port,
                },
                zstd: use_zstd,
            })
            .await
            .is_err()
        {
            return;
        }

        let mut cancel_rx = auth.cancel_receiver();
        let (bridge_io, server_io) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(scripts.serve_preview_tunnel_io(auth, server_io));
        let _server_guard = AbortTaskOnDrop(server_task.abort_handle());

        let (mut bridge_reader, mut bridge_writer) = tokio::io::split(bridge_io);

        // Separate write and read pumps so a stalled writer never starves upstream reads and
        // outbound WS backpressure never starves inbound writes.
        let mut write_task = tokio::spawn(async move {
            while let Some(chunk) = inbound_rx.recv().await {
                if let Err(err) = bridge_writer.write_all(&chunk.data).await {
                    return Err(format!("preview tunnel write failed: {err}"));
                }
                drop(chunk);
            }
            let _ = bridge_writer.shutdown().await;
            Ok::<(), String>(())
        });
        let _write_guard = AbortTaskOnDrop(write_task.abort_handle());

        let tx_out_read = tx_out.clone();
        let mut read_task = tokio::spawn(async move {
            let mut read_buf = vec![0u8; 32 * 1024];
            loop {
                match bridge_reader.read(&mut read_buf).await {
                    Ok(0) => return Ok::<(), String>(()),
                    Ok(n) => {
                        let payload_b64 =
                            base64::engine::general_purpose::STANDARD.encode(&read_buf[..n]);
                        if tx_out_read
                            .send(SessionFrame::Wire {
                                msg: WireMessage::PreviewTunnelData {
                                    tunnel_id,
                                    payload_b64,
                                },
                                zstd: use_zstd,
                            })
                            .await
                            .is_err()
                        {
                            return Err("session outbound closed".to_string());
                        }
                    }
                    Err(err) => return Err(format!("preview tunnel read failed: {err}")),
                }
            }
        });
        let _read_guard = AbortTaskOnDrop(read_task.abort_handle());

        let mut write_done = false;
        let close_reason: Option<String> = loop {
            tokio::select! {
                read_res = &mut read_task => {
                    break match read_res {
                        Ok(Ok(())) => None,
                        Ok(Err(err)) => Some(err),
                        Err(_) => Some("preview tunnel read task aborted".to_string()),
                    };
                }
                write_res = &mut write_task, if !write_done => {
                    write_done = true;
                    match write_res {
                        Ok(Ok(())) => {}
                        Ok(Err(err)) => break Some(err),
                        Err(_) => break Some("preview tunnel write task aborted".to_string()),
                    }
                }
                _ = wait_preview_service_cancelled(&mut cancel_rx) => {
                    break Some("preview service stopped".to_string());
                }
            }
        };

        drop(_write_guard);
        drop(_read_guard);
        drop(_server_guard);

        let _ = tx_out
            .send(SessionFrame::Wire {
                msg: WireMessage::PreviewTunnelClose {
                    tunnel_id,
                    reason: close_reason,
                },
                zstd: use_zstd,
            })
            .await;
    });

    tunnels.insert(
        tunnel_id,
        ActivePreviewTunnel {
            inbound_tx,
            queued_bytes,
            task: handle.abort_handle(),
        },
    );
}

/// Strictly non-blocking preview tunnel inbound dispatcher.
///
/// NEVER awaits a slow tunnel; enforces per-tunnel frame cap (`MAX_PREVIEW_TUNNEL_INBOUND_FRAMES`),
/// per-tunnel byte cap (`MAX_PREVIEW_TUNNEL_INBOUND_BYTES`), and session-wide byte cap
/// (`MAX_SESSION_PREVIEW_TUNNEL_BYTES`). Any overflow or malformed/empty frame closes only the
/// offending tunnel without stalling RPCs, terminals, or peer closes.
fn handle_preview_tunnel_data(
    tunnels: &mut HashMap<Uuid, ActivePreviewTunnel>,
    session_queued_bytes: &Arc<std::sync::atomic::AtomicUsize>,
    out_tx: &mpsc::Sender<SessionFrame>,
    use_zstd: bool,
    tunnel_id: Uuid,
    payload_b64: &str,
) {
    use base64::Engine as _;

    let close_tunnel = |tunnels: &mut HashMap<Uuid, ActivePreviewTunnel>, reason: &str| {
        if let Some(removed) = tunnels.remove(&tunnel_id) {
            removed.task.abort();
        }
        send_session_frame_nonblocking(
            out_tx,
            SessionFrame::Wire {
                msg: WireMessage::PreviewTunnelClose {
                    tunnel_id,
                    reason: Some(reason.to_string()),
                },
                zstd: use_zstd,
            },
        );
    };

    if !tunnels.contains_key(&tunnel_id) {
        return;
    }
    if payload_b64.is_empty() {
        close_tunnel(tunnels, "empty preview tunnel frame");
        return;
    }
    if payload_b64.len() > (MAX_PREVIEW_TUNNEL_FRAME_BYTES / 3 + 2) * 4 {
        close_tunnel(tunnels, "preview tunnel frame exceeds size limit");
        return;
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(payload_b64) else {
        close_tunnel(tunnels, "invalid preview tunnel base64 payload");
        return;
    };
    let len = bytes.len();
    if len == 0 {
        close_tunnel(tunnels, "empty preview tunnel frame");
        return;
    }
    if len > MAX_PREVIEW_TUNNEL_FRAME_BYTES {
        close_tunnel(tunnels, "preview tunnel frame exceeds size limit");
        return;
    }

    let send_err_reason = {
        let Some(tunnel) = tunnels.get(&tunnel_id) else {
            return;
        };
        if !try_reserve_atomic_budget(&tunnel.queued_bytes, len, MAX_PREVIEW_TUNNEL_INBOUND_BYTES) {
            Some("preview tunnel byte buffer overflow")
        } else if !try_reserve_atomic_budget(
            session_queued_bytes,
            len,
            MAX_SESSION_PREVIEW_TUNNEL_BYTES,
        ) {
            saturating_atomic_sub(&tunnel.queued_bytes, len);
            Some("session preview tunnel byte budget exceeded")
        } else {
            let chunk = QueuedPreviewBytes {
                data: bytes,
                len,
                tunnel_bytes: Arc::clone(&tunnel.queued_bytes),
                session_bytes: Arc::clone(session_queued_bytes),
            };
            match tunnel.inbound_tx.try_send(chunk) {
                Ok(()) => None,
                Err(mpsc::error::TrySendError::Full(_dropped_chunk)) => {
                    Some("preview tunnel frame queue overflow")
                }
                Err(mpsc::error::TrySendError::Closed(_dropped_chunk)) => {
                    Some("preview tunnel closed")
                }
            }
        }
    };

    if let Some(reason) = send_err_reason {
        close_tunnel(tunnels, reason);
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
                host: runtime.effective_host_info().await,
            })
        }
        WireMessage::Ping { ts_ms } => Some(WireMessage::Pong { ts_ms }),
        WireMessage::ConfigureHostRelay {
            request_id,
            relay_url,
        } => match runtime.set_outbound_relay(relay_url, true).await {
            Ok(effective) => Some(WireMessage::HostRelayConfigured {
                request_id,
                relay_url: effective,
            }),
            Err(e) => Some(rpc_err(request_id, "configure_relay_failed", e)),
        },

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
                        source_agent_id: None,
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
        WireMessage::GitStageFiles {
            request_id,
            cwd,
            paths,
            all,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(authorized_cwd) => {
                        match crate::git_write::stage_files(&authorized_cwd, &paths, all).await {
                            Ok(result) => {
                                let mut status = inspect_git_status(&authorized_cwd).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result,
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitUnstageFiles {
            request_id,
            cwd,
            paths,
            all,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(authorized_cwd) => {
                        match crate::git_write::unstage_files(&authorized_cwd, &paths, all).await {
                            Ok(result) => {
                                let mut status = inspect_git_status(&authorized_cwd).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result,
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitApplyHunk {
            request_id,
            cwd,
            patch,
            reverse,
            cached,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(authorized_cwd) => {
                        match crate::git_write::apply_hunk(&authorized_cwd, &patch, reverse, cached)
                            .await
                        {
                            Ok(result) => {
                                if !cached {
                                    symbol_index.invalidate(&authorized_cwd);
                                }
                                let mut status = inspect_git_status(&authorized_cwd).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result,
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitDiscardFiles {
            request_id,
            cwd,
            paths,
            include_untracked,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(authorized_cwd) => {
                        match crate::git_write::discard_files(
                            &authorized_cwd,
                            &paths,
                            include_untracked,
                        )
                        .await
                        {
                            Ok(result) => {
                                symbol_index.invalidate(&authorized_cwd);
                                let mut status = inspect_git_status(&authorized_cwd).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result,
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitCommit {
            request_id,
            cwd,
            message,
            amend,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => {
                        match crate::git_write::commit_changes(&repo_root, &message, amend).await {
                            Ok(commit_res) => {
                                let mut status = inspect_git_status(&repo_root).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::Committed(
                                        commit_res,
                                    ),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitPush {
            request_id,
            cwd,
            remote,
            branch,
            set_upstream,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => {
                        match crate::git_write::push_branch(
                            &repo_root,
                            remote.as_deref(),
                            branch.as_deref(),
                            set_upstream,
                        )
                        .await
                        {
                            Ok(push_res) => {
                                let mut status = inspect_git_status(&repo_root).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::Pushed(push_res),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitCreatePr {
            request_id,
            cwd,
            title,
            body,
            base,
            draft,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => {
                        match crate::git_write::create_pull_request(
                            &repo_root,
                            &title,
                            &body,
                            base.as_deref(),
                            draft,
                        )
                        .await
                        {
                            Ok(pr_res) => {
                                let mut status = inspect_git_status(&repo_root).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::PrCreated(pr_res),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitStashList { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(auth_cwd) => match crate::git_collab::git_stash_list(&auth_cwd).await {
                        Ok(res) => WireMessage::GitCollabResponse {
                            request_id,
                            cwd,
                            payload: orbit_protocol::GitCollabQueryResult::StashList(res),
                        },
                        Err(e) => rpc_err(request_id, e.code(), e),
                    },
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitStashPush {
            request_id,
            cwd,
            message,
            include_untracked,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => {
                        match crate::git_collab::git_stash_push(
                            &repo_root,
                            message.as_deref(),
                            include_untracked,
                        )
                        .await
                        {
                            Ok(res) => {
                                symbol_index.invalidate(&repo_root);
                                let mut status = inspect_git_status(&repo_root).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::StashMutated(res),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitStashApply {
            request_id,
            cwd,
            stash_ref,
            expected_oid,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => {
                        match crate::git_collab::git_stash_apply(
                            &repo_root,
                            &stash_ref,
                            expected_oid.as_deref(),
                        )
                        .await
                        {
                            Ok(res) => {
                                symbol_index.invalidate(&repo_root);
                                let mut status = inspect_git_status(&repo_root).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::StashMutated(res),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitStashPop {
            request_id,
            cwd,
            stash_ref,
            expected_oid,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => {
                        match crate::git_collab::git_stash_pop(
                            &repo_root,
                            &stash_ref,
                            expected_oid.as_deref(),
                        )
                        .await
                        {
                            Ok(res) => {
                                symbol_index.invalidate(&repo_root);
                                let mut status = inspect_git_status(&repo_root).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::StashMutated(res),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitStashDrop {
            request_id,
            cwd,
            stash_ref,
            expected_oid,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => {
                        match crate::git_collab::git_stash_drop(
                            &repo_root,
                            &stash_ref,
                            expected_oid.as_deref(),
                        )
                        .await
                        {
                            Ok(res) => {
                                let mut status = inspect_git_status(&repo_root).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::StashMutated(res),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitRebaseStatus { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(auth_cwd) => match crate::git_collab::git_rebase_status(&auth_cwd).await {
                        Ok(res) => WireMessage::GitCollabResponse {
                            request_id,
                            cwd,
                            payload: orbit_protocol::GitCollabQueryResult::RebaseStatus(res),
                        },
                        Err(e) => rpc_err(request_id, e.code(), e),
                    },
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitRebaseStart {
            request_id,
            cwd,
            onto_ref,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => {
                        match crate::git_collab::git_rebase_start(&repo_root, &onto_ref).await {
                            Ok(res) => {
                                symbol_index.invalidate(&repo_root);
                                let mut status = inspect_git_status(&repo_root).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::RebaseAction(res),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitRebaseContinue { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => match crate::git_collab::git_rebase_continue(&repo_root).await
                    {
                        Ok(res) => {
                            symbol_index.invalidate(&repo_root);
                            let mut status = inspect_git_status(&repo_root).await;
                            status.cwd = cwd.clone();
                            WireMessage::GitMutationResult {
                                request_id,
                                cwd,
                                result: orbit_protocol::GitMutationOutcome::RebaseAction(res),
                                status: Some(status),
                            }
                        }
                        Err(e) => rpc_err(request_id, e.code(), e),
                    },
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitRebaseAbort { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => match crate::git_collab::git_rebase_abort(&repo_root).await {
                        Ok(res) => {
                            symbol_index.invalidate(&repo_root);
                            let mut status = inspect_git_status(&repo_root).await;
                            status.cwd = cwd.clone();
                            WireMessage::GitMutationResult {
                                request_id,
                                cwd,
                                result: orbit_protocol::GitMutationOutcome::RebaseAction(res),
                                status: Some(status),
                            }
                        }
                        Err(e) => rpc_err(request_id, e.code(), e),
                    },
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitRebaseSkip {
            request_id,
            cwd,
            expected_step,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_git_repo_root_mutation(&store, &resolved).await {
                    Ok(repo_root) => {
                        match crate::git_collab::git_rebase_skip(&repo_root, expected_step).await {
                            Ok(res) => {
                                symbol_index.invalidate(&repo_root);
                                let mut status = inspect_git_status(&repo_root).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::RebaseAction(res),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err((msg, code)) => rpc_err(request_id, code, msg),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitListConflicts { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(auth_cwd) => match crate::git_collab::git_list_conflicts(&auth_cwd).await {
                        Ok(res) => WireMessage::GitCollabResponse {
                            request_id,
                            cwd,
                            payload: orbit_protocol::GitCollabQueryResult::ConflictList(res),
                        },
                        Err(e) => rpc_err(request_id, e.code(), e),
                    },
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitResolveConflict {
            request_id,
            cwd,
            path,
            block_index,
            resolution,
            auto_stage,
            expected_sha256,
        } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            let symbol_index = runtime.symbol_index.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(auth_cwd) => {
                        match crate::git_collab::git_resolve_conflict(
                            &auth_cwd,
                            &path,
                            block_index,
                            &resolution,
                            auto_stage,
                            expected_sha256.as_deref(),
                        )
                        .await
                        {
                            Ok(res) => {
                                symbol_index.invalidate(&auth_cwd);
                                let mut status = inspect_git_status(&auth_cwd).await;
                                status.cwd = cwd.clone();
                                WireMessage::GitMutationResult {
                                    request_id,
                                    cwd,
                                    result: orbit_protocol::GitMutationOutcome::ConflictResolved(
                                        res,
                                    ),
                                    status: Some(status),
                                }
                            }
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitPrStatus { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(auth_cwd) => match crate::git_collab::git_pr_status(&auth_cwd).await {
                        Ok(res) => WireMessage::GitCollabResponse {
                            request_id,
                            cwd,
                            payload: orbit_protocol::GitCollabQueryResult::PrStatus(res),
                        },
                        Err(e) => rpc_err(request_id, e.code(), e),
                    },
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::GitGenerateCommitMessage { request_id, cwd } => {
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let resolved = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved).await {
                    Ok(auth_cwd) => {
                        match crate::git_collab::git_generate_commit_message(&auth_cwd).await {
                            Ok(res) => WireMessage::GitCollabResponse {
                                request_id,
                                cwd,
                                payload: orbit_protocol::GitCollabQueryResult::CommitMessage(res),
                            },
                            Err(e) => rpc_err(request_id, e.code(), e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "git_mutation_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ListProjectMemories {
            request_id,
            cwd,
            status,
            category,
            query,
            limit,
        } => {
            runtime.spawn_memory_rpc(
                request_id,
                expand_host_path(&cwd),
                out_tx,
                *use_zstd,
                move |engine, root| async move {
                    let items = engine.list(&root, status, category, query, limit).await?;
                    Ok(WireMessage::ProjectMemoriesList {
                        request_id,
                        cwd,
                        items,
                    })
                },
            );
            None
        }
        WireMessage::UpsertProjectMemory {
            request_id,
            cwd,
            id,
            title,
            content,
            category,
            status,
            file_paths,
            tags,
        } => {
            runtime.spawn_memory_rpc(
                request_id,
                expand_host_path(&cwd),
                out_tx,
                *use_zstd,
                move |engine, root| async move {
                    let item = engine
                        .save(
                            &root,
                            memory::MemoryDraft {
                                id,
                                title,
                                content,
                                category,
                                status,
                                file_paths,
                                tags,
                            },
                            None,
                        )
                        .await?;
                    Ok(WireMessage::ProjectMemorySaved { request_id, item })
                },
            );
            None
        }
        WireMessage::ConfirmProjectMemory {
            request_id,
            cwd,
            id,
        } => {
            runtime.spawn_memory_rpc(
                request_id,
                expand_host_path(&cwd),
                out_tx,
                *use_zstd,
                move |engine, root| async move {
                    let item = engine.confirm(&root, id).await?;
                    Ok(WireMessage::ProjectMemorySaved { request_id, item })
                },
            );
            None
        }
        WireMessage::DeleteProjectMemory {
            request_id,
            cwd,
            id,
        } => {
            runtime.spawn_memory_rpc(
                request_id,
                expand_host_path(&cwd),
                out_tx,
                *use_zstd,
                move |engine, root| async move {
                    let deleted = engine.forget(&root, id.clone()).await?;
                    Ok(WireMessage::ProjectMemoryDeleted {
                        request_id,
                        id,
                        deleted,
                    })
                },
            );
            None
        }
        WireMessage::RecallProjectMemories {
            request_id,
            cwd,
            query,
            limit,
            include_unconfirmed,
        } => {
            runtime.spawn_memory_rpc(
                request_id,
                expand_host_path(&cwd),
                out_tx,
                *use_zstd,
                move |engine, root| async move {
                    let hits = engine
                        .recall(&root, query, limit, include_unconfirmed.unwrap_or(false))
                        .await?;
                    Ok(WireMessage::ProjectMemoryRecallResult {
                        request_id,
                        cwd,
                        hits,
                    })
                },
            );
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
        WireMessage::SearchMessages {
            request_id,
            query,
            agent_id,
            limit,
        } => {
            let trimmed_query = query.trim().to_string();
            let scoped_agent_id = agent_id
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let effective_limit = match limit {
                Some(0) => 0usize,
                Some(n) => (n as usize).min(100),
                None => {
                    if scoped_agent_id.is_some() {
                        100
                    } else {
                        20
                    }
                }
            };
            if trimmed_query.is_empty() || effective_limit == 0 {
                return Some(WireMessage::SearchMessagesResult {
                    request_id,
                    hits: Vec::new(),
                });
            }

            let tx = out_tx.clone();
            let zstd = *use_zstd;
            let store = runtime.store.clone();
            tokio::spawn(async move {
                let outcome = tokio::task::spawn_blocking(move || {
                    store.search_daemon_messages_scoped(
                        &trimmed_query,
                        scoped_agent_id.as_deref(),
                        effective_limit,
                    )
                })
                .await;
                let reply = match outcome {
                    Ok(Ok(rows)) => WireMessage::SearchMessagesResult {
                        request_id,
                        hits: rows
                            .into_iter()
                            .map(|h| orbit_protocol::RemoteMessageSearchHit {
                                agent_id: h.agent_id,
                                seq: h.seq,
                                turn_id: h.turn_id,
                                role: h.role,
                                snippet: h.snippet,
                                timestamp: h.timestamp,
                                bm25_score: h.bm25_score,
                                agent_title: h.agent_title,
                                cwd: h.cwd,
                            })
                            .collect(),
                    },
                    Ok(Err(e)) => rpc_err(request_id, "search_messages_failed", e),
                    Err(e) => rpc_err(request_id, "search_messages_failed", e),
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
                                let _ = hooks.dispatch_event_async_with_targets(
                                    orbit_protocol::HookEventKind::PostFileEdit,
                                    Some(hook_cwd),
                                    vec![hook_path],
                                    hook_payload,
                                );
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
            preserve_queue,
        } => match runtime
            .orchestrator
            .interrupt_agent_with_options(&agent_id, preserve_queue)
            .await
        {
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
            agent_originated,
        } => {
            let res = if agent_originated {
                runtime
                    .orchestrator
                    .resolve_permission_by_agent(&agent_id, permission_request_id, decision)
                    .await
            } else {
                runtime
                    .orchestrator
                    .resolve_permission(&agent_id, permission_request_id, decision)
                    .await
            };
            match res {
                Ok(()) => Some(WireMessage::Ack { request_id }),
                Err(e) => Some(rpc_err(request_id, "resolve_permission_failed", e)),
            }
        }
        WireMessage::ResolvePlanApproval {
            request_id,
            agent_id,
            permission_request_id,
            action,
            edited_plan_markdown,
            feedback,
            target_mode,
        } => match runtime
            .orchestrator
            .resolve_plan_approval(
                &agent_id,
                permission_request_id,
                action,
                edited_plan_markdown,
                feedback,
                target_mode,
            )
            .await
        {
            Ok(()) => Some(WireMessage::Ack { request_id }),
            Err(e) => Some(rpc_err(request_id, "resolve_plan_approval_failed", e)),
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
            tail_limit,
        } => {
            if since_seq == 0
                && let Some(tail) = tail_limit.filter(|&n| n > 0)
            {
                let capped = (tail as usize).clamp(1, 2000);
                match runtime.store.fetch_timeline_tail_budgeted(
                    &agent_id,
                    capped,
                    orbit_protocol::MAX_WIRE_TIMELINE_PAGE_BYTES,
                ) {
                    Ok((events, has_older)) => {
                        subscribed_agents.insert(agent_id.clone());
                        Some(WireMessage::TimelineCatchup {
                            request_id,
                            outline_prompts: runtime
                                .store
                                .fetch_timeline_outline_prompts(&agent_id)
                                .unwrap_or_default(),
                            agent_id,
                            events,
                            has_more: false,
                            has_older_history: has_older,
                        })
                    }
                    Err(e) => Some(rpc_err(request_id, "timeline_error", e)),
                }
            } else {
                match runtime.store.fetch_timeline_since_budgeted(
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
                            outline_prompts: if since_seq == 0 || tail_limit.is_some() {
                                runtime
                                    .store
                                    .fetch_timeline_outline_prompts(&agent_id)
                                    .unwrap_or_default()
                            } else {
                                Vec::new()
                            },
                            agent_id,
                            events,
                            has_more,
                            has_older_history: false,
                        })
                    }
                    Err(e) => Some(rpc_err(request_id, "timeline_error", e)),
                }
            }
        }
        WireMessage::FetchTimelineBefore {
            request_id,
            agent_id,
            before_seq,
            limit,
        } => {
            let capped = if limit == 0 {
                300
            } else {
                (limit as usize).clamp(1, 1000)
            };
            match runtime.store.fetch_timeline_before_budgeted(
                &agent_id,
                before_seq,
                capped,
                orbit_protocol::MAX_WIRE_TIMELINE_PAGE_BYTES,
            ) {
                Ok((events, has_more_older)) => Some(WireMessage::TimelineHistoryPage {
                    request_id,
                    agent_id,
                    events,
                    has_more_older,
                }),
                Err(e) => Some(rpc_err(request_id, "timeline_history_error", e)),
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
        WireMessage::LspHover {
            request_id,
            cwd,
            path,
            line,
            col,
            content,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match lsp
                            .hover(&authorized_cwd, &path, line, col, content.as_deref())
                            .await
                        {
                            Ok(hover) => WireMessage::LspHoverResponse {
                                request_id,
                                cwd,
                                hover,
                            },
                            Err(e) => rpc_err(request_id, "lsp_hover_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::LspReferences {
            request_id,
            cwd,
            path,
            line,
            col,
            include_declaration,
            content,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match lsp
                            .references(
                                &authorized_cwd,
                                &path,
                                line,
                                col,
                                include_declaration,
                                content.as_deref(),
                            )
                            .await
                        {
                            Ok(references) => WireMessage::LspReferencesResponse {
                                request_id,
                                cwd,
                                references,
                            },
                            Err(e) => rpc_err(request_id, "lsp_references_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::LspCompletion {
            request_id,
            cwd,
            path,
            line,
            col,
            trigger_character,
            content,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match lsp
                            .completion(
                                &authorized_cwd,
                                &path,
                                line,
                                col,
                                trigger_character.as_deref(),
                                content.as_deref(),
                            )
                            .await
                        {
                            Ok(items) => WireMessage::LspCompletionResponse {
                                request_id,
                                cwd,
                                items,
                            },
                            Err(e) => rpc_err(request_id, "lsp_completion_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::LspCodeActionRequest {
            request_id,
            cwd,
            path,
            line,
            col,
            end_line,
            end_col,
            diagnostics,
            content,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match lsp
                            .code_action(
                                &authorized_cwd,
                                &path,
                                orbit_protocol::LspRange {
                                    line,
                                    col,
                                    end_line,
                                    end_col,
                                },
                                &diagnostics,
                                content.as_deref(),
                            )
                            .await
                        {
                            Ok(actions) => WireMessage::LspCodeActionResponse {
                                request_id,
                                cwd,
                                actions,
                            },
                            Err(e) => rpc_err(request_id, "lsp_code_action_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::LspRename {
            request_id,
            cwd,
            path,
            line,
            col,
            new_name,
            content,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match lsp
                            .rename(
                                &authorized_cwd,
                                &path,
                                line,
                                col,
                                &new_name,
                                content.as_deref(),
                            )
                            .await
                        {
                            Ok(result) => WireMessage::LspWorkspaceEditResponse {
                                request_id,
                                cwd,
                                result,
                            },
                            Err(e) => rpc_err(request_id, "lsp_rename_failed", e),
                        }
                    }
                    Err(e) => rpc_err(request_id, "lsp_cwd_denied", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::LspApplyWorkspaceEdit {
            request_id,
            cwd,
            edit,
        } => {
            let lsp = Arc::clone(&runtime.lsp);
            let store = runtime.store.clone();
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let resolved_cwd = expand_host_path(&cwd);
                let reply = match authorize_workspace_mutation_cwd(&store, &resolved_cwd).await {
                    Ok(authorized_cwd) => {
                        match lsp.apply_workspace_edit(&authorized_cwd, &edit).await {
                            Ok(result) => WireMessage::LspWorkspaceEditResponse {
                                request_id,
                                cwd,
                                result,
                            },
                            Err(e) => rpc_err(request_id, "lsp_apply_workspace_edit_failed", e),
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

        // Fine-Grained Persistent Permission Rules Engine & Safe Batch Approval
        WireMessage::ListPermissionRules {
            request_id,
            project_cwd,
        } => match runtime
            .orchestrator
            .list_permission_rules(project_cwd.as_deref())
        {
            Ok(rules) => Some(WireMessage::PermissionRulesSnapshot { request_id, rules }),
            Err(e) => Some(rpc_err(request_id, "list_permission_rules_failed", e)),
        },
        WireMessage::UpsertPermissionRule { request_id, rule } => {
            match runtime.orchestrator.upsert_permission_rule(rule) {
                Ok(saved) => Some(WireMessage::PermissionRuleUpdated {
                    request_id,
                    rule: saved,
                }),
                Err(e) => Some(rpc_err(request_id, "upsert_permission_rule_failed", e)),
            }
        }
        WireMessage::DeletePermissionRule {
            request_id,
            rule_id,
        } => match runtime.orchestrator.delete_permission_rule(&rule_id) {
            Ok(deleted) => Some(WireMessage::PermissionRuleDeleted {
                request_id,
                rule_id,
                deleted,
            }),
            Err(e) => Some(rpc_err(request_id, "delete_permission_rule_failed", e)),
        },
        WireMessage::ResolvePermissionsBatch {
            request_id,
            items,
            decision,
        } => match runtime
            .orchestrator
            .resolve_permissions_batch(items, decision)
            .await
        {
            Ok(result) => Some(WireMessage::PermissionsBatchResolved {
                request_id,
                resolved_count: result.resolved_count,
                skipped_count: result.skipped_count,
                skipped_reasons: result.skipped_reasons,
            }),
            Err(e) => Some(rpc_err(request_id, "resolve_permissions_batch_failed", e)),
        },

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
            workspace_root,
        } => {
            let browser = Arc::clone(&runtime.browser);
            let store = Arc::clone(&runtime.store);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                // Security: when owned by an agent, ALWAYS bind workspace_root authoritatively
                // from the agent's recorded `cwd` so callers cannot widen upload confinement.
                let resolved_root = if let Some(ref aid) = agent_id {
                    store
                        .get_agent(aid)
                        .ok()
                        .flatten()
                        .map(|a| PathBuf::from(a.cwd))
                } else {
                    workspace_root
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(PathBuf::from)
                };
                let reply = match browser
                    .create_session(CreateBrowserSessionSpec {
                        agent_id,
                        initial_url,
                        headless,
                        viewport_width,
                        viewport_height,
                        workspace_root: resolved_root,
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
            let store = Arc::clone(&runtime.store);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let mut extra_roots: Vec<PathBuf> = Vec::new();
                if let Ok(Some(owner_agent_id)) = browser.get_session_owner(&session_id).await
                    && let Ok(Some(agent)) = store.get_agent(&owner_agent_id)
                {
                    extra_roots.push(PathBuf::from(&agent.cwd));
                    if !agent.workspace_id.is_empty()
                        && let Ok(Some(ws)) = store.get_workspace(&agent.workspace_id)
                    {
                        extra_roots.push(PathBuf::from(&ws.cwd));
                        if let Some(ref src) = ws.source_cwd {
                            extra_roots.push(PathBuf::from(src));
                        }
                    }
                }
                let reply = match browser
                    .execute_action_with_allowed_roots(&session_id, action, &extra_roots)
                    .await
                {
                    Ok(outcome) => WireMessage::BrowserActionResponse {
                        request_id,
                        session: outcome.session,
                        elements: outcome.elements,
                        screenshot_base64: outcome.screenshot_base64,
                        text_excerpt: outcome.text_excerpt,
                        captcha_detected: outcome.captcha_detected,
                        picked_element: outcome.picked_element,
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
            mcp_server_name: _,
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
                        // SECURITY: client/user UpsertArtifact RPCs cannot spoof trusted MCP
                        // server ownership; only daemon-trusted MCP tool extraction binds a server.
                        mcp_server_name: None,
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
        WireMessage::ReadMcpResource {
            request_id,
            artifact_id,
            artifact_version,
            uri,
        } => {
            let home = runtime.home_dir.clone();
            let store = Arc::clone(&runtime.store);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                // Strictly resolve the bound MCP server AND workspace cwd from the persisted
                // artifact revision (never from caller-supplied server_name or active agent cwd).
                let resolved =
                    resolve_bound_artifact_mcp_owner(&store, &artifact_id, artifact_version).await;
                let (owner_server, artifact_ws_cwd) = match resolved {
                    Ok(pair) => pair,
                    Err((code, msg)) => {
                        let _ = tx
                            .send(SessionFrame::Wire {
                                msg: rpc_err(request_id, code, msg),
                                zstd,
                            })
                            .await;
                        return;
                    }
                };
                let reply = match crate::mcp_servers::read_mcp_resource(
                    &home,
                    &owner_server,
                    &uri,
                    artifact_ws_cwd.as_deref(),
                )
                .await
                {
                    Ok(content) => WireMessage::McpResourceResponse {
                        request_id,
                        server_name: owner_server,
                        uri,
                        content,
                    },
                    Err(e) => rpc_err(request_id, "read_mcp_resource_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::CallArtifactMcpTool {
            request_id,
            artifact_id,
            artifact_version,
            tool_name,
            arguments,
        } => {
            let home = runtime.home_dir.clone();
            let store = Arc::clone(&runtime.store);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                // Strictly resolve the bound MCP server AND workspace cwd from the persisted
                // artifact revision (never from caller-supplied cwd or active agent cwd).
                let resolved =
                    resolve_bound_artifact_mcp_owner(&store, &artifact_id, artifact_version).await;
                let (owner_server, artifact_ws_cwd) = match resolved {
                    Ok(pair) => pair,
                    Err((code, msg)) => {
                        let _ = tx
                            .send(SessionFrame::Wire {
                                msg: rpc_err(request_id, code, msg),
                                zstd,
                            })
                            .await;
                        return;
                    }
                };
                let reply = match crate::mcp_servers::call_mcp_tool(
                    &home,
                    &owner_server,
                    &tool_name,
                    arguments,
                    artifact_ws_cwd.as_deref(),
                )
                .await
                {
                    Ok(outcome) => WireMessage::ArtifactMcpToolResponse {
                        request_id,
                        artifact_id,
                        tool_name,
                        outcome,
                    },
                    Err(e) => rpc_err(request_id, "call_artifact_mcp_tool_failed", e),
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
            ssh_alias,
            remote_daemon_port,
        } => match runtime
            .connected_hosts
            .add_from_pairing_uri_with_transport(label, &pairing_uri, ssh_alias, remote_daemon_port)
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
        WireMessage::GetPersonalRouterStatus { request_id } => {
            let status = runtime.personal_router.status_snapshot().await;
            Some(WireMessage::PersonalRouterStatusSnapshot { request_id, status })
        }
        WireMessage::ConfigurePersonalRouter {
            request_id,
            config,
            external_webhook_secret,
            activate,
        } => {
            let pr = Arc::clone(&runtime.personal_router);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match pr
                    .configure(config, external_webhook_secret, activate)
                    .await
                {
                    Ok(status) => WireMessage::PersonalRouterStatusSnapshot { request_id, status },
                    Err(e) => rpc_err(request_id, "configure_personal_router_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ProbeRouterCapabilities { request_id } => {
            let capabilities = runtime.personal_router.probe_local_capabilities().await;
            Some(WireMessage::RouterCapabilitiesResponse {
                request_id,
                capabilities,
            })
        }
        WireMessage::DiscoverFleetRouterHosts { request_id } => {
            let pr = Arc::clone(&runtime.personal_router);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let hosts = pr.discover_fleet_router_hosts().await;
                let reply = WireMessage::FleetRouterHostsSnapshot { request_id, hosts };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
        }
        WireMessage::ExportHermesBridgeConfig { request_id } => {
            let bundle = runtime.personal_router.export_hermes_bridge_bundle().await;
            Some(WireMessage::HermesBridgeConfigResponse { request_id, bundle })
        }
        WireMessage::InspectProjectContext { request_id, cwd } => {
            let pr = Arc::clone(&runtime.personal_router);
            let tx = out_tx.clone();
            let zstd = *use_zstd;
            tokio::spawn(async move {
                let reply = match pr.inspect_local_project_context(&cwd).await {
                    Ok(context) => WireMessage::ProjectContextResponse {
                        request_id,
                        context,
                    },
                    Err(e) => rpc_err(request_id, "inspect_project_context_failed", e),
                };
                let _ = tx.send(SessionFrame::Wire { msg: reply, zstd }).await;
            });
            None
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
            transport_mode,
            app_id,
            primary_secret,
            signing_secret,
            app_token,
            public_key,
            keep_existing_secrets,
            allowed_chat_ids,
            allowed_user_ids,
            default_workspace_cwd,
            default_provider,
            default_mode,
            forward_approvals,
            routing_mode,
            router_mirror_policy,
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
                    transport_mode,
                    app_id,
                    primary_secret,
                    signing_secret,
                    app_token,
                    public_key,
                    keep_existing_secrets,
                    allowed_chat_ids,
                    allowed_user_ids,
                    default_workspace_cwd,
                    default_provider,
                    default_mode,
                    forward_approvals,
                    routing_mode,
                    router_mirror_policy,
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
        WireMessage::NotifyFleetVaultChanged {
            request_id,
            vault_fingerprint,
            revision,
        } => {
            let _ = runtime
                .orchestrator
                .broadcast_tx
                .send(DaemonBroadcast::FleetVaultChanged {
                    origin_session_id: session_id,
                    vault_fingerprint,
                    revision,
                });
            Some(WireMessage::Ack { request_id })
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

fn non_empty_trimmed(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(unix)]
fn run_hostname_cmd(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout.lines().find_map(non_empty_trimmed)
}

fn hostname_string() -> String {
    if let Ok(env_host) = std::env::var("HOSTNAME")
        && let Some(clean) = non_empty_trimmed(&env_host)
    {
        return clean;
    }
    if let Ok(file_host) = std::fs::read_to_string("/etc/hostname")
        && let Some(clean) = non_empty_trimmed(&file_host)
    {
        return clean;
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(clean) = run_hostname_cmd("/usr/sbin/scutil", &["--get", "LocalHostName"])
            .or_else(|| run_hostname_cmd("/usr/sbin/scutil", &["--get", "ComputerName"]))
        {
            return clean;
        }
    }
    #[cfg(unix)]
    {
        if let Some(clean) = run_hostname_cmd("/bin/hostname", &["-s"])
            .or_else(|| run_hostname_cmd("hostname", &["-s"]))
            .or_else(|| run_hostname_cmd("hostname", &[]))
        {
            return clean;
        }
    }
    #[cfg(windows)]
    {
        if let Ok(win_host) = std::env::var("COMPUTERNAME")
            && let Some(clean) = non_empty_trimmed(&win_host)
        {
            return clean;
        }
    }
    "orbit-host".to_string()
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
    /// Download the matching Orbit release binary, verify its SHA-256 against `SHA256SUMS.txt`
    /// from the exact same GitHub Release, verify `--version`, atomically install to
    /// `~/.local/bin/orbit` (or `%USERPROFILE%\.local\bin\orbit.exe` on Windows), and trigger a
    /// detached `orbit daemon up` hot-restart so all `orbit shim` agent & PTY sessions stay alive.
    pub async fn self_upgrade_and_restart(
        &self,
        target_version: Option<String>,
    ) -> anyhow::Result<()> {
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

        let repo = orbit_protocol::configured_github_repo();

        let release_tag = match target_version
            .as_deref()
            .map(str::trim)
            .map(|s| s.trim_start_matches(['v', 'V']))
            .filter(|s| !s.is_empty())
        {
            Some(raw_ver) => {
                if !is_valid_release_semver(raw_ver) {
                    anyhow::bail!("Invalid target version format: {raw_ver}");
                }
                format!("v{raw_ver}")
            }
            None => resolve_latest_github_release_tag(&repo).await?,
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .user_agent(format!("orbit-daemon/{}", env!("CARGO_PKG_VERSION")))
            .build()?;

        let release_base_url = format!("https://github.com/{repo}/releases/download/{release_tag}");
        let verified_sha256 =
            download_and_verify_release_binary(&client, &release_base_url, asset, &tmp_path)
                .await?;

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
            release_tag = %release_tag,
            sha256 = %verified_sha256,
            port,
            restart_relay,
            "Scheduled self-upgrade hot-restart via `orbit daemon up`"
        );
        Ok(())
    }
}

fn extract_tag_from_github_release_url(url: &str) -> Option<String> {
    let clean = url.split(['?', '#']).next()?.trim().trim_end_matches('/');
    let (_, after_tag) = clean.rsplit_once("/releases/tag/")?;
    let tag = after_tag.split('/').next()?.trim();
    if tag.is_empty() {
        return None;
    }
    Some(tag.to_string())
}

async fn resolve_latest_github_release_tag(repo: &str) -> anyhow::Result<String> {
    let latest_web_url = format!("https://github.com/{repo}/releases/latest");
    let no_redirect = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("orbit-daemon/{}", env!("CARGO_PKG_VERSION")))
        .build()?;

    let mut web_err: Option<String> = None;
    for use_head in [true, false] {
        let req = if use_head {
            no_redirect.head(&latest_web_url)
        } else {
            no_redirect.get(&latest_web_url)
        };
        match req.send().await {
            Ok(resp) => {
                if resp.status().is_redirection()
                    && let Some(loc) = resp
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|v| v.to_str().ok())
                    && let Some(tag) = extract_tag_from_github_release_url(loc)
                {
                    return Ok(tag);
                }
                if let Some(tag) = extract_tag_from_github_release_url(resp.url().as_str()) {
                    return Ok(tag);
                }
                if !use_head {
                    web_err = Some(format!(
                        "HTTP {} without release tag redirect",
                        resp.status()
                    ));
                }
            }
            Err(e) if !use_head => {
                web_err = Some(e.to_string());
            }
            Err(_) => {}
        }
    }

    let api_url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let api_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("orbit-daemon/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = api_client
        .get(&api_url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to resolve latest Orbit release tag (web redirect: {}; API: {e})",
                web_err.as_deref().unwrap_or("unknown")
            )
        })?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "Failed to resolve latest Orbit release tag (web redirect: {}; API HTTP {})",
            web_err.as_deref().unwrap_or("unknown"),
            resp.status()
        );
    }
    let meta: serde_json::Value = resp.json().await?;
    meta.get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing tag_name in GitHub release metadata"))
}

/// Fetch `SHA256SUMS.txt` and `asset` from the exact same release base URL, streaming the binary
/// to `tmp_path` while hashing, and verifying SHA-256 BEFORE marking executable or executing.
async fn download_and_verify_release_binary(
    client: &reqwest::Client,
    release_base_url: &str,
    asset: &str,
    tmp_path: &Path,
) -> anyhow::Result<String> {
    use tokio::io::AsyncWriteExt;

    let base = release_base_url.trim_end_matches('/');
    let sums_url = format!("{base}/{}", orbit_crypto::SHA256SUMS_FILENAME);
    let asset_url = format!("{base}/{asset}");

    let sums_resp = client
        .get(&sums_url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Network error downloading checksums {sums_url}: {e}"))?;
    if !sums_resp.status().is_success() {
        anyhow::bail!(
            "Failed to fetch release checksums {sums_url} (HTTP {}); refusing unverified upgrade",
            sums_resp.status()
        );
    }
    let sums_text = sums_resp
        .text()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read {sums_url}: {e}"))?;
    let expected_sha256 = orbit_crypto::find_expected_sha256(&sums_text, asset).map_err(|e| {
        anyhow::anyhow!("Release checksum manifest {sums_url} rejected `{asset}`: {e}")
    })?;

    let mut resp = client
        .get(&asset_url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Network error downloading {asset_url}: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} downloading {asset_url}", resp.status());
    }

    let mut file = tokio::fs::File::create(tmp_path).await?;
    let mut hasher = orbit_crypto::Sha256Hasher::new();
    let mut stream_err: Option<anyhow::Error> = None;
    loop {
        match resp.chunk().await {
            Ok(Some(bytes)) => {
                hasher.update(&bytes);
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
        let _ = tokio::fs::remove_file(tmp_path).await;
        return Err(err);
    }
    if let Err(e) = file.flush().await {
        let _ = tokio::fs::remove_file(tmp_path).await;
        return Err(e.into());
    }
    drop(file);

    let actual_sha256 = hasher.finalize_hex();
    if let Err(err) = orbit_crypto::verify_sha256_hex(asset, &expected_sha256, &actual_sha256) {
        let _ = tokio::fs::remove_file(tmp_path).await;
        anyhow::bail!("SHA-256 verification failed for {asset_url}: {err}");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(tmp_path, std::fs::Permissions::from_mode(0o755)) {
            let _ = std::fs::remove_file(tmp_path);
            return Err(e.into());
        }
    }

    Ok(actual_sha256)
}

fn parse_relay_probe_addr_from_json(raw: &str) -> Option<SocketAddr> {
    let val: serde_json::Value = serde_json::from_str(raw).ok()?;
    let addr_str = val.get("listen_addr")?.as_str()?;
    let addr = addr_str.parse::<SocketAddr>().ok()?;
    Some(match addr {
        SocketAddr::V4(v4) if v4.ip().is_unspecified() => {
            SocketAddr::from(([127, 0, 0, 1], v4.port()))
        }
        SocketAddr::V6(v6) if v6.ip().is_unspecified() => {
            SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, v6.port()))
        }
        explicit => explicit,
    })
}

async fn detect_running_local_relay(home_dir: &Path) -> bool {
    let relay_cfg_path = home_dir.join("relay.json");
    let Ok(raw) = std::fs::read_to_string(&relay_cfg_path) else {
        return false;
    };
    let probe_addr = parse_relay_probe_addr_from_json(&raw)
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 7878)));
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return false;
    };
    let Ok(resp) = client
        .get(format!("http://{probe_addr}/healthz"))
        .send()
        .await
    else {
        return false;
    };
    if resp.status() != reqwest::StatusCode::OK {
        return false;
    }
    if resp.content_length().is_some_and(|len| len > 64) {
        return false;
    }
    resp.text()
        .await
        .map(|body| body.trim() == "ok")
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

/// Strictly resolve the owning MCP server name and persisted workspace `cwd` for a stored
/// artifact revision (`artifact_id`, optional `artifact_version`).
///
/// SECURITY: Never accepts caller-supplied `server_name` or active-agent `cwd`, preventing
/// cross-project server hijacking when identically named servers exist in multiple repos.
async fn resolve_bound_artifact_mcp_owner(
    store: &Arc<OrbitStore>,
    artifact_id: &str,
    artifact_version: Option<u32>,
) -> Result<(String, Option<PathBuf>), (&'static str, String)> {
    let aid_lookup = artifact_id.trim().to_string();
    if aid_lookup.is_empty() {
        return Err((
            "artifact_not_found",
            "Artifact ID must not be empty".to_string(),
        ));
    }
    let store_lookup = Arc::clone(store);
    let lookup_res = tokio::task::spawn_blocking(move || {
        let detail = store_lookup.get_artifact(&aid_lookup, artifact_version)?;
        let ws_cwd = detail
            .as_ref()
            .and_then(|d| {
                store_lookup
                    .get_workspace(&d.summary.workspace_id)
                    .ok()
                    .flatten()
            })
            .map(|w| PathBuf::from(w.cwd));
        Ok::<_, orbit_store::StoreError>((detail, ws_cwd))
    })
    .await;

    let (detail, ws_cwd) = match lookup_res {
        Ok(Ok(pair)) => pair,
        Ok(Err(e)) => return Err(("artifact_lookup_failed", e.to_string())),
        Err(e) => return Err(("artifact_lookup_failed", e.to_string())),
    };

    let Some(detail) = detail else {
        return Err((
            "artifact_not_found",
            format!("Artifact `{artifact_id}` not found"),
        ));
    };

    let Some(owner_server) = detail
        .summary
        .mcp_server_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
    else {
        return Err((
            "artifact_mcp_server_unbound",
            format!(
                "Artifact `{artifact_id}` (v{}) is not bound to an owning external MCP server",
                detail.summary.version
            ),
        ));
    };

    Ok((owner_server, ws_cwd))
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

/// Authorize a repository-wide Git mutation (`commit`, `push`, `create_pr`):
/// both the requested `cwd` AND the resolved Git repository top-level root must be authorized
/// so a subdirectory workspace cannot commit, push, or open a PR covering sibling directories
/// outside its authorized workspace root.
async fn authorize_git_repo_root_mutation(
    store: &Arc<OrbitStore>,
    resolved_cwd: &Path,
) -> Result<PathBuf, (String, &'static str)> {
    let authorized_cwd = authorize_workspace_mutation_cwd(store, resolved_cwd)
        .await
        .map_err(|e| (e, "git_mutation_denied"))?;
    let repo_root = crate::git_write::resolve_repo_root(&authorized_cwd)
        .await
        .map_err(|e| (e.to_string(), e.code()))?;
    authorize_workspace_mutation_cwd(store, &repo_root)
        .await
        .map_err(|e| {
            (
                format!(
                    "Repository root `{}` is outside authorized workspace scope: {e}",
                    repo_root.display()
                ),
                "git_mutation_denied",
            )
        })?;
    Ok(repo_root)
}

#[cfg(test)]
mod self_upgrade_tests {
    use super::*;

    #[tokio::test]
    async fn git_write_repo_root_authorization_blocks_subdir_only_workspace() {
        let root = std::env::temp_dir().join(format!("orbit_git_root_auth_{}", Uuid::new_v4()));
        let repo = root.join("monorepo");
        let subdir = repo.join("packages/app");
        std::fs::create_dir_all(&subdir).expect("mkdir subdir");

        let init_out = tokio::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(&repo)
            .output()
            .await
            .expect("git init");
        assert!(init_out.status.success(), "git init failed");

        let store = Arc::new(OrbitStore::open(root.join("orbit.db")).expect("open store"));
        // Register ONLY the nested subdirectory, NOT the enclosing monorepo root.
        store
            .upsert_host_project(&orbit_protocol::HostProjectEntry {
                project_id: "app_only".into(),
                name: "App Subdir".into(),
                directories: vec![subdir.to_string_lossy().to_string()],
                sort_order: 0,
                hidden: false,
                updated_at_ms: 1,
            })
            .expect("upsert subdir project");

        assert!(
            authorize_workspace_mutation_cwd(&store, &subdir)
                .await
                .is_ok()
        );
        let denied = authorize_git_repo_root_mutation(&store, &subdir)
            .await
            .expect_err("subdir-only workspace must not authorize repo-wide commit/push/PR");
        assert_eq!(denied.1, "git_mutation_denied");

        // Once the repo root itself is authorized, repo-wide mutations from subdir succeed.
        store
            .upsert_host_project(&orbit_protocol::HostProjectEntry {
                project_id: "monorepo_root".into(),
                name: "Monorepo Root".into(),
                directories: vec![repo.to_string_lossy().to_string()],
                sort_order: 1,
                hidden: false,
                updated_at_ms: 2,
            })
            .expect("upsert repo root project");

        let auth_root = authorize_git_repo_root_mutation(&store, &subdir)
            .await
            .expect("repo root authorization should pass once repo root is registered");
        let canon_repo = crate::platform::canonicalize_clean(&repo).expect("canon repo");
        assert_eq!(auth_root, canon_repo);

        let _ = std::fs::remove_dir_all(&root);
    }

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
    async fn resolve_bound_artifact_mcp_owner_scopes_to_persisted_workspace_and_revision() {
        let root = std::env::temp_dir().join(format!("orbit_art_owner_{}", Uuid::new_v4()));
        let ws_a = root.join("ws_a");
        std::fs::create_dir_all(&ws_a).expect("mkdir ws_a");
        let store = Arc::new(OrbitStore::open(root.join("orbit.db")).expect("open store"));
        let now = chrono::Utc::now();
        store
            .upsert_workspace(&orbit_protocol::WorkspaceSummary {
                workspace_id: "ws_a".into(),
                title: "Workspace A".into(),
                cwd: ws_a.to_string_lossy().to_string(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                is_curated: false,
                auto_clustered: false,
            })
            .expect("upsert ws_a");

        // v1 bound to `analytics`
        store
            .upsert_artifact(orbit_store::UpsertArtifactInput {
                artifact_id: Some("art_scoped".into()),
                workspace_id: "ws_a".into(),
                agent_id: None,
                title: "Scoped App".into(),
                kind: orbit_protocol::ArtifactKind::McpApp,
                language: Some("html".into()),
                mime_type: None,
                resource_uri: Some("ui://analytics/v1".into()),
                mcp_server_name: Some("analytics".into()),
                content: "<div>v1</div>".into(),
                summary_note: None,
            })
            .expect("v1");

        // v2 edited without MCP owner -> must NOT inherit v1's server binding
        store
            .upsert_artifact(orbit_store::UpsertArtifactInput {
                artifact_id: Some("art_scoped".into()),
                workspace_id: "ws_a".into(),
                agent_id: None,
                title: "Scoped App".into(),
                kind: orbit_protocol::ArtifactKind::Html,
                language: Some("html".into()),
                mime_type: None,
                resource_uri: Some("ui://analytics/v1".into()),
                mcp_server_name: None,
                content: "<div>v2</div>".into(),
                summary_note: None,
            })
            .expect("v2");

        // v1 resolves strictly to `analytics` + persisted `ws_a` cwd
        let (srv, resolved_cwd) = resolve_bound_artifact_mcp_owner(&store, "art_scoped", Some(1))
            .await
            .expect("v1 bound");
        assert_eq!(srv, "analytics");
        assert_eq!(resolved_cwd, Some(ws_a.clone()));

        // Latest (v2) is unbound and MUST be rejected with `artifact_mcp_server_unbound`
        let (code, _) = resolve_bound_artifact_mcp_owner(&store, "art_scoped", None)
            .await
            .expect_err("v2 unbound");
        assert_eq!(code, "artifact_mcp_server_unbound");

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
    fn parse_relay_probe_addr_from_json_reads_listen_addr() {
        assert_eq!(
            parse_relay_probe_addr_from_json(
                r#"{"listen_addr":"0.0.0.0:7878","allowed_pubkeys":[]}"#
            ),
            Some(SocketAddr::from(([127, 0, 0, 1], 7878)))
        );
        assert_eq!(
            parse_relay_probe_addr_from_json(r#"{"listen_addr":"[::]:7879"}"#),
            Some(SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, 7879)))
        );
        assert_eq!(
            parse_relay_probe_addr_from_json(r#"{"listen_addr":"[::1]:9001"}"#),
            Some(SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, 9001)))
        );
        assert_eq!(
            parse_relay_probe_addr_from_json(r#"{"listen_addr":"127.0.0.2:7880"}"#),
            Some(SocketAddr::from(([127, 0, 0, 2], 7880)))
        );
        assert_eq!(parse_relay_probe_addr_from_json("not json"), None);
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

    #[test]
    fn hostname_string_resolves_real_non_fallback_host_name() {
        let resolved = hostname_string();
        assert!(!resolved.trim().is_empty());
        assert_ne!(resolved, "orbit-host");
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

    #[tokio::test]
    async fn pairing_secret_persists_across_bootstrap_rotates_atomically_and_fails_closed() {
        let home = std::env::temp_dir().join(format!("orbit_secret_persist_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&home).expect("mkdir");

        // Concurrent first-time init must converge on a single identical secret
        let mut init_tasks = Vec::new();
        for _ in 0..8 {
            let h = home.clone();
            init_tasks.push(tokio::task::spawn_blocking(move || {
                load_or_create_pairing_secret(&h).expect("concurrent init")
            }));
        }
        let mut init_results = Vec::new();
        for t in init_tasks {
            init_results.push(t.await.expect("join"));
        }
        let first_secret = init_results[0].clone();
        assert_eq!(first_secret.len(), 43);
        for s in &init_results {
            assert_eq!(s, &first_secret, "concurrent init must not diverge");
        }

        let rt1 = DaemonRuntime::bootstrap(&home).await.expect("bootstrap 1");
        let secret1 = rt1.pairing_secret.clone();
        assert_eq!(secret1, first_secret);
        let offer1 = rt1
            .build_pairing_offer(vec!["ws://127.0.0.1:6868/ws".into()], None)
            .expect("offer 1");
        assert_eq!(offer1.pairing_secret, secret1);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(home.join(PAIRING_SECRET_FILENAME)).expect("stat secret");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
        drop(rt1);

        let rt2 = DaemonRuntime::bootstrap(&home).await.expect("bootstrap 2");
        assert_eq!(
            rt2.pairing_secret, secret1,
            "pairing_secret must persist identically across daemon restarts"
        );
        assert_eq!(
            rt2.build_pairing_offer(vec![], None)
                .expect("offer 2")
                .pairing_secret,
            secret1
        );

        let rotated = rt2.rotate_pairing_secret().expect("rotate secret");
        assert_eq!(rotated.len(), 43);
        assert_ne!(rotated, secret1);
        assert_eq!(
            rt2.active_pairing_secret().as_deref(),
            Some(rotated.as_str())
        );
        assert_eq!(
            rt2.build_pairing_offer(vec![], None)
                .expect("offer rotated")
                .pairing_secret,
            rotated
        );
        drop(rt2);

        let rt3 = DaemonRuntime::bootstrap(&home).await.expect("bootstrap 3");
        assert_eq!(rt3.pairing_secret, rotated);

        // Corrupting the persisted secret must fail closed (never overwrite or fall back)
        std::fs::write(home.join(PAIRING_SECRET_FILENAME), "short").expect("corrupt");
        assert_eq!(rt3.active_pairing_secret(), None);
        assert!(rt3.build_pairing_offer(vec![], None).is_err());
        assert!(load_or_create_pairing_secret(&home).is_err());

        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn ws_endpoint_enforces_bearer_auth_origin_cswsh_live_rotation_and_fail_closed() {
        use tokio_tungstenite::{
            connect_async,
            tungstenite::{
                Error as WsError,
                client::IntoClientRequest,
                http::{
                    HeaderValue,
                    header::{AUTHORIZATION, ORIGIN},
                },
            },
        };

        let home = std::env::temp_dir().join(format!("orbit_ws_auth_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&home).expect("mkdir");
        let (addr, runtime) = spawn_embedded_daemon(home.clone(), None)
            .await
            .expect("spawn daemon");
        let ws_url = format!("ws://{addr}/ws");
        let valid_secret = runtime.pairing_secret.clone();

        // 1. Missing Authorization header -> 401 Unauthorized
        let err = connect_async(&ws_url)
            .await
            .expect_err("missing token must fail");
        match err {
            WsError::Http(resp) => assert_eq!(resp.status().as_u16(), 401),
            other => panic!("expected HTTP 401 for missing token, got {other:?}"),
        }

        // 2. Wrong Bearer token -> 401 Unauthorized
        let mut wrong_req = ws_url.as_str().into_client_request().expect("req");
        wrong_req.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong-secret-token-00000000000000000000"),
        );
        let err = connect_async(wrong_req)
            .await
            .expect_err("wrong token must fail");
        match err {
            WsError::Http(resp) => assert_eq!(resp.status().as_u16(), 401),
            other => panic!("expected HTTP 401 for wrong token, got {other:?}"),
        }

        // 3. Malicious browser Origin -> 403 Forbidden (even with valid Bearer token!)
        let mut evil_req = ws_url.as_str().into_client_request().expect("req");
        evil_req.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {valid_secret}")).expect("hdr"),
        );
        evil_req
            .headers_mut()
            .insert(ORIGIN, HeaderValue::from_static("https://evil.example"));
        let err = connect_async(evil_req)
            .await
            .expect_err("malicious origin must fail");
        match err {
            WsError::Http(resp) => assert_eq!(resp.status().as_u16(), 403),
            other => panic!("expected HTTP 403 for malicious origin, got {other:?}"),
        }

        // 4. Valid Bearer token with no Origin (native CLI/mobile) -> keep open to test rotation revocation!
        let mut native_req = ws_url.as_str().into_client_request().expect("req");
        native_req.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {valid_secret}")).expect("hdr"),
        );
        let (mut active_ws, _) = connect_async(native_req)
            .await
            .expect("valid native client must connect");

        // 5. Valid Bearer token with explicit allowed Tauri Origin -> succeeds
        let mut tauri_req = ws_url.as_str().into_client_request().expect("req");
        tauri_req.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {valid_secret}")).expect("hdr"),
        );
        tauri_req
            .headers_mut()
            .insert(ORIGIN, HeaderValue::from_static("tauri://localhost"));
        let (mut ws_tauri, _) = connect_async(tauri_req)
            .await
            .expect("allowed tauri origin with valid token must connect");
        let _ = ws_tauri.close(None).await;

        // 6. Secret rotation invalidates old secret immediately AND closes already-active sessions!
        let rotated = runtime.rotate_pairing_secret().expect("rotate");
        let closed = tokio::time::timeout(Duration::from_secs(3), async {
            while let Some(msg) = active_ws.next().await {
                if matches!(msg, Ok(TungMsg::Close(_)) | Err(_)) {
                    return true;
                }
            }
            true
        })
        .await
        .unwrap_or(false);
        assert!(
            closed,
            "already-connected direct /ws session must close when pairing_secret is rotated"
        );

        let mut stale_req = ws_url.as_str().into_client_request().expect("req");
        stale_req.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {valid_secret}")).expect("hdr"),
        );
        match connect_async(stale_req)
            .await
            .expect_err("rotated secret must reject old token")
        {
            WsError::Http(resp) => assert_eq!(resp.status().as_u16(), 401),
            other => panic!("expected HTTP 401 for stale token, got {other:?}"),
        }

        let mut fresh_req = ws_url.as_str().into_client_request().expect("req");
        fresh_req.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {rotated}")).expect("hdr"),
        );
        let (mut ws_fresh, _) = connect_async(fresh_req)
            .await
            .expect("rotated secret must connect");
        let _ = ws_fresh.close(None).await;

        // 7. Missing/deleted pairing-secret file must fail closed (NEVER fall back to bootstrap secret!)
        std::fs::remove_file(home.join(PAIRING_SECRET_FILENAME)).expect("remove secret");
        for tok in [&valid_secret, &rotated] {
            let mut req = ws_url.as_str().into_client_request().expect("req");
            req.headers_mut().insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {tok}")).expect("hdr"),
            );
            match connect_async(req)
                .await
                .expect_err("deleted secret must fail closed")
            {
                WsError::Http(resp) => assert_eq!(resp.status().as_u16(), 401),
                other => panic!("expected HTTP 401 when secret file deleted, got {other:?}"),
            }
        }

        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn mcp_agents_endpoint_enforces_origin_and_constant_time_bearer_auth() {
        let home = std::env::temp_dir().join(format!("orbit_mcp_auth_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&home).expect("mkdir");
        let (addr, runtime) = spawn_embedded_daemon(home.clone(), None)
            .await
            .expect("spawn daemon");
        let url = format!("http://{addr}/mcp/agents");
        let http = reqwest::Client::new();
        let ping_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping"
        });

        // 1. Malicious Origin -> 403 Forbidden (even with valid MCP token)
        let resp = http
            .post(&url)
            .header("Origin", "https://evil.example")
            .header("Authorization", format!("Bearer {}", runtime.mcp_token))
            .json(&ping_body)
            .send()
            .await
            .expect("send evil origin");
        assert_eq!(resp.status().as_u16(), 403);

        // 2. Missing token -> 401 Unauthorized
        let resp = http
            .post(&url)
            .json(&ping_body)
            .send()
            .await
            .expect("send missing auth");
        assert_eq!(resp.status().as_u16(), 401);

        // 3. Wrong token -> 401 Unauthorized
        let resp = http
            .post(&url)
            .header("Authorization", "Bearer wrong-mcp-token")
            .json(&ping_body)
            .send()
            .await
            .expect("send wrong auth");
        assert_eq!(resp.status().as_u16(), 401);

        // 4. Valid token without Origin -> 200 OK
        let resp = http
            .post(&url)
            .header("Authorization", format!("Bearer {}", runtime.mcp_token))
            .json(&ping_body)
            .send()
            .await
            .expect("send valid auth");
        assert_eq!(resp.status().as_u16(), 200);

        // 5. Actual bound SocketAddr is recorded in `$ORBIT_HOME/daemon.addr`
        assert_eq!(read_live_daemon_addr(&home), Some(addr));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(home.join(DAEMON_ADDR_FILENAME))
                .expect("daemon.addr metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }

        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bootstrap_hardens_orbit_home_shims_and_runtime_dirs_to_0700() {
        use std::os::unix::fs::PermissionsExt;

        let home = std::env::temp_dir().join(format!("orbit-home-perm-test-{}", Uuid::new_v4()));
        let shims = home.join("shims");
        let runtime_dir = home.join("runtime");
        std::fs::create_dir_all(&shims).expect("create shims");
        std::fs::create_dir_all(&runtime_dir).expect("create runtime");
        for dir in [&home, &shims, &runtime_dir] {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))
                .expect("set initial 0755 perms");
        }

        let _rt = DaemonRuntime::bootstrap(&home)
            .await
            .expect("bootstrap daemon");

        for (label, dir) in [
            ("ORBIT_HOME", &home),
            ("shims", &shims),
            ("runtime", &runtime_dir),
        ] {
            let mode = std::fs::metadata(dir)
                .expect("dir metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(
                mode, 0o700,
                "{label} directory must be hardened to 0700, got {:o}",
                mode
            );
        }

        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn revocation_terminates_session_immediately_even_when_outbound_queue_is_saturated() {
        let home = std::env::temp_dir().join(format!("orbit_rev_sat_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&home).expect("mkdir");
        let runtime = DaemonRuntime::bootstrap(&home).await.expect("bootstrap");
        let secret = runtime.pairing_secret.clone();

        let (in_tx, in_rx) = mpsc::channel::<SessionFrame>(4);
        let (out_tx, mut out_rx) = mpsc::channel::<SessionFrame>(1);

        // Saturate the outbound queue with a pending terminal frame so `out_tx.send(...)` blocks.
        out_tx
            .try_send(SessionFrame::Terminal(TerminalBinaryFrame::Output {
                slot: 0,
                data: b"queued-pty-bytes".to_vec(),
            }))
            .expect("fill outbound queue");

        let rt_session = Arc::clone(&runtime);
        let session_task = tokio::spawn(async move {
            run_supervised_client_session(rt_session, in_rx, out_tx, secret, None).await
        });

        // Trigger a Ping RPC whose Pong reply blocks on the saturated `out_tx` channel.
        in_tx
            .send(SessionFrame::Wire {
                msg: WireMessage::Ping { ts_ms: 42 },
                zstd: false,
            })
            .await
            .expect("send ping");
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(
            !session_task.is_finished(),
            "session should still be running while blocked on saturated out_tx"
        );

        // Rotate pairing_secret: independent revocation supervisor MUST exit with Revoked
        // immediately without waiting for `out_tx` to drain.
        let _rotated = runtime.rotate_pairing_secret().expect("rotate");
        let exit = tokio::time::timeout(Duration::from_millis(500), session_task)
            .await
            .expect("revocation must not block on saturated outbound queue")
            .expect("session task join");
        assert_eq!(exit, SessionExitReason::Revoked);

        // Only the pre-queued frame should exist; the Pong must not have been enqueued after revocation.
        assert!(out_rx.try_recv().is_ok());
        assert!(out_rx.try_recv().is_err());

        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn detect_running_local_relay_probes_healthz_endpoint() {
        let base = std::env::temp_dir().join(format!(
            "orbit_relay_healthz_test_{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&base).expect("mkdir");

        // 1. Missing relay.json -> false
        assert!(!detect_running_local_relay(&base).await);

        // 2. Local server serving `/healthz` (matching `orbit-relay`, wildcard 0.0.0.0 -> 127.0.0.1) -> true
        let healthz_app = Router::new().route("/healthz", get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind healthz");
        let healthz_addr = listener.local_addr().expect("addr");
        let healthz_srv = tokio::spawn(async move {
            let _ = axum::serve(listener, healthz_app).await;
        });

        std::fs::write(
            base.join("relay.json"),
            serde_json::json!({
                "listen_addr": format!("0.0.0.0:{}", healthz_addr.port()),
                "allowed_pubkeys": []
            })
            .to_string(),
        )
        .expect("write relay.json");
        assert!(
            detect_running_local_relay(&base).await,
            "expected local relay serving /healthz to be detected"
        );
        healthz_srv.abort();

        // 3. IPv6 loopback listener (`[::1]`) -> true
        if let Ok(v6_listener) = tokio::net::TcpListener::bind("[::1]:0").await {
            let v6_addr = v6_listener.local_addr().expect("v6 addr");
            let v6_app = Router::new().route("/healthz", get(|| async { "ok" }));
            let v6_srv = tokio::spawn(async move {
                let _ = axum::serve(v6_listener, v6_app).await;
            });
            std::fs::write(
                base.join("relay.json"),
                serde_json::json!({
                    "listen_addr": v6_addr.to_string(),
                    "allowed_pubkeys": []
                })
                .to_string(),
            )
            .expect("write v6 relay.json");
            assert!(
                detect_running_local_relay(&base).await,
                "expected IPv6 loopback relay serving /healthz to be detected"
            );
            v6_srv.abort();
        }

        // 4. Redirect on `/healthz` must NOT be followed -> false
        let redirect_app = Router::new()
            .route(
                "/healthz",
                get(|| async { axum::response::Redirect::temporary("/ok") }),
            )
            .route("/ok", get(|| async { "ok" }));
        let redir_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect");
        let redir_addr = redir_listener.local_addr().expect("redir addr");
        let redir_srv = tokio::spawn(async move {
            let _ = axum::serve(redir_listener, redirect_app).await;
        });
        std::fs::write(
            base.join("relay.json"),
            serde_json::json!({
                "listen_addr": redir_addr.to_string(),
                "allowed_pubkeys": []
            })
            .to_string(),
        )
        .expect("write redirect relay.json");
        assert!(
            !detect_running_local_relay(&base).await,
            "redirecting /healthz response must not pass health probe"
        );
        redir_srv.abort();

        // 5. Server serving only legacy `/health` (404 on `/healthz`) -> false
        let legacy_app = Router::new().route("/health", get(|| async { "ok" }));
        let legacy_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind legacy");
        let legacy_addr = legacy_listener.local_addr().expect("legacy addr");
        let legacy_srv = tokio::spawn(async move {
            let _ = axum::serve(legacy_listener, legacy_app).await;
        });
        std::fs::write(
            base.join("relay.json"),
            serde_json::json!({
                "listen_addr": legacy_addr.to_string(),
                "allowed_pubkeys": []
            })
            .to_string(),
        )
        .expect("write legacy relay.json");
        assert!(
            !detect_running_local_relay(&base).await,
            "server without /healthz must not be treated as healthy orbit-relay"
        );
        legacy_srv.abort();

        // 6. Server returning HTTP 200 JSON on `/healthz` (e.g. daemon or unrelated service) -> false
        let json_app = Router::new().route(
            "/healthz",
            get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }),
        );
        let json_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind json healthz");
        let json_addr = json_listener.local_addr().expect("json addr");
        let json_srv = tokio::spawn(async move {
            let _ = axum::serve(json_listener, json_app).await;
        });
        std::fs::write(
            base.join("relay.json"),
            serde_json::json!({
                "listen_addr": json_addr.to_string(),
                "allowed_pubkeys": []
            })
            .to_string(),
        )
        .expect("write json relay.json");
        assert!(
            !detect_running_local_relay(&base).await,
            "200 JSON /healthz body must not be treated as healthy orbit-relay"
        );
        json_srv.abort();

        // 7. Server returning HTTP 204 No Content on `/healthz` -> false
        let no_content_app = Router::new().route(
            "/healthz",
            get(|| async { axum::http::StatusCode::NO_CONTENT }),
        );
        let nc_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind 204 healthz");
        let nc_addr = nc_listener.local_addr().expect("204 addr");
        let nc_srv = tokio::spawn(async move {
            let _ = axum::serve(nc_listener, no_content_app).await;
        });
        std::fs::write(
            base.join("relay.json"),
            serde_json::json!({
                "listen_addr": nc_addr.to_string(),
                "allowed_pubkeys": []
            })
            .to_string(),
        )
        .expect("write 204 relay.json");
        assert!(
            !detect_running_local_relay(&base).await,
            "204 /healthz must not be treated as healthy orbit-relay"
        );
        nc_srv.abort();

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn download_and_verify_release_binary_enforces_sha256sums() {
        const GENUINE_BYTES: &[u8] = b"genuine-orbit-daemon-release-binary-v0.9.2";
        const TAMPERED_BYTES: &[u8] = b"tampered-orbit-daemon-release-binary-v0.9.2";
        let genuine_sha = orbit_crypto::sha256_hex_bytes(GENUINE_BYTES);
        let valid_sums = format!("{genuine_sha}  orbit-cli-linux-x64\n");
        let missing_asset_sums = format!("{genuine_sha}  other-asset\n");

        let app = Router::new()
            .route(
                "/ok/SHA256SUMS.txt",
                get({
                    let s = valid_sums.clone();
                    move || async move { s }
                }),
            )
            .route("/ok/orbit-cli-linux-x64", get(|| async { GENUINE_BYTES }))
            .route(
                "/tampered/SHA256SUMS.txt",
                get({
                    let s = valid_sums.clone();
                    move || async move { s }
                }),
            )
            .route(
                "/tampered/orbit-cli-linux-x64",
                get(|| async { TAMPERED_BYTES }),
            )
            .route(
                "/missing-entry/SHA256SUMS.txt",
                get({
                    let s = missing_asset_sums.clone();
                    move || async move { s }
                }),
            )
            .route(
                "/missing-entry/orbit-cli-linux-x64",
                get(|| async { GENUINE_BYTES }),
            )
            .route(
                "/no-sums/orbit-cli-linux-x64",
                get(|| async { GENUINE_BYTES }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock release server");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("client");
        let tmp_dir = std::env::temp_dir().join(format!(
            "orbit_daemon_sha256_test_{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&tmp_dir).expect("mkdir");

        // 1. Valid SHA256SUMS.txt + matching binary -> succeeds
        let ok_path = tmp_dir.join("orbit.ok");
        let verified = download_and_verify_release_binary(
            &client,
            &format!("http://{addr}/ok"),
            "orbit-cli-linux-x64",
            &ok_path,
        )
        .await
        .expect("matching sha256 must succeed");
        assert_eq!(verified, genuine_sha);
        assert_eq!(std::fs::read(&ok_path).expect("read ok"), GENUINE_BYTES);

        // 2. Tampered binary -> aborts with mismatch error & deletes temp file
        let bad_path = tmp_dir.join("orbit.tampered");
        let err = download_and_verify_release_binary(
            &client,
            &format!("http://{addr}/tampered"),
            "orbit-cli-linux-x64",
            &bad_path,
        )
        .await
        .expect_err("tampered binary must be rejected");
        assert!(err.to_string().contains("SHA-256"), "unexpected err: {err}");
        assert!(
            !bad_path.exists(),
            "tampered temp file must be removed immediately"
        );

        // 3. Missing SHA256SUMS.txt -> aborts cleanly
        let no_sums_path = tmp_dir.join("orbit.nosums");
        let err = download_and_verify_release_binary(
            &client,
            &format!("http://{addr}/no-sums"),
            "orbit-cli-linux-x64",
            &no_sums_path,
        )
        .await
        .expect_err("missing SHA256SUMS.txt must fail");
        assert!(
            err.to_string().contains("refusing unverified upgrade"),
            "unexpected err: {err}"
        );
        assert!(!no_sums_path.exists());

        // 4. Missing asset entry in SHA256SUMS.txt -> aborts cleanly
        let missing_path = tmp_dir.join("orbit.missing");
        let err = download_and_verify_release_binary(
            &client,
            &format!("http://{addr}/missing-entry"),
            "orbit-cli-linux-x64",
            &missing_path,
        )
        .await
        .expect_err("missing asset entry must fail");
        assert!(
            err.to_string().contains("not found in SHA256SUMS.txt"),
            "unexpected err: {err}"
        );
        assert!(!missing_path.exists());

        server.abort();
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[tokio::test]
    async fn first_e2ee_transport_record_rejects_silent_replay_and_forged_ciphertext_before_session()
     {
        use orbit_crypto::ClientHandshake;
        use tokio_tungstenite::{accept_async, connect_async};

        let kp = HostIdentityKeypair::generate().expect("keygen");
        let pub_bytes = kp.public_bytes().expect("pub");
        let priv_bytes = kp.private_bytes().expect("priv");
        let secret = orbit_crypto::generate_pairing_secret();

        let (client_hs1, msg1) = ClientHandshake::new(&pub_bytes, &secret).expect("client init");
        let (_daemon_chan1, msg2_1) =
            DaemonHandshake::accept(&priv_bytes, &secret, &msg1).expect("daemon accept 1");
        let mut client_chan1 = client_hs1.finalize(&msg2_1).expect("finalize 1");
        let hello = WireMessage::Hello {
            client_id: "c1".into(),
            client_name: "test".into(),
            protocol_version: PROTOCOL_VERSION,
            supports_zstd: true,
        };
        let hello_z = hello.encode_zstd(3).expect("zstd");
        let mut plain = Vec::with_capacity(1 + hello_z.len());
        plain.push(E2EE_FRAME_WIRE);
        plain.extend_from_slice(&hello_z);
        let legit_ct1 = client_chan1.seal_record(&plain).expect("seal hello");

        // Case 1: Replayed msg1 with silence (+ Ping keepalives) times out and closes with Policy.
        {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("addr");
            let (mut daemon_replayed, _msg2_replay) =
                DaemonHandshake::accept(&priv_bytes, &secret, &msg1).expect("replayed accept");

            let srv = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.expect("accept");
                let ws = accept_async(tcp).await.expect("ws accept");
                let (mut sink, mut stream) = ws.split();
                receive_first_e2ee_wire_message(
                    &mut daemon_replayed,
                    &mut sink,
                    &mut stream,
                    Duration::from_millis(120),
                )
                .await
            });

            let (mut cli_ws, _) = connect_async(format!("ws://{addr}"))
                .await
                .expect("connect");
            // Send a Ping; daemon must reply Pong but still enforce the overall timeout.
            cli_ws
                .send(TungMsg::Ping(b"keepalive".to_vec().into()))
                .await
                .expect("send ping");
            let mut saw_close_policy = false;
            while let Ok(Some(Ok(frame))) =
                tokio::time::timeout(Duration::from_secs(2), cli_ws.next()).await
            {
                if let TungMsg::Close(Some(cf)) = frame
                    && cf.code == TungCloseCode::Policy
                {
                    saw_close_policy = true;
                    break;
                }
            }
            assert!(
                saw_close_policy,
                "silent replayed msg1 must receive Policy close"
            );
            assert!(srv.await.expect("join").is_err());
        }

        // Case 2: Replayed msg1 followed by captured ciphertext from session 1 fails immediately.
        {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("addr");
            let (mut daemon_replayed, _msg2_replay) =
                DaemonHandshake::accept(&priv_bytes, &secret, &msg1).expect("replayed accept");

            let srv = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.expect("accept");
                let ws = accept_async(tcp).await.expect("ws accept");
                let (mut sink, mut stream) = ws.split();
                receive_first_e2ee_wire_message(
                    &mut daemon_replayed,
                    &mut sink,
                    &mut stream,
                    Duration::from_secs(5),
                )
                .await
            });

            let (mut cli_ws, _) = connect_async(format!("ws://{addr}"))
                .await
                .expect("connect");
            cli_ws
                .send(TungMsg::Binary(legit_ct1.clone().into()))
                .await
                .expect("send replayed ct");
            let close_frame = tokio::time::timeout(Duration::from_secs(2), cli_ws.next())
                .await
                .expect("close timeout")
                .expect("frame")
                .expect("ok");
            assert!(
                matches!(close_frame, TungMsg::Close(Some(ref cf)) if cf.code == TungCloseCode::Policy),
                "replayed ciphertext must close with Policy, got {close_frame:?}"
            );
            assert!(srv.await.expect("join").is_err());
        }

        // Case 3: Legitimate client completing handshake & sending encrypted Hello succeeds.
        {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("addr");
            let (client_hs_ok, msg1_ok) =
                ClientHandshake::new(&pub_bytes, &secret).expect("client init ok");
            let (mut daemon_ok, msg2_ok) =
                DaemonHandshake::accept(&priv_bytes, &secret, &msg1_ok).expect("daemon accept ok");
            let mut client_ok = client_hs_ok.finalize(&msg2_ok).expect("finalize ok");
            let ct_ok = client_ok.seal_record(&plain).expect("seal ok");

            let srv = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.expect("accept");
                let ws = accept_async(tcp).await.expect("ws accept");
                let (mut sink, mut stream) = ws.split();
                receive_first_e2ee_wire_message(
                    &mut daemon_ok,
                    &mut sink,
                    &mut stream,
                    Duration::from_secs(5),
                )
                .await
            });

            let (mut cli_ws, _) = connect_async(format!("ws://{addr}"))
                .await
                .expect("connect");
            cli_ws
                .send(TungMsg::Binary(ct_ok.into()))
                .await
                .expect("send valid hello");
            let decoded = srv
                .await
                .expect("join")
                .expect("valid encrypted Hello must succeed");
            assert!(matches!(decoded, WireMessage::Hello { client_id, .. } if client_id == "c1"));
        }
    }

    #[tokio::test]
    async fn search_messages_rpc_queries_daemon_fts5_with_ascii_cjk_and_agent_scoping() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::{
            connect_async,
            tungstenite::{
                Message as TungMsg,
                client::IntoClientRequest,
                http::{HeaderValue, header::AUTHORIZATION},
            },
        };

        let home = std::env::temp_dir().join(format!("orbit_ws_search_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&home).expect("mkdir");
        let (addr, runtime) = spawn_embedded_daemon(home.clone(), None)
            .await
            .expect("spawn daemon");

        let now = chrono::Utc::now();
        runtime
            .store
            .upsert_workspace(&orbit_protocol::WorkspaceSummary {
                workspace_id: "ws_search_rpc".into(),
                title: "Orbit Search RPC".into(),
                cwd: "/workspace/orbit-rpc".into(),
                git_branch: Some("main".into()),
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("upsert workspace");

        let make_agent = |id: &str, title: &str, cwd: &str| orbit_protocol::AgentSummary {
            agent_id: id.into(),
            workspace_id: "ws_search_rpc".into(),
            cwd: cwd.into(),
            title: title.into(),
            provider: orbit_protocol::ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            fast_mode: false,
            mode: orbit_protocol::SessionMode::Default,
            status: orbit_protocol::AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
            timeline_epoch: 1,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: orbit_protocol::AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };

        runtime
            .store
            .upsert_agent(
                &make_agent("ag_s1", "RPC Engine", "/workspace/orbit-rpc"),
                None,
            )
            .expect("ag_s1");
        runtime
            .store
            .upsert_agent(
                &make_agent("ag_s2", "中文检索测试", "/workspace/orbit-cjk"),
                None,
            )
            .expect("ag_s2");

        let ev1 = runtime
            .store
            .append_timeline_event(
                "ag_s1",
                1,
                Some("t_early"),
                orbit_protocol::TimelinePayload::UserPrompt {
                    message_id: "u1".into(),
                    text: "Investigate quasar_rpc_needle on historical turn 1".into(),
                    badges: Vec::new(),
                },
            )
            .expect("ev1");
        let ev2 = runtime
            .store
            .append_timeline_event(
                "ag_s1",
                1,
                Some("t_late"),
                orbit_protocol::TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "Resolved quasar_rpc_needle with FTS5 remote search".into(),
                    streaming: false,
                },
            )
            .expect("ev2");
        let ev3 = runtime
            .store
            .append_timeline_event(
                "ag_s2",
                1,
                Some("t_cjk"),
                orbit_protocol::TimelinePayload::AssistantMessage {
                    item_id: "a2".into(),
                    text: "确认 quasar_rpc_needle 在远程历史全文搜索中生效".into(),
                    streaming: false,
                },
            )
            .expect("ev3");

        let ws_url = format!("ws://{addr}/ws");
        let mut req = ws_url.as_str().into_client_request().expect("ws req");
        req.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", runtime.pairing_secret)).expect("hdr"),
        );
        let (mut ws, _) = connect_async(req).await.expect("connect ws");

        // Complete Hello handshake
        let hello = WireMessage::Hello {
            client_id: "search-test-client".into(),
            client_name: "Test".into(),
            protocol_version: orbit_protocol::PROTOCOL_VERSION,
            supports_zstd: false,
        };
        ws.send(TungMsg::Text(
            serde_json::to_string(&hello).expect("ser hello").into(),
        ))
        .await
        .expect("send hello");
        let _welcome = tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("welcome timeout")
            .expect("welcome frame")
            .expect("welcome ok");

        async fn rpc_search(
            ws: &mut tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            query: &str,
            agent_id: Option<&str>,
            limit: Option<u32>,
        ) -> Vec<orbit_protocol::RemoteMessageSearchHit> {
            let req_id = Uuid::new_v4();
            let msg = WireMessage::SearchMessages {
                request_id: req_id,
                query: query.to_string(),
                agent_id: agent_id.map(str::to_string),
                limit,
            };
            ws.send(TungMsg::Text(
                serde_json::to_string(&msg).expect("ser").into(),
            ))
            .await
            .expect("send search");
            loop {
                let frame = tokio::time::timeout(Duration::from_secs(3), ws.next())
                    .await
                    .expect("resp timeout")
                    .expect("resp stream")
                    .expect("resp ok");
                let text = match frame {
                    TungMsg::Text(t) => t.to_string(),
                    TungMsg::Binary(b) => {
                        match WireMessage::decode_zstd(&b).expect("decode zstd") {
                            WireMessage::SearchMessagesResult { request_id, hits }
                                if request_id == req_id =>
                            {
                                return hits;
                            }
                            _ => continue,
                        }
                    }
                    _ => continue,
                };
                if let Ok(WireMessage::SearchMessagesResult { request_id, hits }) =
                    serde_json::from_str::<WireMessage>(&text)
                    && request_id == req_id
                {
                    return hits;
                }
            }
        }

        // 1. Global search returns 1 best match per conversation (2 total)
        let global_hits = rpc_search(&mut ws, "quasar_rpc_needle", None, Some(20)).await;
        assert_eq!(global_hits.len(), 2);
        assert!(global_hits.iter().any(|h| h.agent_id == "ag_s1"
            && h.agent_title == "RPC Engine"
            && h.cwd == "/workspace/orbit-rpc"));
        assert!(global_hits.iter().any(|h| h.agent_id == "ag_s2"
            && h.agent_title == "中文检索测试"
            && h.cwd == "/workspace/orbit-cjk"));

        // 2. Agent-scoped search returns BOTH events in ag_s1 and none from ag_s2
        let scoped_hits = rpc_search(&mut ws, "quasar_rpc_needle", Some("ag_s1"), Some(50)).await;
        assert_eq!(scoped_hits.len(), 2);
        assert!(scoped_hits.iter().all(|h| h.agent_id == "ag_s1"));
        assert!(
            scoped_hits
                .iter()
                .any(|h| h.seq == ev1.seq && h.turn_id.as_deref() == Some("t_early"))
        );
        assert!(
            scoped_hits
                .iter()
                .any(|h| h.seq == ev2.seq && h.turn_id.as_deref() == Some("t_late"))
        );

        // 3. CJK query matches ag_s2 with highlighted <mark> snippet
        let cjk_hits = rpc_search(&mut ws, "远程历史全文搜索", None, Some(20)).await;
        assert_eq!(cjk_hits.len(), 1);
        assert_eq!(cjk_hits[0].agent_id, "ag_s2");
        assert_eq!(cjk_hits[0].seq, ev3.seq);
        assert!(
            cjk_hits[0]
                .snippet
                .contains("<mark>远程历史全文搜索</mark>")
        );

        // 4. Empty query & limit=0 return empty cleanly
        assert!(rpc_search(&mut ws, "   ", None, Some(10)).await.is_empty());
        assert!(
            rpc_search(&mut ws, "quasar_rpc_needle", None, Some(0))
                .await
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn preview_tunnel_overflow_closes_tunnel_nonblockingly_and_refunds_budget() {
        use base64::Engine as _;

        let session_bytes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (out_tx, mut out_rx) = mpsc::channel::<SessionFrame>(16);

        // 1. Frame queue overflow on a stalled tunnel closes non-blockingly and refunds bytes.
        {
            let tid = Uuid::new_v4();
            let (inbound_tx, inbound_rx) =
                mpsc::channel::<QueuedPreviewBytes>(MAX_PREVIEW_TUNNEL_INBOUND_FRAMES);
            let stalled_task = tokio::spawn(std::future::pending::<()>());
            let mut tunnels = HashMap::new();
            tunnels.insert(
                tid,
                ActivePreviewTunnel {
                    inbound_tx,
                    queued_bytes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                    task: stalled_task.abort_handle(),
                },
            );

            let frame_b64 = base64::engine::general_purpose::STANDARD.encode(b"chunk-16-bytes!!");
            for _ in 0..MAX_PREVIEW_TUNNEL_INBOUND_FRAMES {
                handle_preview_tunnel_data(
                    &mut tunnels,
                    &session_bytes,
                    &out_tx,
                    false,
                    tid,
                    &frame_b64,
                );
            }
            assert!(tunnels.contains_key(&tid));
            assert_eq!(
                session_bytes.load(std::sync::atomic::Ordering::Acquire),
                MAX_PREVIEW_TUNNEL_INBOUND_FRAMES * 16
            );

            // 65th frame MUST NOT block and MUST close & evict only the stalled tunnel.
            handle_preview_tunnel_data(
                &mut tunnels,
                &session_bytes,
                &out_tx,
                false,
                tid,
                &frame_b64,
            );
            assert!(
                !tunnels.contains_key(&tid),
                "overflowed tunnel must be evicted immediately"
            );
            let close_frame = out_rx.try_recv().expect("PreviewTunnelClose emitted");
            match close_frame {
                SessionFrame::Wire {
                    msg:
                        WireMessage::PreviewTunnelClose {
                            tunnel_id,
                            reason: Some(reason),
                        },
                    ..
                } => {
                    assert_eq!(tunnel_id, tid);
                    assert!(reason.contains("overflow"), "unexpected reason: {reason}");
                }
                _ => panic!("expected PreviewTunnelClose wire frame on overflow"),
            }
            // Dropping the stalled receiver refunds every queued byte cleanly.
            drop(inbound_rx);
            assert_eq!(session_bytes.load(std::sync::atomic::Ordering::Acquire), 0);
        }

        // 2. Per-tunnel 4 MiB byte budget overflow closes non-blockingly and refunds bytes.
        {
            let tid = Uuid::new_v4();
            let (inbound_tx, inbound_rx) =
                mpsc::channel::<QueuedPreviewBytes>(MAX_PREVIEW_TUNNEL_INBOUND_FRAMES);
            let stalled_task = tokio::spawn(std::future::pending::<()>());
            let mut tunnels = HashMap::new();
            tunnels.insert(
                tid,
                ActivePreviewTunnel {
                    inbound_tx,
                    queued_bytes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                    task: stalled_task.abort_handle(),
                },
            );

            let big_b64 = base64::engine::general_purpose::STANDARD
                .encode(vec![0xABu8; MAX_PREVIEW_TUNNEL_FRAME_BYTES]);
            let max_frames_in_4mib =
                MAX_PREVIEW_TUNNEL_INBOUND_BYTES / MAX_PREVIEW_TUNNEL_FRAME_BYTES;
            for _ in 0..max_frames_in_4mib {
                handle_preview_tunnel_data(
                    &mut tunnels,
                    &session_bytes,
                    &out_tx,
                    false,
                    tid,
                    &big_b64,
                );
            }
            assert!(tunnels.contains_key(&tid));
            handle_preview_tunnel_data(&mut tunnels, &session_bytes, &out_tx, false, tid, &big_b64);
            assert!(!tunnels.contains_key(&tid));
            let close_frame = out_rx.try_recv().expect("byte-budget close emitted");
            assert!(matches!(
                close_frame,
                SessionFrame::Wire {
                    msg: WireMessage::PreviewTunnelClose { tunnel_id, reason: Some(ref r) },
                    ..
                } if tunnel_id == tid && r.contains("byte buffer overflow")
            ));
            drop(inbound_rx);
            assert_eq!(session_bytes.load(std::sync::atomic::Ordering::Acquire), 0);
        }

        // 3. Empty PreviewTunnelData frame is rejected and closes the tunnel cleanly.
        {
            let tid = Uuid::new_v4();
            let (inbound_tx, _inbound_rx) =
                mpsc::channel::<QueuedPreviewBytes>(MAX_PREVIEW_TUNNEL_INBOUND_FRAMES);
            let stalled_task = tokio::spawn(std::future::pending::<()>());
            let mut tunnels = HashMap::new();
            tunnels.insert(
                tid,
                ActivePreviewTunnel {
                    inbound_tx,
                    queued_bytes: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                    task: stalled_task.abort_handle(),
                },
            );
            handle_preview_tunnel_data(&mut tunnels, &session_bytes, &out_tx, false, tid, "");
            assert!(!tunnels.contains_key(&tid));
            let close_frame = out_rx.try_recv().expect("empty-frame close emitted");
            assert!(matches!(
                close_frame,
                SessionFrame::Wire {
                    msg: WireMessage::PreviewTunnelClose { tunnel_id, reason: Some(ref r) },
                    ..
                } if tunnel_id == tid && r.contains("empty")
            ));
        }
    }
}
