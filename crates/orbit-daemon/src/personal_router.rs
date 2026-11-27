//! Personal Router ("Chief-of-Staff / 分工师") Control Plane, Multi-Host Fleet Router
//! Auto-Discovery, Dynamic Fleet Context Delta Engine, 3-Tier Guardian Pre-Review Gate,
//! and Bidirectional Hermes ACP / MCP / Webhook Interoperability.
//!
//! Security & Architecture Invariants:
//! - Tier-3 (`High` / `Critical`) Guardian requests are ALWAYS fail-closed and hard-blocked from
//!   Personal Router auto-approval.
//! - Zero Python/Node.js runtime dependency inside `orbit`: Hermes is probed and driven as an
//!   optional external ACP agent (or interoperable external Gateway via HTTP MCP + Webhook),
//!   while any native Orbit provider (`claude`, `codex`, `agy`, custom ACP) can also serve as
//!   the Personal Router brain.
//! - Respects `share_host_hermes_memory` (default `true`) so the user's existing `~/.hermes/`
//!   long-term memories & skills seamlessly interoperate with Orbit.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use orbit_protocol::{
    AGENT_ROLE_PERSONAL_ROUTER, AgentRole, AgentStatus, AgentSummary, ConnectedHostSummary,
    FleetProjectEntry, FleetWorkspaceEntry, GuardianAssessment, GuardianRiskLevel,
    HermesBridgeBundle, HostInfo, LABEL_AGENT_ROLE, LABEL_SESSION_TIER, PendingPermissionSummary,
    PersonalRouterConfig, ProjectContextPacket, ProviderKind, RouterHostCapabilities,
    RouterPreReviewMode, RouterScope, RouterStatusSnapshot, SESSION_TIER_PERSONAL_ROUTER,
    SessionMode, WireMessage, WorktreeCreateMode,
};
use orbit_providers::{find_provider_executable, resolve_acp_agent_command};
use orbit_store::OrbitStore;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::connected_hosts::ConnectedHostsManager;
use crate::manager::{AgentOrchestrator, CreateAgentSpec};

const ROUTER_CONFIG_FILE: &str = "personal_router.json";
const MAX_FLEET_WORKERS_IN_CONTEXT: usize = 32;
const MAX_FLEET_WORKSPACES_IN_CONTEXT: usize = 16;
const MAX_FLEET_PROJECTS_IN_CONTEXT: usize = 24;
const REMOTE_FLEET_CACHE_TTL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Default)]
struct CachedRemoteFleetSnapshot {
    fetched_at: Option<std::time::Instant>,
    host_descriptors: BTreeMap<String, String>,
    projects: Vec<FleetProjectEntry>,
    workspaces: Vec<FleetWorkspaceEntry>,
    workers: BTreeMap<String, WorkerContextDigest>,
    pending_perms: Vec<String>,
}

/// Persisted on-disk wrapper in `$ORBIT_HOME/personal_router.json` (`0o600`),
/// keeping `external_webhook_secret` strictly out of wire-serialized [`PersonalRouterConfig`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistedRouterFile {
    #[serde(default)]
    config: PersonalRouterConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_webhook_secret: Option<String>,
}

/// Compact per-worker fingerprint used to compute incremental `<orbit-fleet-delta>` updates.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkerContextDigest {
    agent_id: String,
    handle: String,
    title: String,
    workspace_id: String,
    provider: String,
    status: String,
    task_summary: String,
    pending_perms: usize,
}

#[derive(Debug, Clone, Default)]
struct FleetContextState {
    revision: u64,
    hosts: BTreeMap<String, String>,
    projects: BTreeMap<String, String>,
    workspaces: BTreeMap<String, String>,
    workers: BTreeMap<String, WorkerContextDigest>,
}

/// Host-local manager for the Personal Router role & multi-host Fleet discovery.
pub struct PersonalRouterManager {
    home_dir: PathBuf,
    host_info: HostInfo,
    listen_port: Arc<RwLock<u16>>,
    mcp_token: String,
    store: Arc<OrbitStore>,
    orchestrator: Arc<AgentOrchestrator>,
    connected_hosts: Arc<ConnectedHostsManager>,
    state: RwLock<PersistedRouterFile>,
    fleet_ctx_state: RwLock<Option<FleetContextState>>,
    remote_fleet_cache: RwLock<CachedRemoteFleetSnapshot>,
}

impl PersonalRouterManager {
    pub fn new(
        home_dir: PathBuf,
        host_info: HostInfo,
        listen_port: Arc<RwLock<u16>>,
        mcp_token: String,
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        connected_hosts: Arc<ConnectedHostsManager>,
    ) -> Arc<Self> {
        let mut persisted = load_router_file(&home_dir);
        persisted.config.has_external_webhook_secret = persisted
            .external_webhook_secret
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty());
        Arc::new(Self {
            home_dir,
            host_info,
            listen_port,
            mcp_token,
            store,
            orchestrator,
            connected_hosts,
            state: RwLock::new(persisted),
            fleet_ctx_state: RwLock::new(None),
            remote_fleet_cache: RwLock::new(CachedRemoteFleetSnapshot::default()),
        })
    }

    pub async fn config(&self) -> PersonalRouterConfig {
        self.state.read().await.config.clone()
    }

    /// Build the authoritative persistent system prompt for launching or resuming a Personal Router session.
    pub async fn router_system_prompt_for_launch(&self) -> String {
        let cfg = self.state.read().await.config.clone();
        build_router_system_instructions(&cfg)
    }

    pub async fn active_router_agent_id(&self) -> Option<String> {
        let cfg = self.state.read().await.config.clone();
        if !cfg.enabled {
            return None;
        }
        let aid = cfg.router_agent_id?;
        match self.store.get_agent(&aid).ok().flatten() {
            Some(ag) if ag.archived_at.is_none() => Some(ag.agent_id),
            _ => None,
        }
    }

    /// Return `true` if `caller_agent_id` is an active Personal Router with authority over `is_remote`.
    pub async fn is_authorized_global_router(
        &self,
        caller_agent_id: &str,
        is_remote: bool,
    ) -> bool {
        let cfg = self.state.read().await.config.clone();
        if !cfg.enabled {
            return false;
        }
        if is_remote && cfg.scope != RouterScope::GlobalFleet {
            return false;
        }
        if cfg.router_agent_id.as_deref() == Some(caller_agent_id) {
            return true;
        }
        self.store
            .get_agent(caller_agent_id)
            .ok()
            .flatten()
            .is_some_and(|ag| ag.archived_at.is_none() && ag.is_personal_router())
    }

    /// Enforce the 3-tier Guardian + Router Pre-Review authority boundary when a Personal Router
    /// calls `respond_to_permission` on behalf of a Worker.
    ///
    /// - `High` / `Critical` requests are ALWAYS hard-blocked from Router `allow`/`allow_always`.
    /// - `RouterPreReviewMode::Disabled` and `AssistOnly` forbid direct Router permission resolution.
    /// - `deny` is allowed in `MediumRiskOnly` even for elevated risks so the Router can protect the user.
    pub async fn validate_router_permission_decision(
        &self,
        decision: &str,
        assessment: Option<&GuardianAssessment>,
    ) -> Result<(), String> {
        let cfg = self.state.read().await.config.clone();
        match cfg.pre_review_mode {
            RouterPreReviewMode::Disabled => {
                return Err(
                    "Personal Router pre-review is disabled; escalate this permission request to the human."
                        .to_string(),
                );
            }
            RouterPreReviewMode::AssistOnly => {
                return Err(
                    "Personal Router is in `assist_only` pre-review mode; summarize context and wait for the human to approve or deny."
                        .to_string(),
                );
            }
            RouterPreReviewMode::MediumRiskOnly => {}
        }

        let is_allow = matches!(decision, "allow" | "allow_always");
        if !is_allow {
            return Ok(());
        }

        if let Some(a) = assessment
            && crate::guardian::assessment_requires_human_only(a)
        {
            return Err(
                "Fail-closed or human-only Guardian assessment cannot be auto-approved by Personal Router; human confirmation via interactive approval card is required."
                    .to_string(),
            );
        }

        match assessment.map(|a| a.risk_level) {
            Some(GuardianRiskLevel::Low | GuardianRiskLevel::Medium) => Ok(()),
            Some(GuardianRiskLevel::High | GuardianRiskLevel::Critical) => Err(
                "Tier-3 High/Critical security-boundary operation cannot be auto-approved by Personal Router; human confirmation via interactive approval card is required."
                    .to_string(),
            ),
            None => Err(
                "Permission request lacks a Low/Medium Guardian risk assessment; fail-closed requires human approval."
                    .to_string(),
            ),
        }
    }

    /// Update Personal Router configuration (and optionally create/activate the canonical Router session).
    pub async fn configure(
        self: &Arc<Self>,
        mut new_config: PersonalRouterConfig,
        external_webhook_secret: Option<String>,
        activate: bool,
    ) -> anyhow::Result<RouterStatusSnapshot> {
        {
            let mut guard = self.state.write().await;
            if let Some(secret) = external_webhook_secret {
                let trimmed = secret.trim().to_string();
                guard.external_webhook_secret = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                };
            }
            new_config.has_external_webhook_secret = guard
                .external_webhook_secret
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty());
            // Preserve existing canonical router_agent_id if caller omitted it while staying enabled
            if new_config.router_agent_id.is_none() {
                new_config.router_agent_id = guard.config.router_agent_id.clone();
            }
            guard.config = new_config.clone();
            save_router_file(&self.home_dir, &guard)?;
        }

        // Reset incremental fleet delta & remote cache whenever config changes so next turn sends a fresh full snapshot.
        self.reset_fleet_context_state().await;

        if activate && new_config.enabled {
            let _ = self.ensure_router_agent().await?;
        }

        Ok(self.status_snapshot().await)
    }

    /// Ensure the host's canonical `PersonalRouter` conversation exists and is ready.
    pub async fn ensure_router_agent(&self) -> anyhow::Result<AgentSummary> {
        let cfg = self.state.read().await.config.clone();
        // Determine effective provider (fall back gracefully if configured Hermes ACP isn't installed yet)
        let (provider, acp_agent_id) = resolve_effective_router_provider(&cfg)?;

        if let Some(ref existing_id) = cfg.router_agent_id
            && let Ok(Some(mut ag)) = self.store.get_agent(existing_id)
            && ag.archived_at.is_none()
        {
            let engine_matches = ag.provider == provider
                && (provider != ProviderKind::Acp || ag.acp_agent_id == acp_agent_id);
            if engine_matches {
                if !ag.is_personal_router() || !ag.is_pinned {
                    ag.set_role(AgentRole::PersonalRouter);
                    ag.is_pinned = true;
                    let _ = self.store.upsert_agent(&ag, None);
                }
                return Ok(ag);
            }
            // Configured engine changed: retire the previous canonical Router conversation cleanly
            // before spawning a new one on the newly selected provider/harness.
            let _ = self.orchestrator.archive_agent(&ag.agent_id).await;
            let mut guard = self.state.write().await;
            if guard.config.router_agent_id.as_deref() == Some(ag.agent_id.as_str()) {
                guard.config.router_agent_id = None;
                let _ = save_router_file(&self.home_dir, &guard);
            }
        }
        let cwd = cfg
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| resolve_user_home().to_string_lossy().to_string());

        let mut init_labels = HashMap::new();
        init_labels.insert(
            LABEL_AGENT_ROLE.to_string(),
            AGENT_ROLE_PERSONAL_ROUTER.to_string(),
        );
        init_labels.insert(
            LABEL_SESSION_TIER.to_string(),
            SESSION_TIER_PERSONAL_ROUTER.to_string(),
        );
        init_labels.insert(
            crate::workspace_organizer::LABEL_TITLE_LOCKED.to_string(),
            "1".to_string(),
        );

        let persisted_default_mode = crate::DaemonPersistedConfig::load(&self.home_dir)
            .orchestration
            .default_session_mode
            .filter(|m| !matches!(m, SessionMode::Plan));
        let created = self
            .orchestrator
            .create_and_start_agent_with_labels(
                build_router_create_spec(&cfg, provider, acp_agent_id, cwd, persisted_default_mode),
                init_labels,
            )
            .await?;

        // Mark canonical role, pin at top, and lock title
        let mut updated = self
            .store
            .get_agent(&created.agent_id)
            .ok()
            .flatten()
            .unwrap_or(created);
        updated.set_role(AgentRole::PersonalRouter);
        updated.is_pinned = true;
        updated.pinned_order = i64::MAX / 2;
        updated.labels.insert(
            crate::workspace_organizer::LABEL_TITLE_LOCKED.to_string(),
            "1".to_string(),
        );
        let _ = self.store.upsert_agent(&updated, None);

        {
            let mut guard = self.state.write().await;
            guard.config.enabled = true;
            guard.config.router_agent_id = Some(updated.agent_id.clone());
            let _ = save_router_file(&self.home_dir, &guard);
        }

        info!(
            agent_id = %updated.agent_id,
            provider = ?updated.provider,
            acp_agent_id = ?updated.acp_agent_id,
            "Activated canonical Personal Router session"
        );
        Ok(updated)
    }

    /// Build the full live status snapshot for this host.
    pub async fn status_snapshot(&self) -> RouterStatusSnapshot {
        let config = self.state.read().await.config.clone();
        let host_capabilities = self.probe_local_capabilities().await;
        let router_agent = config
            .router_agent_id
            .as_deref()
            .and_then(|id| self.store.get_agent(id).ok().flatten())
            .filter(|a| a.archived_at.is_none());

        let workers = self
            .store
            .list_agents_filtered(None, false, false, None)
            .unwrap_or_default();
        let managed_workers_count = workers
            .iter()
            .filter(|a| a.is_primary_conversation())
            .count();
        let pending_approvals_count = self.orchestrator.list_pending_permissions(None).await.len();

        RouterStatusSnapshot {
            config,
            host_capabilities,
            router_agent,
            managed_workers_count,
            pending_approvals_count,
        }
    }

    /// Probe local host readiness for running Hermes or fallback Personal Router backends.
    pub async fn probe_local_capabilities(&self) -> RouterHostCapabilities {
        let cfg = self.state.read().await.config.clone();
        let active_router = cfg
            .router_agent_id
            .as_deref()
            .and_then(|id| self.store.get_agent(id).ok().flatten())
            .filter(|a| a.archived_at.is_none());

        let user_home = resolve_user_home();
        let (hermes_bin, hermes_version, hermes_has_memory, hermes_gateway_running, providers) =
            tokio::task::spawn_blocking(move || {
                let hermes_bin = find_hermes_executable(&user_home);
                let hermes_version = hermes_bin.as_deref().and_then(probe_hermes_version);
                let hermes_has_memory = detect_hermes_existing_memory(&user_home);
                let hermes_gateway_running = detect_hermes_gateway_running(&user_home);
                let mut providers = Vec::new();
                for kind in [
                    ProviderKind::ClaudeCode,
                    ProviderKind::Codex,
                    ProviderKind::Antigravity,
                ] {
                    if find_provider_executable(kind).is_some() {
                        providers.push(kind);
                    }
                }
                (
                    hermes_bin,
                    hermes_version,
                    hermes_has_memory,
                    hermes_gateway_running,
                    providers,
                )
            })
            .await
            .unwrap_or_else(|_| (None, None, false, false, Vec::new()));

        let hermes_installed = hermes_bin.is_some();
        let hermes_acp_ready = hermes_installed;
        let (score, reason) = compute_recommendation_score(
            true,
            hermes_installed,
            hermes_has_memory,
            active_router.is_some(),
            !providers.is_empty(),
        );

        RouterHostCapabilities {
            host_id: self.host_info.host_id.clone(),
            host_name: self.host_info.hostname.clone(),
            is_local: true,
            online: true,
            hermes_installed,
            hermes_version,
            hermes_has_existing_memory: hermes_has_memory,
            hermes_acp_ready,
            hermes_gateway_running,
            available_providers: providers,
            active_router_agent_id: active_router.as_ref().map(|a| a.agent_id.clone()),
            active_router_status: active_router.map(|a| a.status),
            router_enabled: cfg.enabled,
            recommended_score: score,
            recommendation_reason: Some(reason),
        }
    }

    /// Auto-discover Personal Router & Hermes readiness across the local host AND all connected
    /// remote hosts in the Multi-Host Fleet concurrently, sorted by `recommended_score` descending.
    pub async fn discover_fleet_router_hosts(&self) -> Vec<RouterHostCapabilities> {
        let local_fut = self.probe_local_capabilities();
        let remote_hosts = self.connected_hosts.list_summaries().await;

        let mut remote_tasks = Vec::with_capacity(remote_hosts.len());
        for host in remote_hosts {
            let connected = Arc::clone(&self.connected_hosts);
            remote_tasks.push(tokio::spawn(async move {
                probe_remote_host_capabilities(connected, host).await
            }));
        }

        let mut results = vec![local_fut.await];
        for task in remote_tasks {
            if let Ok(cap) = task.await {
                results.push(cap);
            }
        }

        results.sort_by(|a, b| {
            b.recommended_score
                .cmp(&a.recommended_score)
                .then_with(|| b.online.cmp(&a.online))
                .then_with(|| a.host_name.cmp(&b.host_name))
        });
        results
    }

    /// List discovered projects on the local host, a specific remote host, or ALL online hosts
    /// in the Multi-Host Fleet (`target_host_id == Some("all")` or `None` when `GlobalFleet`).
    pub async fn list_fleet_projects(
        &self,
        target_host_id: Option<&str>,
    ) -> Vec<FleetProjectEntry> {
        let include_all =
            target_host_id.is_none() || matches!(target_host_id, Some("all") | Some("*"));
        let mut out = Vec::new();

        // Local projects
        if target_host_id.is_none() || include_all {
            let store = Arc::clone(&self.store);
            let local_host_id = self.host_info.host_id.clone();
            let local_host_name = self.host_info.hostname.clone();
            if let Ok(discovered) = tokio::task::spawn_blocking(move || {
                crate::discovery::discover_host_projects(&store)
            })
            .await
            {
                for p in discovered.into_iter().take(MAX_FLEET_PROJECTS_IN_CONTEXT) {
                    out.push(FleetProjectEntry {
                        host_id: local_host_id.clone(),
                        host_name: local_host_name.clone(),
                        is_local: true,
                        name: p.name,
                        cwd: p.path,
                        git_branch: p.git_branch,
                    });
                }
            }
        }

        // Remote projects
        let remote_targets: Vec<ConnectedHostSummary> = if include_all {
            self.connected_hosts
                .list_summaries()
                .await
                .into_iter()
                .filter(|h| h.enabled)
                .collect()
        } else if let Some(rid) = target_host_id {
            self.connected_hosts
                .list_summaries()
                .await
                .into_iter()
                .filter(|h| {
                    h.enabled && (h.host_id.as_deref() == Some(rid) || h.registration_id == rid)
                })
                .collect()
        } else {
            Vec::new()
        };

        let mut tasks = Vec::with_capacity(remote_targets.len());
        for host in remote_targets {
            let connected = Arc::clone(&self.connected_hosts);
            tasks.push(tokio::spawn(async move {
                fetch_remote_host_projects(connected, host).await
            }));
        }
        for t in tasks {
            if let Ok(mut items) = t.await {
                out.append(&mut items);
            }
        }
        out
    }

    /// Inspect a project's concise context (`AGENTS.md`, `README.md`, git branch, top-level files,
    /// active workspaces & active workers) on the local host OR any connected remote host!
    pub async fn inspect_project_context(
        &self,
        target_host_id: Option<&str>,
        cwd: &str,
    ) -> anyhow::Result<ProjectContextPacket> {
        if let Some(remote_id) = target_host_id {
            let req_id = Uuid::new_v4();
            match self
                .connected_hosts
                .send_rpc(
                    remote_id,
                    req_id,
                    WireMessage::InspectProjectContext {
                        request_id: req_id,
                        cwd: cwd.to_string(),
                    },
                    Duration::from_secs(8),
                )
                .await?
            {
                WireMessage::ProjectContextResponse { context, .. } => Ok(context),
                WireMessage::RpcError { message, .. } => {
                    anyhow::bail!(
                        "remote host `{remote_id}` inspect_project_context failed: {message}"
                    )
                }
                _ => anyhow::bail!("unexpected remote reply for inspect_project_context"),
            }
        } else {
            self.inspect_local_project_context(cwd).await
        }
    }

    /// Build a token-bounded [`ProjectContextPacket`] for a directory on THIS host.
    pub async fn inspect_local_project_context(
        &self,
        cwd: &str,
    ) -> anyhow::Result<ProjectContextPacket> {
        let requested = cwd.trim().to_string();
        if requested.is_empty() {
            anyhow::bail!("`cwd` is required for inspect_project_context");
        }
        let host_id = self.host_info.host_id.clone();
        let host_name = self.host_info.hostname.clone();
        let store = Arc::clone(&self.store);

        tokio::task::spawn_blocking(move || {
            let expanded = if let Some(rest) = requested.strip_prefix("~/") {
                resolve_user_home().join(rest)
            } else {
                PathBuf::from(&requested)
            };
            let canonical = crate::platform::canonicalize_clean(&expanded).unwrap_or(expanded);
            if !canonical.is_dir() {
                anyhow::bail!(
                    "project directory `{}` does not exist on host `{host_name}`",
                    canonical.display()
                );
            }
            let canon_str = canonical.to_string_lossy().to_string();
            let project_name = canonical
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
                .to_string();

            // Git branch & dirty state
            let git_branch = std::process::Command::new("git")
                .args(["-C", &canon_str, "rev-parse", "--abbrev-ref", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .filter(|s| !s.is_empty());
            let git_dirty = std::process::Command::new("git")
                .args(["-C", &canon_str, "status", "--porcelain"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .is_some_and(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty());

            // Top-level entries (capped at 40)
            let mut top_level_entries = Vec::new();
            if let Ok(rd) = std::fs::read_dir(&canonical) {
                for entry in rd.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if matches!(
                        name.as_str(),
                        ".git" | "node_modules" | "target" | ".DS_Store"
                    ) {
                        continue;
                    }
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    top_level_entries.push(if is_dir { format!("{name}/") } else { name });
                }
            }
            top_level_entries.sort();
            top_level_entries.truncate(40);

            // Read concise AGENTS.md / CLAUDE.md and README.md excerpts
            let instructions_excerpt = ["AGENTS.md", "CLAUDE.md", ".orbit/AGENTS.md"]
                .iter()
                .find_map(|rel| read_excerpt(&canonical.join(rel), 2400));
            let readme_excerpt = ["README.md", "readme.md"]
                .iter()
                .find_map(|rel| read_excerpt(&canonical.join(rel), 1600));

            // Matching active workspaces & workers on this host
            let active_workspaces: Vec<String> = store
                .list_workspaces_filtered(false, false)
                .unwrap_or_default()
                .into_iter()
                .filter(|ws| {
                    ws.cwd == canon_str || ws.project_root.as_deref() == Some(canon_str.as_str())
                })
                .take(8)
                .map(|ws| {
                    format!(
                        "{} ({}, board_rev={})",
                        ws.title, ws.workspace_id, ws.board_revision
                    )
                })
                .collect();
            let active_workers: Vec<String> = store
                .list_agents_filtered(None, false, false, None)
                .unwrap_or_default()
                .into_iter()
                .filter(|a| a.is_primary_conversation() && a.cwd == canon_str)
                .take(12)
                .map(|a| {
                    let h = a
                        .handle
                        .as_deref()
                        .map(|s| format!("@{s}"))
                        .unwrap_or_else(|| a.agent_id.clone());
                    let summary = a
                        .task_summary
                        .as_deref()
                        .map(|s| compact_inline(s, 72))
                        .unwrap_or_default();
                    format!(
                        "{h} ({}) [{}] {summary}",
                        a.agent_id,
                        format_compact_agent_status(&a.status)
                    )
                })
                .collect();

            Ok(ProjectContextPacket {
                host_id,
                host_name,
                cwd: canon_str,
                project_name,
                git_branch,
                git_dirty,
                top_level_entries,
                instructions_excerpt,
                readme_excerpt,
                active_workspaces,
                active_workers,
            })
        })
        .await
        .map_err(|e| anyhow::anyhow!("inspect_local_project_context task failed: {e}"))?
    }

    async fn get_cached_remote_fleet_snapshot(
        &self,
        connected: &[ConnectedHostSummary],
    ) -> CachedRemoteFleetSnapshot {
        {
            let guard = self.remote_fleet_cache.read().await;
            if let Some(ts) = guard.fetched_at
                && ts.elapsed() < REMOTE_FLEET_CACHE_TTL
            {
                return guard.clone();
            }
        }
        let mut host_tasks = Vec::new();
        for host in connected.iter().filter(|h| h.enabled) {
            let c = Arc::clone(&self.connected_hosts);
            let h = host.clone();
            host_tasks.push(tokio::spawn(async move {
                fetch_remote_host_atlas_bundle(c, h).await
            }));
        }
        let mut snap = CachedRemoteFleetSnapshot {
            fetched_at: Some(std::time::Instant::now()),
            ..CachedRemoteFleetSnapshot::default()
        };
        for t in host_tasks {
            if let Ok((hid, host_desc, mut projs, mut wss, map, mut perms)) = t.await {
                if let Some(desc) = host_desc {
                    snap.host_descriptors.insert(hid, desc);
                }
                snap.projects.append(&mut projs);
                snap.workspaces.append(&mut wss);
                snap.workers.extend(map);
                snap.pending_perms.append(&mut perms);
            }
        }
        let mut guard = self.remote_fleet_cache.write().await;
        *guard = snap.clone();
        snap
    }

    pub async fn invalidate_remote_fleet_cache(&self) {
        self.remote_fleet_cache.write().await.fetched_at = None;
    }

    pub async fn reset_fleet_context_state(&self) {
        *self.fleet_ctx_state.write().await = None;
        self.remote_fleet_cache.write().await.fetched_at = None;
    }

    /// Build either a full `<orbit-fleet-state rev="N">` (first turn / after reset) or a compact
    /// `<orbit-fleet-delta rev="N">` (subsequent turns) to prepend to Personal Router prompts.
    ///
    /// Aggregates all local + connected remote hosts, their OS/provider capabilities, active Git
    /// projects, curated Mission Workspaces (with current `Goal`), and primary user-visible
    /// `Conversation`s (strictly excluding internal `Subagent`s).
    pub async fn build_fleet_context_incremental(&self, router_agent_id: &str) -> Option<String> {
        let cfg = self.state.read().await.config.clone();
        let connected = if cfg.scope == RouterScope::GlobalFleet {
            self.connected_hosts.list_summaries().await
        } else {
            Vec::new()
        };
        let workspaces = self
            .store
            .list_workspaces_filtered(false, false)
            .unwrap_or_default();
        let agents = self
            .store
            .list_agents_filtered(None, false, false, None)
            .unwrap_or_default();
        let pending_perms = self.orchestrator.list_pending_permissions(None).await;
        let profiles = self.store.list_profiles().unwrap_or_default();

        let mut perm_counts: HashMap<String, usize> = HashMap::new();
        for p in &pending_perms {
            *perm_counts.entry(p.agent_id.clone()).or_insert(0) += 1;
        }

        let mut cur_hosts = BTreeMap::new();
        cur_hosts.insert(
            self.host_info.host_id.clone(),
            format!(
                "{} (local, online, {}/{})",
                self.host_info.hostname, self.host_info.os, self.host_info.arch
            ),
        );
        for h in &connected {
            let hid = h
                .host_id
                .clone()
                .unwrap_or_else(|| h.registration_id.clone());
            let status_str = if h.enabled {
                format!("{:?}", h.status).to_ascii_lowercase()
            } else {
                "disabled".to_string()
            };
            cur_hosts.insert(hid, format!("{} (remote, {status_str})", h.label));
        }

        let mut cur_projects = BTreeMap::new();
        let mut total_local_projects = 0usize;
        {
            let store_clone = Arc::clone(&self.store);
            let self_hid = self.host_info.host_id.clone();
            if let Ok(local_projs) = tokio::task::spawn_blocking(move || {
                crate::discovery::discover_host_projects(&store_clone)
            })
            .await
            {
                total_local_projects = local_projs.len();
                for p in local_projs.into_iter().take(12) {
                    let key = format!("{self_hid}:{}", p.path);
                    let br = p.git_branch.unwrap_or_else(|| "-".into());
                    cur_projects.insert(
                        key,
                        format!(
                            "[local] {} | cwd={} | branch={}",
                            compact_inline(&p.name, 36),
                            compact_inline(&p.path, 72),
                            br
                        ),
                    );
                }
            }
        }

        let mut cur_workspaces = BTreeMap::new();
        for ws in workspaces
            .iter()
            .filter(|w| w.is_curated)
            .take(MAX_FLEET_WORKSPACES_IN_CONTEXT)
        {
            let goal_part = ws
                .mission_goal
                .as_deref()
                .map(str::trim)
                .filter(|g| !g.is_empty())
                .map(|g| format!(" | goal={}", compact_inline(g, 64)))
                .unwrap_or_default();
            cur_workspaces.insert(
                format!("[local] {}", ws.workspace_id),
                format!(
                    "{} | cwd={} | board_rev={}{}",
                    compact_inline(&ws.title, 48),
                    compact_inline(&ws.cwd, 64),
                    ws.board_revision,
                    goal_part
                ),
            );
        }

        let mut cur_workers = BTreeMap::new();
        for ag in agents
            .iter()
            .filter(|a| a.agent_id != router_agent_id && a.is_primary_conversation())
            .take(MAX_FLEET_WORKERS_IN_CONTEXT)
        {
            let handle = ag
                .handle
                .as_deref()
                .filter(|h| !h.trim().is_empty())
                .map(|h| format!("@{h}"))
                .unwrap_or_else(|| ag.agent_id.clone());
            let status = format_compact_agent_status(&ag.status);
            let summary = ag
                .task_summary
                .as_deref()
                .map(|s| compact_inline(s, 80))
                .unwrap_or_default();
            let pending = perm_counts.get(&ag.agent_id).copied().unwrap_or(0);
            cur_workers.insert(
                ag.agent_id.clone(),
                WorkerContextDigest {
                    agent_id: ag.agent_id.clone(),
                    handle,
                    title: compact_inline(&ag.title, 48),
                    workspace_id: ag.workspace_id.clone(),
                    provider: ag.provider.display_name().to_string(),
                    status,
                    task_summary: summary,
                    pending_perms: pending,
                },
            );
        }

        let mut remote_perm_lines: Vec<String> = Vec::new();
        let mut total_remote_projects: usize = 0;
        if cfg.scope == RouterScope::GlobalFleet && !connected.is_empty() {
            let remote_snap = self.get_cached_remote_fleet_snapshot(&connected).await;
            for (hid, desc) in remote_snap.host_descriptors {
                cur_hosts.insert(hid, desc);
            }
            total_remote_projects = remote_snap.projects.len();
            for rp in remote_snap
                .projects
                .into_iter()
                .take(MAX_FLEET_PROJECTS_IN_CONTEXT)
            {
                let key = format!("{}:{}", rp.host_id, rp.cwd);
                let br = rp.git_branch.unwrap_or_else(|| "-".into());
                cur_projects.insert(
                    key,
                    format!(
                        "[host={}] {} | cwd={} | branch={}",
                        rp.host_id,
                        compact_inline(&rp.name, 36),
                        compact_inline(&rp.cwd, 72),
                        br
                    ),
                );
            }
            for rws in remote_snap.workspaces {
                if cur_workspaces.len() >= MAX_FLEET_WORKSPACES_IN_CONTEXT * 2 {
                    break;
                }
                let goal_part = rws
                    .mission_goal
                    .as_deref()
                    .map(str::trim)
                    .filter(|g| !g.is_empty())
                    .map(|g| format!(" | goal={}", compact_inline(g, 64)))
                    .unwrap_or_default();
                cur_workspaces.insert(
                    format!("[host={}] {}", rws.host_id, rws.workspace_id),
                    format!(
                        "{} | cwd={} | board_rev={}{}",
                        compact_inline(&rws.title, 48),
                        compact_inline(&rws.cwd, 64),
                        rws.board_revision,
                        goal_part
                    ),
                );
            }
            for (k, rw) in remote_snap.workers {
                if cur_workers.len() >= MAX_FLEET_WORKERS_IN_CONTEXT {
                    break;
                }
                cur_workers.insert(k, rw);
            }
            remote_perm_lines = remote_snap.pending_perms;
        }

        let mut guard = self.fleet_ctx_state.write().await;
        let next_rev = guard.as_ref().map(|s| s.revision + 1).unwrap_or(1);

        let xml = match guard.as_ref() {
            None => {
                // Full compact snapshot on Turn 1 (or after reset)
                let mut out = format!(
                    "<orbit-fleet-state rev=\"{next_rev}\" scope=\"{:?}\" pre_review=\"{:?}\">\n",
                    cfg.scope, cfg.pre_review_mode
                );
                out.push_str("Hosts:\n");
                for (id, desc) in &cur_hosts {
                    out.push_str(&format!("- {id}: {desc}\n"));
                }
                if !cur_projects.is_empty() {
                    out.push_str("Projects:\n");
                    for desc in cur_projects.values().take(MAX_FLEET_PROJECTS_IN_CONTEXT) {
                        out.push_str(&format!("- {desc}\n"));
                    }
                    let hidden_local = total_local_projects.saturating_sub(12);
                    let hidden_remote =
                        total_remote_projects.saturating_sub(MAX_FLEET_PROJECTS_IN_CONTEXT);
                    let hidden_total = cur_projects
                        .len()
                        .saturating_sub(MAX_FLEET_PROJECTS_IN_CONTEXT)
                        .saturating_add(hidden_local)
                        .saturating_add(hidden_remote);
                    if hidden_total > 0 {
                        out.push_str(&format!(
                            "- (+{hidden_total} more projects omitted; call `orbit_control(action=\"list_projects\", host_id=\"all\")` before concluding a repo does not exist)\n"
                        ));
                    }
                }
                if !cur_workspaces.is_empty() {
                    out.push_str("Workspaces:\n");
                    for (id, desc) in &cur_workspaces {
                        out.push_str(&format!("- {id}: {desc}\n"));
                    }
                }
                if cur_workers.is_empty() {
                    out.push_str("ActiveWorkers: (none)\n");
                } else {
                    out.push_str("ActiveWorkers:\n");
                    for w in cur_workers.values() {
                        out.push_str(&format_worker_line(w));
                    }
                }
                if !profiles.is_empty() {
                    out.push_str("Profiles:\n");
                    for p in profiles.iter().take(12) {
                        out.push_str(&format!(
                            "- {} ({}, {}): {}\n",
                            p.profile_id,
                            p.name,
                            p.provider.display_name(),
                            compact_inline(&p.when_to_use, 80)
                        ));
                    }
                }
                if !pending_perms.is_empty() || !remote_perm_lines.is_empty() {
                    out.push_str("PendingPermissions:\n");
                    for p in pending_perms.iter().take(10) {
                        out.push_str(&format_pending_perm_line(p));
                    }
                    for rp in remote_perm_lines.iter().take(10) {
                        out.push_str(rp);
                    }
                }
                out.push_str("</orbit-fleet-state>");
                Some(out)
            }
            Some(prev) => {
                let mut lines = Vec::new();
                for (id, desc) in &cur_hosts {
                    if prev.hosts.get(id) != Some(desc) {
                        lines.push(format!("+ host {id}: {desc}"));
                    }
                }
                for (k, desc) in &cur_projects {
                    if prev.projects.get(k) != Some(desc) {
                        lines.push(format!("+ project {desc}"));
                    }
                }
                for (id, desc) in &cur_workspaces {
                    if prev.workspaces.get(id) != Some(desc) {
                        lines.push(format!("~ workspace {id}: {desc}"));
                    }
                }
                for (id, w) in &cur_workers {
                    if prev.workers.get(id) != Some(w) {
                        lines.push(format!(
                            "~ worker {}",
                            format_worker_line(w).trim_start_matches("- ").trim_end()
                        ));
                    }
                }
                for id in prev.workers.keys() {
                    if !cur_workers.contains_key(id) {
                        lines.push(format!("- worker {id} (archived/closed)"));
                    }
                }
                if !pending_perms.is_empty() {
                    for p in pending_perms.iter().take(6) {
                        lines.push(format!(
                            "! pending_perm {}",
                            format_pending_perm_line(p)
                                .trim_start_matches("- ")
                                .trim_end()
                        ));
                    }
                }
                for rp in remote_perm_lines.iter().take(6) {
                    lines.push(format!(
                        "! pending_perm {}",
                        rp.trim_start_matches("- ").trim_end()
                    ));
                }
                if lines.is_empty() {
                    None
                } else {
                    Some(format!(
                        "<orbit-fleet-delta rev=\"{next_rev}\">\n{}\n</orbit-fleet-delta>",
                        lines.join("\n")
                    ))
                }
            }
        };

        *guard = Some(FleetContextState {
            revision: next_rev,
            hosts: cur_hosts,
            projects: cur_projects,
            workspaces: cur_workspaces,
            workers: cur_workers,
        });

        xml
    }

    /// Generate ready-to-paste configuration snippets for connecting an external Hermes Gateway
    /// to this Orbit host via Streamable HTTP MCP and Router SOUL/Skill templates.
    pub async fn export_hermes_bridge_bundle(&self) -> HermesBridgeBundle {
        let port = *self.listen_port.read().await;
        let effective_port = if port > 0 { port } else { 6868 };
        let caller_id = self
            .active_router_agent_id()
            .await
            .unwrap_or_else(|| "external_hermes_router".to_string());
        let mcp_http_url =
            format!("http://127.0.0.1:{effective_port}/mcp/agents?caller_agent_id={caller_id}");
        let cfg = self.state.read().await.config.clone();

        let mcp_yaml_snippet = format!(
            "# Add to ~/.hermes/config.yaml to let Hermes orchestrate Orbit Workers & Workspaces\n\
             mcp_servers:\n\
               orbit:\n\
                 url: \"{mcp_http_url}\"\n\
                 transport: \"streamable_http\"\n\
                 headers:\n\
                   Authorization: \"Bearer {}\"\n",
            self.mcp_token
        );

        let acp_toolset_yaml_snippet = "\
# Recommended ~/.hermes/config.yaml toolset scoping for Orbit Personal Router mode:\n\
# Keeps Hermes lightweight & non-blocking (memory + skills + Orbit MCP only; heavy coding goes to Orbit Workers)\n\
platform_toolsets:\n\
  acp:\n\
    - memory\n\
    - session_search\n\
    - skills\n\
    - mcp-orbit\n"
            .to_string();

        let soul_md_template = build_router_system_instructions(&cfg);
        let skill_md_template = ORBIT_ROUTER_SKILL_TEMPLATE.to_string();

        HermesBridgeBundle {
            host_id: self.host_info.host_id.clone(),
            host_name: self.host_info.hostname.clone(),
            mcp_http_url,
            mcp_yaml_snippet,
            acp_toolset_yaml_snippet,
            soul_md_template,
            skill_md_template,
        }
    }

    /// Best-effort asynchronous push of Worker lifecycle events to an optional external Hermes
    /// Inbound Webhook (used when the user runs Hermes Gateway externally alongside Orbit MCP).
    pub fn emit_external_webhook_event(
        self: &Arc<Self>,
        event_type: &'static str,
        payload: serde_json::Value,
    ) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let (url_opt, secret_opt) = {
                let guard = this.state.read().await;
                (
                    guard.config.external_webhook_url.clone(),
                    guard.external_webhook_secret.clone(),
                )
            };
            let Some(url) = url_opt
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
            else {
                return;
            };
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return;
            }
            let body = serde_json::json!({
                "event": event_type,
                "host_id": this.host_info.host_id,
                "host_name": this.host_info.hostname,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "payload": payload,
            });
            let client = match reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
            {
                Ok(c) => c,
                Err(_) => return,
            };
            let mut req = client.post(&url).json(&body);
            if let Some(sec) = secret_opt.filter(|s| !s.is_empty()) {
                req = req.header("Authorization", format!("Bearer {sec}"));
            }
            if let Err(e) = req.send().await {
                warn!(event = event_type, error = %e, "External Hermes webhook delivery failed");
            }
        });
    }
}

/// Core assistant & routing constitution injected persistently into the Personal Router's `system_prompt_append` & exported as `SOUL.md`.
pub fn build_router_system_instructions(cfg: &PersonalRouterConfig) -> String {
    let mut base = String::from(
        "# Orbit Personal Agent (个人助理 & 多机总控)\n\n\
         You are the user's persistent **Personal Agent** inside Orbit — a natural, sharp, reliable personal assistant \
         who can both chat/answer everyday questions directly AND orchestrate hands-on work across all of the user's \
         connected devices (`<orbit-fleet-state>` / `<orbit-fleet-delta>`).\n\n\
         ## Operating Principles\n\
         1. **Chat & Answer Directly When Light; Dispatch Cleanly When Hands-On**:\n\
            - For casual conversation, quick Q&A, brainstorming, explanations, writing/drafting, or recalling personal preferences, \
              reply directly in a natural, human, concise tone — do NOT force-spawn a separate session for simple questions.\n\
            - For hands-on coding, editing repo files, debugging builds/tests, multi-host operations, or deep research, \
              do **1–2 fast scouting steps** if helpful (e.g., `orbit_control(action=\"inspect_project_context\")` or `orbit_memory(action=\"recall\", ...)`) \
              and then dispatch a user-visible Orbit Conversation via `dispatch_conversation` (or `create_agent` / `send_agent_prompt` with `notify_on_finish: true`).\n\
            - Never implement repo code edits directly in this chat when a dedicated project Conversation belongs on a target host/workspace.\n\
         2. **User-Visible Orbit `Conversation`s — NEVER Black-Box Internal Subagents**:\n\
            - **NEVER** call harness-internal subagent tools (`delegate_task`, `Task`, `invoke_subagent`, `swarm`, etc.): those hide sessions inside a black box.\n\
            - **ALWAYS** dispatch execution/research via Orbit MCP `dispatch_conversation` / `create_agent` / `send_agent_prompt` (`notify_on_finish: true`) \
              so the user can inspect or take over any thread in the Left Sidebar.\n\
            - Workers automatically inherit the user's account-wide default permission mode when `mode` is omitted; only pass `mode` explicitly if the user requests a different permission level.\n\
         3. **Multi-Device Fleet & Long-Lived Workspace Routing**:\n\
            - Inspect `<orbit-fleet-state>` first (Hosts, Projects, Workspaces + goals, ActiveWorkers). If projects are truncated, call `orbit_control(action=\"list_projects\", host_id=\"all\")` before assuming a repo is missing.\n\
            - Reuse an existing `Conversation` (`send_agent_prompt`) when continuing the same topic, and reuse an existing `Workspace` (`workspace_id=...`) for the same project arena \
              (use `orbit_workspace(action=\"reset_board\", ...)` when starting a fresh phase in a finished workspace).\n\
         4. **Natural, High-Signal Communication**:\n\
            - Match the user's language and conversational rhythm — warm, clear, no stiff boilerplate or raw log/XML dumps.\n\
            - When you dispatch work: briefly confirm what you kicked off and where, in plain language.\n\
            - When woken by a finished Conversation: report the outcome directly to the user in a crisp summary (what was done, key result, and any decision needed). \
              Orbit's UI already attaches the source tag and an expandable details card, so do NOT repeat raw agent IDs or system notification headers.\n\
         5. **Personal Memory**:\n\
            - Use `orbit_memory` (`recall` / `remember` / `confirm` / `forget`) to remember and recall the user's preferences, recurring context, and cross-project habits.\n\
         6. **Strict Manual Reclaim Only (`发指令或点按钮才收回`)**:\n\
            - **NEVER** auto-archive or call `reclaim_conversation` / `orbit_agent(action=\"archive\" | \"kill\")` on your own when a Conversation finishes! \
              Leave completed Conversations visible until the user explicitly asks to reclaim or archive them.\n\
         7. **3-Tier Safety & Pre-Review Discipline**:\n\
            - Only pre-review Tier-2 (`Medium`) permission checkpoints for Conversations you routed when clearly aligned with user intent (`orbit_control(action=\"respond_permission\", ...)` in `medium_risk_only` mode).\n\
            - In `assist_only` mode OR for any Tier-3 (`High`/`Critical`, `human_only`, or plan approval) checkpoint, do NOT resolve directly — call \
              `orbit_control(action=\"recommend_permission\", agent_id=..., request_id=..., decision=\"allow\"|\"deny\", reason=...)` with a 1-sentence rationale.\n",
    );
    if let Some(ref custom) = cfg.soul_prompt_override {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            base.push_str("\n## User Custom Instructions\n");
            base.push_str(trimmed);
            base.push('\n');
        }
    }
    base
}

const ORBIT_ROUTER_SKILL_TEMPLATE: &str = "\
---
name: orbit-router
description: Act as the user's personal assistant and multi-host Orbit coordinator — chat/answer directly when light, and dispatch hands-on work into user-visible Sidebar Conversations via Orbit MCP.
---

# Orbit Personal Agent Workflow

1. Answer conversational questions, drafting, brainstorming, and memory queries directly and naturally in this chat.
2. For coding, repo edits, or multi-host tasks, check `<orbit-fleet-state>` (call `orbit_control(action=\"list_projects\", host_id=\"all\")` if truncated), scout lightly if needed, and dispatch a user-visible Sidebar Conversation via `dispatch_conversation` or `send_agent_prompt` (`notify_on_finish: true`).
3. Omit `mode` unless the user specifies one so workers inherit the account-wide default permission mode. Never spawn black-box subagents (`delegate_task` / `invoke_subagent`).
4. When a dispatched Conversation finishes, report a clear, natural summary (the UI attaches the source badge & expandable card). Keep completed Conversations in the sidebar until explicitly asked to reclaim.
";

type RemoteHostAtlasBundle = (
    String,
    Option<String>,
    Vec<FleetProjectEntry>,
    Vec<FleetWorkspaceEntry>,
    BTreeMap<String, WorkerContextDigest>,
    Vec<String>,
);

async fn fetch_remote_host_atlas_bundle(
    connected: Arc<ConnectedHostsManager>,
    host: ConnectedHostSummary,
) -> RemoteHostAtlasBundle {
    let hid = host
        .host_id
        .clone()
        .unwrap_or_else(|| host.registration_id.clone());
    let c_proj = Arc::clone(&connected);
    let h_proj = host.clone();
    let c_ws = Arc::clone(&connected);
    let h_ws = host.clone();
    let c_ag = Arc::clone(&connected);
    let h_ag = host.clone();
    let c_perm = Arc::clone(&connected);
    let hid_perm = hid.clone();
    let c_cap = Arc::clone(&connected);
    let h_cap = host.clone();

    let (projs, wss, mut workers, perms_raw, cap) = tokio::join!(
        fetch_remote_host_projects(c_proj, h_proj),
        fetch_remote_host_workspaces(c_ws, h_ws),
        fetch_remote_host_workers(c_ag, h_ag),
        async move {
            let req_id = Uuid::new_v4();
            match c_perm
                .send_rpc(
                    &hid_perm,
                    req_id,
                    WireMessage::ListPendingPermissions {
                        request_id: req_id,
                        agent_id: None,
                    },
                    Duration::from_secs(4),
                )
                .await
            {
                Ok(WireMessage::PendingPermissionsSnapshot { permissions, .. }) => permissions,
                _ => Vec::new(),
            }
        },
        probe_remote_host_capabilities(c_cap, h_cap),
    );

    let mut perm_counts: HashMap<String, usize> = HashMap::new();
    let mut perm_lines = Vec::new();
    for p in &perms_raw {
        *perm_counts.entry(p.agent_id.clone()).or_insert(0) += 1;
        let risk = p
            .guardian_assessment
            .as_ref()
            .map(|a| a.risk_level.as_str())
            .unwrap_or("unknown");
        perm_lines.push(format!(
            "- [host={hid}] agent={} req={} risk={} title={}\n",
            p.agent_id,
            p.request_id,
            risk,
            compact_inline(&p.title, 64)
        ));
    }
    for (key, w) in &mut workers {
        if let Some((_, raw_aid)) = key.split_once(':')
            && let Some(&cnt) = perm_counts.get(raw_aid)
        {
            w.pending_perms = cnt;
        }
    }

    let host_desc = if cap.online {
        let provs = if cap.available_providers.is_empty() {
            String::new()
        } else {
            let names: Vec<&str> = cap
                .available_providers
                .iter()
                .map(|p| p.short_label())
                .collect();
            format!(", providers=[{}]", names.join(","))
        };
        Some(format!("{} (remote, online{provs})", cap.host_name))
    } else {
        None
    };

    (hid, host_desc, projs, wss, workers, perm_lines)
}

async fn fetch_remote_host_workspaces(
    connected: Arc<ConnectedHostsManager>,
    host: ConnectedHostSummary,
) -> Vec<FleetWorkspaceEntry> {
    let hid = host
        .host_id
        .clone()
        .unwrap_or_else(|| host.registration_id.clone());
    let req_id = Uuid::new_v4();
    match connected
        .send_rpc(
            &hid,
            req_id,
            WireMessage::ListWorkspaces {
                request_id: req_id,
                include_archived: false,
                include_background: false,
            },
            Duration::from_secs(4),
        )
        .await
    {
        Ok(WireMessage::WorkspacesSnapshot { workspaces, .. }) => workspaces
            .into_iter()
            .filter(|w| w.is_curated)
            .take(MAX_FLEET_WORKSPACES_IN_CONTEXT)
            .map(|w| FleetWorkspaceEntry {
                host_id: hid.clone(),
                host_name: host.label.clone(),
                is_local: false,
                workspace_id: w.workspace_id,
                title: w.title,
                cwd: w.cwd,
                git_branch: w.git_branch,
                mission_goal: w.mission_goal,
                board_revision: w.board_revision,
            })
            .collect(),
        _ => Vec::new(),
    }
}

async fn fetch_remote_host_projects(
    connected: Arc<ConnectedHostsManager>,
    host: ConnectedHostSummary,
) -> Vec<FleetProjectEntry> {
    let hid = host
        .host_id
        .clone()
        .unwrap_or_else(|| host.registration_id.clone());
    let req_id = Uuid::new_v4();
    match connected
        .send_rpc(
            &hid,
            req_id,
            WireMessage::DiscoverProjects { request_id: req_id },
            Duration::from_secs(4),
        )
        .await
    {
        Ok(WireMessage::DiscoveredProjectsSnapshot { projects, .. }) => projects
            .into_iter()
            .take(12)
            .map(|p| FleetProjectEntry {
                host_id: hid.clone(),
                host_name: host.label.clone(),
                is_local: false,
                name: p.name,
                cwd: p.path,
                git_branch: p.git_branch,
            })
            .collect(),
        _ => Vec::new(),
    }
}

async fn fetch_remote_host_workers(
    connected: Arc<ConnectedHostsManager>,
    host: ConnectedHostSummary,
) -> BTreeMap<String, WorkerContextDigest> {
    let hid = host
        .host_id
        .clone()
        .unwrap_or_else(|| host.registration_id.clone());
    let req_id = Uuid::new_v4();
    let mut out = BTreeMap::new();
    if let Ok(WireMessage::AgentsSnapshot { agents, .. }) = connected
        .send_rpc(
            &hid,
            req_id,
            WireMessage::ListAgents {
                request_id: req_id,
                workspace_id: None,
                include_archived: false,
                include_background: false,
                parent_agent_id: None,
            },
            Duration::from_secs(4),
        )
        .await
    {
        for ag in agents
            .into_iter()
            .filter(|a| a.is_primary_conversation())
            .take(16)
        {
            let handle = ag
                .handle
                .as_deref()
                .filter(|h| !h.trim().is_empty())
                .map(|h| format!("@{h}"))
                .unwrap_or_else(|| ag.agent_id.clone());
            let key = format!("{hid}:{}", ag.agent_id);
            out.insert(
                key,
                WorkerContextDigest {
                    agent_id: format!("[host={hid}] {}", ag.agent_id),
                    handle,
                    title: compact_inline(&ag.title, 48),
                    workspace_id: ag.workspace_id,
                    provider: ag.provider.display_name().to_string(),
                    status: format_compact_agent_status(&ag.status),
                    task_summary: ag
                        .task_summary
                        .as_deref()
                        .map(|s| compact_inline(s, 80))
                        .unwrap_or_default(),
                    pending_perms: 0,
                },
            );
        }
    }
    out
}

fn read_excerpt(path: &Path, max_chars: usize) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() <= max_chars {
        Some(trimmed.to_string())
    } else {
        let mut s: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
        s.push('…');
        Some(s)
    }
}

async fn probe_remote_host_capabilities(
    connected: Arc<ConnectedHostsManager>,
    host: ConnectedHostSummary,
) -> RouterHostCapabilities {
    let hid = host
        .host_id
        .clone()
        .unwrap_or_else(|| host.registration_id.clone());
    if !host.enabled {
        return RouterHostCapabilities {
            host_id: hid,
            host_name: host.label,
            is_local: false,
            online: false,
            hermes_installed: false,
            hermes_version: None,
            hermes_has_existing_memory: false,
            hermes_acp_ready: false,
            hermes_gateway_running: false,
            available_providers: Vec::new(),
            active_router_agent_id: None,
            active_router_status: None,
            router_enabled: false,
            recommended_score: 0,
            recommendation_reason: Some("Host disabled".to_string()),
        };
    }

    let req_id = Uuid::new_v4();
    match connected
        .send_rpc(
            &hid,
            req_id,
            WireMessage::ProbeRouterCapabilities { request_id: req_id },
            Duration::from_secs(5),
        )
        .await
    {
        Ok(WireMessage::RouterCapabilitiesResponse {
            mut capabilities, ..
        }) => {
            capabilities.is_local = false;
            capabilities.online = true;
            if capabilities.host_name.trim().is_empty() {
                capabilities.host_name = host.label;
            }
            capabilities
        }
        _ => RouterHostCapabilities {
            host_id: hid,
            host_name: host.label,
            is_local: false,
            online: false,
            hermes_installed: false,
            hermes_version: None,
            hermes_has_existing_memory: false,
            hermes_acp_ready: false,
            hermes_gateway_running: false,
            available_providers: Vec::new(),
            active_router_agent_id: None,
            active_router_status: None,
            router_enabled: false,
            recommended_score: 0,
            recommendation_reason: Some("Host unreachable or older daemon".to_string()),
        },
    }
}

fn compute_recommendation_score(
    online: bool,
    hermes_installed: bool,
    hermes_has_memory: bool,
    has_active_router: bool,
    has_fallback_providers: bool,
) -> (u8, String) {
    if !online {
        return (0, "Host offline".to_string());
    }
    let mut score: u8 = 20;
    let mut reasons = Vec::new();
    if has_active_router {
        score = score.saturating_add(30);
        reasons.push("Active Personal Agent session");
    }
    if hermes_installed {
        score = score.saturating_add(35);
        reasons.push("Hermes detected");
    }
    if hermes_has_memory {
        score = score.saturating_add(15);
        reasons.push("Long-term memory present");
    }
    if !hermes_installed && has_fallback_providers {
        score = score.saturating_add(15);
        reasons.push("Built-in Coding Harness driver available");
    }
    let reason = if reasons.is_empty() {
        "Online host".to_string()
    } else {
        reasons.join(" · ")
    };
    (score.min(100), reason)
}

fn resolve_effective_router_provider(
    cfg: &PersonalRouterConfig,
) -> anyhow::Result<(ProviderKind, Option<String>)> {
    if cfg.provider == ProviderKind::Acp {
        let acp_id = cfg
            .acp_agent_id
            .clone()
            .unwrap_or_else(|| "hermes".to_string());
        if acp_id == "hermes" {
            let user_home = resolve_user_home();
            if find_hermes_executable(&user_home).is_some() {
                return Ok((ProviderKind::Acp, Some("hermes".to_string())));
            }
            // Graceful fallback when user enables Router before installing Hermes
            for fallback in [
                ProviderKind::ClaudeCode,
                ProviderKind::Antigravity,
                ProviderKind::Codex,
            ] {
                if find_provider_executable(fallback).is_some() {
                    warn!(
                        fallback = ?fallback,
                        "Hermes CLI not found on host; falling back to installed provider for Personal Router"
                    );
                    return Ok((fallback, None));
                }
            }
        }
        return Ok((ProviderKind::Acp, Some(acp_id)));
    }
    Ok((cfg.provider, cfg.acp_agent_id.clone()))
}

pub fn find_hermes_executable(user_home: &Path) -> Option<PathBuf> {
    if let Some(p) = resolve_acp_agent_command("hermes") {
        return Some(p);
    }
    let candidates = [
        user_home.join(".hermes/hermes-agent/.venv/bin/hermes"),
        user_home.join(".local/bin/hermes"),
        user_home.join(".hermes/bin/hermes"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

fn normalize_hermes_version_line(raw_line: &str) -> Option<String> {
    let mut s = raw_line.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    for prefix in ["hermes agent ", "hermes-agent ", "hermes "] {
        if lower.starts_with(prefix) {
            s = s[prefix.len()..].trim();
            break;
        }
    }
    if s.is_empty() {
        None
    } else {
        Some(compact_inline(s, 48))
    }
}

fn probe_hermes_version(bin: &Path) -> Option<String> {
    let out = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    normalize_hermes_version_line(raw.lines().next()?)
}

fn detect_hermes_existing_memory(user_home: &Path) -> bool {
    let hermes_dir = user_home.join(".hermes");
    hermes_dir.join("memories/MEMORY.md").is_file()
        || hermes_dir.join("memories/USER.md").is_file()
        || hermes_dir.join("state.db").is_file()
        || hermes_dir.join("SOUL.md").is_file()
}

fn detect_hermes_gateway_running(user_home: &Path) -> bool {
    if user_home.join(".hermes/gateway.pid").is_file() {
        return true;
    }
    let addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], 8642));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(80)).is_ok()
}

fn resolve_user_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn format_compact_agent_status(status: &AgentStatus) -> String {
    match status {
        AgentStatus::Initializing => "initializing".to_string(),
        AgentStatus::Idle => "idle".to_string(),
        AgentStatus::Running { .. } => "running".to_string(),
        AgentStatus::AutonomousWakeup { .. } => "wakeup".to_string(),
        AgentStatus::AwaitingPermission { .. } => "awaiting_permission".to_string(),
        AgentStatus::Error { message, .. } => format!("error({})", compact_inline(message, 40)),
        AgentStatus::Closed => "closed".to_string(),
    }
}

fn format_worker_line(w: &WorkerContextDigest) -> String {
    let perm_tag = if w.pending_perms > 0 {
        format!(" [pending_perms={}]", w.pending_perms)
    } else {
        String::new()
    };
    let summary_part = if w.task_summary.is_empty() {
        String::new()
    } else {
        format!(" | {}", w.task_summary)
    };
    format!(
        "- {} ({}) ws={} {} [{}]{}{}\n",
        w.handle, w.agent_id, w.workspace_id, w.provider, w.status, perm_tag, summary_part
    )
}

fn format_pending_perm_line(p: &PendingPermissionSummary) -> String {
    let risk = p
        .guardian_assessment
        .as_ref()
        .map(|a| a.risk_level.as_str())
        .unwrap_or("unknown");
    format!(
        "- agent={} req={} risk={} title={}\n",
        p.agent_id,
        p.request_id,
        risk,
        compact_inline(&p.title, 64)
    )
}

fn build_router_create_spec(
    cfg: &PersonalRouterConfig,
    provider: ProviderKind,
    acp_agent_id: Option<String>,
    cwd: String,
    account_default_mode: Option<SessionMode>,
) -> CreateAgentSpec {
    // Full Router constitution is persistently injected via `SessionLaunchConfig.system_prompt_append`
    // on every launch and resume. Keep `initial_prompt` empty so creating the Personal Agent never
    // injects a fake user-authored greeting bubble into the conversation timeline.
    let mode = account_default_mode
        .filter(|m| !matches!(m, SessionMode::Plan))
        .unwrap_or(SessionMode::Auto);
    CreateAgentSpec {
        cwd,
        extra_dirs: Vec::new(),
        title: Some("Personal Agent".to_string()),
        provider,
        model: cfg.model.clone(),
        effort: cfg.effort,
        fast_mode: None,
        mode,
        initial_prompt: String::new(),
        initial_display_prompt: None,
        initial_badges: Vec::new(),
        worktree: false,
        workspace_id: None,
        parent_agent_id: None,
        parent_host_id: None,
        initial_prompt_source: None,
        profile_id: None,
        background: false,
        auto_archive: false,
        base_ref: None,
        worktree_mode: WorktreeCreateMode::BranchOff,
        branch: None,
        pr_number: None,
        handle: Some("router".to_string()),
        auto_cluster: Some(false),
        acp_agent_id,
    }
}

fn compact_inline(raw: &str, max_chars: usize) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        collapsed
    } else {
        let mut s: String = collapsed
            .chars()
            .take(max_chars.saturating_sub(1))
            .collect();
        s.push('…');
        s
    }
}

fn load_router_file(home_dir: &Path) -> PersistedRouterFile {
    let path = home_dir.join(ROUTER_CONFIG_FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return PersistedRouterFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_router_file(home_dir: &Path, data: &PersistedRouterFile) -> anyhow::Result<()> {
    std::fs::create_dir_all(home_dir)?;
    let path = home_dir.join(ROUTER_CONFIG_FILE);
    let tmp = home_dir.join(format!(
        "{ROUTER_CONFIG_FILE}.tmp.{}",
        Uuid::new_v4().simple()
    ));
    let json = serde_json::to_string_pretty(data)?;
    std::fs::write(&tmp, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Helper for checking whether a raw label map marks an agent as `PersonalRouter`.
pub fn labels_mark_personal_router(labels: &HashMap<String, String>) -> bool {
    labels
        .get(LABEL_AGENT_ROLE)
        .is_some_and(|v| v == AGENT_ROLE_PERSONAL_ROUTER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_protocol::GuardianDecision;

    #[test]
    fn recommendation_score_prefers_hermes_with_memory_and_active_router() {
        let (s_hermes_mem, _) = compute_recommendation_score(true, true, true, true, true);
        let (s_hermes_only, _) = compute_recommendation_score(true, true, false, false, true);
        let (s_fallback, _) = compute_recommendation_score(true, false, false, false, true);
        let (s_offline, _) = compute_recommendation_score(false, true, true, true, true);
        assert_eq!(s_offline, 0);
        assert!(s_hermes_mem > s_hermes_only);
        assert!(s_hermes_only > s_fallback);
    }

    #[test]
    fn tier3_high_critical_always_blocked_from_router_auto_allow() {
        let rt = tokio::runtime::Runtime::new().expect("rt");
        rt.block_on(async {
            let tmp =
                std::env::temp_dir().join(format!("orbit-router-test-{}", Uuid::new_v4().simple()));
            let store = Arc::new(OrbitStore::open_in_memory().expect("store"));
            let host_info = HostInfo {
                host_id: "host_self".into(),
                hostname: "Test Host".into(),
                os: "macos".into(),
                arch: "aarch64".into(),
                public_key_b64: "pk".into(),
                protocol_version: orbit_protocol::PROTOCOL_VERSION,
                daemon_version: "0.2.0".into(),
                available_providers: vec![],
                relay_url: None,
                direct_endpoints: vec![],
                board_cas_supported: true,
            };
            let listen_port = Arc::new(RwLock::new(6868));
            let orch_cfg = Arc::new(RwLock::new(orbit_protocol::OrchestrationConfig::default()));
            let scripts = crate::workspace_runtime::WorkspaceScriptRegistry::new();
            let keep_awake = Arc::new(crate::keep_awake::KeepAwakeManager::new(false));
            let provider_catalogs = Arc::new(RwLock::new(None));
            let hooks = crate::hooks::HooksEngine::new(tmp.clone());
            let orch = AgentOrchestrator::new(
                tmp.clone(),
                host_info.host_id.clone(),
                "tok".into(),
                Arc::clone(&listen_port),
                orch_cfg,
                Arc::clone(&store),
                scripts,
                keep_awake,
                provider_catalogs,
                hooks,
            )
            .await;
            let connected =
                ConnectedHostsManager::new(host_info.host_id.clone(), Arc::clone(&store));
            let mgr = PersonalRouterManager::new(
                tmp.clone(),
                host_info,
                listen_port,
                "tok".into(),
                store,
                orch,
                connected,
            );

            let make_assess = |risk: GuardianRiskLevel| GuardianAssessment {
                risk_level: risk,
                decision: GuardianDecision::RequireHuman,
                confidence: 0.9,
                reason: "test".into(),
                classifier_tier: "tier1".into(),
                signals: vec![],
                latency_ms: 1,
            };

            // Medium allowed in default MediumRiskOnly mode
            assert!(
                mgr.validate_router_permission_decision(
                    "allow",
                    Some(&make_assess(GuardianRiskLevel::Medium))
                )
                .await
                .is_ok()
            );
            // High & Critical HARD BLOCKED for allow
            assert!(
                mgr.validate_router_permission_decision(
                    "allow",
                    Some(&make_assess(GuardianRiskLevel::High))
                )
                .await
                .is_err()
            );
            assert!(
                mgr.validate_router_permission_decision(
                    "allow_always",
                    Some(&make_assess(GuardianRiskLevel::Critical))
                )
                .await
                .is_err()
            );
            // Missing assessment fail-closed for allow
            assert!(
                mgr.validate_router_permission_decision("allow", None)
                    .await
                    .is_err()
            );
            // Deny is permitted on High so Router can actively protect the user
            assert!(
                mgr.validate_router_permission_decision(
                    "deny",
                    Some(&make_assess(GuardianRiskLevel::High))
                )
                .await
                .is_ok()
            );

            let _ = std::fs::remove_dir_all(&tmp);
        });
    }

    #[test]
    fn normalize_hermes_version_strips_duplicate_product_prefix() {
        assert_eq!(
            normalize_hermes_version_line("Hermes Agent v0.22.2 (2026.9.24)").as_deref(),
            Some("v0.22.2 (2026.9.24)")
        );
        assert_eq!(
            normalize_hermes_version_line("hermes 0.22.2").as_deref(),
            Some("0.22.2")
        );
        assert_eq!(
            normalize_hermes_version_line("v0.22.2").as_deref(),
            Some("v0.22.2")
        );
        assert_eq!(normalize_hermes_version_line("   "), None);
    }

    #[test]
    fn ensure_router_agent_does_not_force_fast_mode_for_acp() {
        let cfg = PersonalRouterConfig {
            enabled: true,
            provider: ProviderKind::Acp,
            acp_agent_id: Some("hermes".into()),
            ..PersonalRouterConfig::default()
        };
        let spec = build_router_create_spec(
            &cfg,
            ProviderKind::Acp,
            Some("hermes".into()),
            "/tmp/workspace".into(),
            Some(SessionMode::FullAccess),
        );
        assert_eq!(spec.fast_mode, None);
        assert_eq!(spec.provider, ProviderKind::Acp);
        assert_eq!(spec.acp_agent_id.as_deref(), Some("hermes"));
        assert_eq!(spec.mode, SessionMode::FullAccess);
        assert!(
            spec.initial_prompt.is_empty(),
            "Personal Agent init must not inject a synthetic user prompt bubble"
        );
        assert_eq!(spec.initial_display_prompt, None);
    }

    #[test]
    fn router_instructions_support_natural_chat_and_multi_host_dispatch() {
        let cfg = PersonalRouterConfig::default();
        let prompt = build_router_system_instructions(&cfg);
        assert!(prompt.contains("Chat & Answer Directly"));
        assert!(prompt.contains("dispatch_conversation"));
        assert!(prompt.contains("orbit_memory"));
        assert!(prompt.contains("account-wide default permission mode"));
        assert!(!prompt.contains("NOT a chatty Q&A bot"));
    }
}
