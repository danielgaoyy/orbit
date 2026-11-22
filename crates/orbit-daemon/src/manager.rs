//! Core Agent, Workspace/Worktree, Script, and Detached PTY lifecycle manager inside `orbit-daemon`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Utc;
use orbit_protocol::{
    ActiveTurnBehavior, AgentStatus, AgentSummary, AttentionState, ForkTarget, HostProjectEntry,
    HostSyncState, KeepAwakeStatus, OrchestrationConfig, PendingPermissionSummary, PromptBadge,
    ProviderKind, ProviderModelCatalog, ProviderSubagentDescriptor, ProviderSubagentEvent,
    QuestionItem, ReasoningEffort, ScheduleSpec, SessionMode, TerminalSummary, TimelineEvent,
    TimelinePayload, ToolCallDetail, ToolCallStatus, WorkspaceBoardSnapshot, WorkspaceEventKind,
    WorkspaceEventRecord, WorkspaceIsolation, WorkspaceOrganizeEvent, WorkspaceScriptInfo,
    WorkspaceScriptKind, WorkspaceScriptStatus, WorkspaceSetupProgress, WorkspaceSetupStatus,
    WorkspaceSummary, WorktreeCreateMode, WorktreeHandoffDirection, WorktreeHandoffMode,
    WorktreeHandoffOutcome, WorktreeHandoffPreview,
};
use orbit_providers::{
    DriverCommand, DriverEvent, OrbitMcpLaunchSpec, PersistedBackgroundTask, SessionLaunchConfig,
    find_provider_executable, provider_model_supports_fast_mode, spawn_provider_session,
};
use orbit_shim::{
    ShimClient, ShimFrame, ShimMode, ShimSpawnSpec, connect_existing_shim, kill_existing_shim,
    probe_existing_shim, spawn_detached_shim,
};
use orbit_store::OrbitStore;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{info, warn};
use uuid::Uuid;

use crate::fast_copy::{DirShareMode, link_or_clone_dir};
use crate::git::{
    capture_git_tree_snapshot, inspect_git_status, resolve_git_scope, restore_git_tree_snapshot,
    reverse_apply_file_edit_diffs,
};
use crate::keep_awake::KeepAwakeManager;
use crate::messaging::{CoalescedDispatchBatch, sanitize_human_prompt_for_wire};
use crate::workspace_board::{
    board_file_path, build_workspace_roster_context, default_board_template,
    upsert_markdown_section, write_board_file,
};
use crate::workspace_cluster::{allocate_unique_handle, evaluate_auto_cluster, sanitize_handle};
use crate::workspace_runtime::{
    WorkspaceScriptRegistry, allocate_ephemeral_port, build_service_proxy_host,
    create_worktree_with_mode_pooled, load_workspace_manifest_with_fallback,
    run_worktree_setup_hooks, teardown_and_remove_worktree, validate_git_base_ref,
    validate_git_branch_name,
};
use crate::worktree_handoff::{
    HandoffExecutePathsSpec, HandoffPreviewPathsSpec, execute_worktree_handoff_paths,
    force_delete_git_branch, preflight_validate_source_safety, preview_worktree_handoff_paths,
    resolve_git_checkout_info, validate_handoff_branch_name,
};
use crate::worktree_pool::WorktreePool;

#[derive(Debug, Clone)]
pub struct RewindAgentOutcome {
    pub agent: AgentSummary,
    pub to_before_seq: u64,
    pub events: Vec<TimelineEvent>,
    pub restored_files: u32,
    pub restored_prompt: Option<String>,
    pub notice: Option<String>,
}

pub struct ForkAgentSpec {
    pub source_agent_id: String,
    pub up_to_seq: Option<u64>,
    pub target: ForkTarget,
    pub title: Option<String>,
    pub initial_prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ForkAgentOutcome {
    pub agent: AgentSummary,
    pub workspace: Option<WorkspaceSummary>,
}

#[derive(Debug, Clone)]
pub struct ExecuteWorktreeHandoffSpec {
    pub workspace_id: String,
    pub direction: WorktreeHandoffDirection,
    pub mode: WorktreeHandoffMode,
    pub target_workspace_id: Option<String>,
    pub include_committed: bool,
    pub branch: Option<String>,
    pub allow_three_way_conflicts: bool,
}

struct DeferredWorktreeSetup {
    workspace_id: String,
    source_cwd: PathBuf,
    worktree_cwd: PathBuf,
    branch_name: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum DaemonBroadcast {
    AgentUpdated(AgentSummary),
    AgentDeleted(String),
    TimelineLive(TimelineEvent),
    WorkspaceUpdated(WorkspaceSummary),
    WorkspaceSetupProgress(WorkspaceSetupProgress),
    WorkspaceScriptUpdated(WorkspaceScriptInfo),
    ScheduleUpdated(ScheduleSpec),
    ScheduleDeleted(String),
    ConnectedHostRemoved(String),
    ProviderSubagentUpdated(ProviderSubagentDescriptor),
    ProviderSubagentTimelineLive(ProviderSubagentEvent),
    AgentRewound {
        origin_session_id: Uuid,
        outcome: RewindAgentOutcome,
    },
    HostSyncStateUpdated(HostSyncState),
    WorkspaceBoardUpdated(WorkspaceBoardSnapshot),
    WorkspaceEventRecorded(WorkspaceEventRecord),
    WorkspaceOrganizeProgress(WorkspaceOrganizeEvent),
    KeepAwakeStatusUpdated(KeepAwakeStatus),
    AcpAgentsUpdated(Vec<orbit_protocol::AcpAgentAvailability>),
    ArtifactUpdated(orbit_protocol::ArtifactSummary),
    ArtifactDeleted(String),
}

struct LiveAgentEntry {
    generation: u64,
    cmd_tx: mpsc::Sender<DriverCommand>,
    done_notify: Arc<tokio::sync::Notify>,
    ingest_abort: tokio::task::AbortHandle,
    shim_seq_tracker: Arc<AtomicU64>,
}

#[derive(Debug, Clone)]
pub enum PtyBroadcast {
    Snapshot(Vec<u8>),
    Output(Vec<u8>),
    Closed,
}

struct LiveTerminal {
    summary: TerminalSummary,
    shim_tx: mpsc::Sender<PtyAction>,
    out_tx: broadcast::Sender<PtyBroadcast>,
    last_snapshot: Arc<RwLock<std::collections::VecDeque<u8>>>,
}

enum PtyAction {
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Kill,
}

pub struct CreateWorkspaceSpec {
    pub source_cwd: String,
    pub title: Option<String>,
    pub worktree_branch: Option<String>,
    pub base_ref: Option<String>,
    pub isolation: WorkspaceIsolation,
    pub worktree_mode: WorktreeCreateMode,
    pub pr_number: Option<u32>,
    pub parent_workspace_id: Option<String>,
    pub background: bool,
    pub project_root: Option<String>,
    pub mission_goal: Option<String>,
    pub allow_reuse_existing: bool,
    pub is_curated: bool,
}

pub struct CreateAgentSpec {
    pub cwd: String,
    pub extra_dirs: Vec<String>,
    pub title: Option<String>,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    pub fast_mode: Option<bool>,
    pub mode: SessionMode,
    pub initial_prompt: String,
    pub initial_display_prompt: Option<String>,
    pub initial_badges: Vec<PromptBadge>,
    pub worktree: bool,
    pub workspace_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub parent_host_id: Option<String>,
    pub profile_id: Option<String>,
    pub background: bool,
    pub auto_archive: bool,
    pub base_ref: Option<String>,
    pub worktree_mode: WorktreeCreateMode,
    pub branch: Option<String>,
    pub pr_number: Option<u32>,
    pub handle: Option<String>,
    pub auto_cluster: Option<bool>,
    pub acp_agent_id: Option<String>,
}

pub struct AgentOrchestrator {
    home_dir: PathBuf,
    orbit_exe: PathBuf,
    host_id: String,
    mcp_token: String,
    listen_port: Arc<RwLock<u16>>,
    orch_config: Arc<RwLock<OrchestrationConfig>>,
    store: Arc<OrbitStore>,
    pub scripts: Arc<WorkspaceScriptRegistry>,
    keep_awake: Arc<KeepAwakeManager>,
    provider_catalogs: Arc<RwLock<Option<Vec<ProviderModelCatalog>>>>,
    hooks: Arc<crate::hooks::HooksEngine>,
    pub worktree_pool: Arc<WorktreePool>,
    live_agents: RwLock<HashMap<String, LiveAgentEntry>>,
    /// Per-agent mutex serializing driver startup (e.g. concurrent eager fork startup and
    /// immediate `send_prompt`) so two shims never collide on `a_<agent_id>.sock`.
    driver_start_locks: RwLock<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    agent_generations: RwLock<HashMap<String, u64>>,
    pending_questions: RwLock<HashMap<String, Vec<String>>>,
    live_terminals: RwLock<HashMap<String, Arc<LiveTerminal>>>,
    /// Pending permissions keyed by `(agent_id, request_id)`
    pending_permissions: RwLock<HashMap<(String, String), PendingPermissionSummary>>,
    /// Last `<orbit-workspace>` roster fingerprint injected per `agent_id` to avoid redundant XML bloat.
    roster_fingerprints: RwLock<HashMap<String, String>>,
    /// Conflict radar cooldown keyed by `(workspace_id, canonical_file_path)`.
    conflict_cooldowns: RwLock<HashMap<(String, String), std::time::Instant>>,
    /// Agents whose next idle transition was caused by explicit interrupt or flush-now,
    /// suppressing automatic queue drain for that transition.
    suppress_auto_drain: RwLock<std::collections::HashSet<String>>,
    /// Agents that already persisted provider-reported `DriverEvent::TurnUsage` for the active turn,
    /// avoiding double-counting when the turn settles to `Idle`.
    turn_usage_recorded: RwLock<std::collections::HashSet<String>>,
    /// Most recent provider-reported instantaneous single-call prompt token count `(tokens, recorded_at_ms)`
    /// per `agent_id` for accurate context occupancy without multi-step turn-sum inflation.
    last_call_prompt_occupancy: RwLock<HashMap<String, (u64, i64)>>,
    /// In-flight AI organizer / summarizer locks keyed by `sum:<agent_id>` or `org:<project_root>`.
    /// Uses a synchronous `std::sync::Mutex` (never held across `.await`) so RAII `OrganizerLockGuard`
    /// always releases the lock and marks `keep_awake` idle on cancellation or panic.
    organizing_locks: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Bounded concurrency limiter for background Turn-1 AI auto-naming & summary tasks
    /// so bursts of completing sessions never saturate local CPU or provider API rate limits.
    auto_name_semaphore: Arc<tokio::sync::Semaphore>,
    pub broadcast_tx: broadcast::Sender<DaemonBroadcast>,
}

/// RAII guard for an organizer/summarizer lock key that guarantees lock release and
/// `keep_awake.mark_idle` even if the owning async task is cancelled or panics.
pub(crate) struct OrganizerLockGuard {
    locks: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    keep_awake: Arc<KeepAwakeManager>,
    key: String,
}

impl Drop for OrganizerLockGuard {
    fn drop(&mut self) {
        let removed = self
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.key);
        if removed {
            self.keep_awake.mark_idle(&format!("workflow:{}", self.key));
        }
    }
}

impl AgentOrchestrator {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        home_dir: PathBuf,
        host_id: String,
        mcp_token: String,
        listen_port: Arc<RwLock<u16>>,
        orch_config: Arc<RwLock<OrchestrationConfig>>,
        store: Arc<OrbitStore>,
        scripts: Arc<WorkspaceScriptRegistry>,
        keep_awake: Arc<KeepAwakeManager>,
        provider_catalogs: Arc<RwLock<Option<Vec<ProviderModelCatalog>>>>,
        hooks: Arc<crate::hooks::HooksEngine>,
    ) -> Arc<Self> {
        let orbit_exe = resolve_orbit_exe();
        let (broadcast_tx, _) = broadcast::channel(2048);
        let worktree_pool = WorktreePool::with_default_config(home_dir.clone());
        Arc::new(Self {
            home_dir,
            orbit_exe,
            host_id,
            mcp_token,
            listen_port,
            orch_config,
            store,
            scripts,
            keep_awake,
            provider_catalogs,
            hooks,
            worktree_pool,
            live_agents: RwLock::new(HashMap::new()),
            driver_start_locks: RwLock::new(HashMap::new()),
            agent_generations: RwLock::new(HashMap::new()),
            pending_questions: RwLock::new(HashMap::new()),
            live_terminals: RwLock::new(HashMap::new()),
            pending_permissions: RwLock::new(HashMap::new()),
            roster_fingerprints: RwLock::new(HashMap::new()),
            conflict_cooldowns: RwLock::new(HashMap::new()),
            suppress_auto_drain: RwLock::new(std::collections::HashSet::new()),
            turn_usage_recorded: RwLock::new(std::collections::HashSet::new()),
            last_call_prompt_occupancy: RwLock::new(HashMap::new()),
            organizing_locks: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            auto_name_semaphore: Arc::new(tokio::sync::Semaphore::new(2)),
            broadcast_tx,
        })
    }

    pub fn keep_awake(&self) -> &Arc<KeepAwakeManager> {
        &self.keep_awake
    }

    pub(crate) fn auto_name_semaphore(&self) -> &Arc<tokio::sync::Semaphore> {
        &self.auto_name_semaphore
    }

    pub(crate) fn store(&self) -> &OrbitStore {
        &self.store
    }

    pub(crate) fn try_acquire_organizer_lock(&self, key: &str) -> Option<OrganizerLockGuard> {
        let acquired = self
            .organizing_locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key.to_string());
        if acquired {
            self.keep_awake.mark_busy(format!("workflow:{key}"));
            Some(OrganizerLockGuard {
                locks: Arc::clone(&self.organizing_locks),
                keep_awake: Arc::clone(&self.keep_awake),
                key: key.to_string(),
            })
        } else {
            None
        }
    }

    pub(crate) async fn resolve_launch_executable(
        &self,
        provider: ProviderKind,
        acp_agent_id: Option<&str>,
    ) -> anyhow::Result<(PathBuf, Option<orbit_protocol::AcpAgentSpec>)> {
        let home_dir = self.home_dir.clone();
        let acp_id = acp_agent_id.map(ToOwned::to_owned);
        tokio::task::spawn_blocking(move || {
            if provider == ProviderKind::Acp {
                let id = acp_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("`acp_agent_id` is required when provider is `acp`")
                    })?;
                let spec = orbit_providers::get_acp_agent_spec(&home_dir, id).ok_or_else(|| {
                    anyhow::anyhow!("ACP agent `{id}` not found in registry on this host")
                })?;
                if !spec.enabled {
                    anyhow::bail!("ACP agent `{id}` is disabled on this host");
                }
                let exe =
                    orbit_providers::resolve_acp_agent_command(&spec.command).ok_or_else(|| {
                        let hint = spec
                            .install_hint
                            .as_deref()
                            .map(|h| format!(" (install: `{h}`)"))
                            .unwrap_or_default();
                        anyhow::anyhow!(
                            "ACP agent `{}` executable (`{}`) not found on host PATH{hint}",
                            spec.name,
                            spec.command
                        )
                    })?;
                return Ok((exe, Some(spec)));
            }

            let exe = find_provider_executable(provider).ok_or_else(|| {
                anyhow::anyhow!(
                    "{} binary (`{}`) not found on host PATH",
                    provider.display_name(),
                    provider.default_binary()
                )
            })?;
            Ok((exe, None))
        })
        .await
        .map_err(|e| anyhow::anyhow!("executable resolution task failed: {e}"))?
    }

    pub(crate) async fn invalidate_roster_fingerprint(&self, agent_id: &str) {
        self.roster_fingerprints.write().await.remove(agent_id);
    }

    async fn current_generation(&self, agent_id: &str) -> u64 {
        self.agent_generations
            .read()
            .await
            .get(agent_id)
            .copied()
            .unwrap_or(0)
    }

    async fn bump_generation(&self, agent_id: &str) -> u64 {
        let mut guard = self.agent_generations.write().await;
        let next = guard.get(agent_id).copied().unwrap_or(0).saturating_add(1);
        guard.insert(agent_id.to_string(), next);
        next
    }

    async fn live_agent_tx(&self, agent_id: &str) -> Option<mpsc::Sender<DriverCommand>> {
        self.live_agents
            .read()
            .await
            .get(agent_id)
            .map(|e| e.cmd_tx.clone())
    }

    /// Increment generation to fence off stale startup/ingestion tasks, shut down any live driver,
    /// wait (bounded) for the old driver ingestion loop to drain, and guarantee zero surviving shims.
    async fn stop_and_wait_driver(&self, agent_id: &str) -> u64 {
        let start_lock = self.agent_start_lock(agent_id).await;
        let _start_guard = start_lock.lock().await;
        let next_gen = self.bump_generation(agent_id).await;
        let removed = self.live_agents.write().await.remove(agent_id);
        if let Some(entry) = removed {
            let notified = entry.done_notify.notified();
            let _ = entry.cmd_tx.send(DriverCommand::Shutdown).await;
            if tokio::time::timeout(Duration::from_secs(3), notified)
                .await
                .is_err()
            {
                entry.ingest_abort.abort();
            }
        }
        kill_existing_shim(&self.shim_socket_path("a", agent_id)).await;
        kill_existing_shim(&self.legacy_shim_socket_path("agent", agent_id)).await;
        self.clear_pending_requests(agent_id).await;
        self.keep_awake.mark_agent_idle(agent_id);
        next_gen
    }

    /// Activate live shim reattachment and startup agent recovery once the Daemon HTTP/WS listener
    /// is bound (must NOT be called from short-lived CLI read commands like `orbit pair`).
    pub async fn activate_runtime(self: &Arc<Self>) {
        self.reattach_surviving_terminals().await;
        self.reattach_or_recover_agents().await;
        let pool = Arc::clone(&self.worktree_pool);
        tokio::spawn(async move {
            let pruned = pool.prune_stale_standbys().await;
            if pruned > 0 {
                info!(pruned, "Pruned stale worktree pool standbys on startup");
            }
        });
    }

    /// Clean up all warm worktree pool standbys on graceful daemon shutdown.
    pub async fn shutdown_worktree_pool(&self) {
        let removed = self.worktree_pool.shutdown_cleanup().await;
        if removed > 0 {
            info!(
                removed,
                "Cleaned up warm worktree pool standbys on shutdown"
            );
        }
    }

    /// Resolve a Unix Domain Socket path guaranteed to fit comfortably under Linux's 108-byte
    /// `sun_path` limit (even under deeply nested `$ORBIT_HOME` paths).
    fn shim_socket_path(&self, kind_prefix: &str, id: &str) -> PathBuf {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(id.as_bytes());
        let dig = h.finalize();
        let short_id: String = dig[..8].iter().map(|b| format!("{b:02x}")).collect();
        let filename = format!("{kind_prefix}_{short_id}.sock");
        let candidate = self.home_dir.join("shims").join(&filename);
        if candidate.to_string_lossy().len() <= 98 {
            return candidate;
        }
        let mut hh = Sha256::new();
        hh.update(self.home_dir.to_string_lossy().as_bytes());
        let hdig = hh.finalize();
        let home_tag: String = hdig[..4].iter().map(|b| format!("{b:02x}")).collect();
        let fallback_dir = std::env::temp_dir().join("orbit-s");
        let _ = std::fs::create_dir_all(&fallback_dir);
        fallback_dir.join(format!("{home_tag}_{filename}"))
    }

    fn legacy_shim_socket_path(&self, legacy_prefix: &str, id: &str) -> PathBuf {
        self.home_dir
            .join("shims")
            .join(format!("{legacy_prefix}_{id}.sock"))
    }

    /// Re-attach to any `orbit shim` PTY sockets that survived a Daemon restart.
    async fn reattach_surviving_terminals(self: &Arc<Self>) {
        let Ok(saved) = self.store.list_terminals() else {
            return;
        };
        for term in saved {
            let primary = self.shim_socket_path("p", &term.terminal_id);
            let legacy = self.legacy_shim_socket_path("pty", &term.terminal_id);
            let connected = if primary.exists() {
                connect_existing_shim(&primary).await
            } else {
                None
            };
            let connected = match connected {
                Some(c) => Some(c),
                None if legacy.exists() => connect_existing_shim(&legacy).await,
                None => None,
            };
            if let Some(shim) = connected {
                self.bind_terminal_shim(term, shim).await;
            } else {
                let _ = self.store.delete_terminal(&term.terminal_id);
            }
        }
    }

    /// Persist the exact last-consumed `orbit-shim` frame sequence for every live agent
    /// right before a daemon shutdown or self-upgrade hot-restart so reattached shims resume
    /// strictly from the gap without replaying already-ingested history.
    pub async fn flush_all_shim_cursors(&self) {
        let entries: Vec<(String, u64)> = self
            .live_agents
            .read()
            .await
            .iter()
            .map(|(aid, entry)| (aid.clone(), entry.shim_seq_tracker.load(Ordering::Relaxed)))
            .collect();
        for (aid, seq) in entries {
            if seq > 0 {
                let _ = self.store.set_agent_shim_cursor(&aid, seq);
            }
        }
    }

    /// Re-attach to surviving agent shims across daemon hot-upgrades, and optionally
    /// resume previously `Running` agents if `orchestration.resume_on_startup` is enabled.
    async fn reattach_or_recover_agents(self: &Arc<Self>) {
        let Ok(agents) = self.store.list_agents_filtered(None, false, true, None) else {
            return;
        };
        let cfg = self.orch_config.read().await.clone();

        for ag in agents {
            let primary = self.shim_socket_path("a", &ag.agent_id);
            let legacy = self.legacy_shim_socket_path("agent", &ag.agent_id);
            let surviving_sock = if primary.exists() && probe_existing_shim(&primary).await {
                Some(primary)
            } else if legacy.exists() && probe_existing_shim(&legacy).await {
                Some(legacy)
            } else {
                None
            };
            let was_running = ag.status.is_foreground_busy();

            if surviving_sock.is_some()
                && let Ok((executable, _)) = self
                    .resolve_launch_executable(ag.provider, ag.acp_agent_id.as_deref())
                    .await
            {
                let resume_ref = self.store.get_persistence_ref(&ag.agent_id).ok().flatten();
                let launch_gen = self.bump_generation(&ag.agent_id).await;
                self.keep_awake
                    .notify_agent_status(&ag.agent_id, &ag.status);
                if self
                    .spawn_and_wire_driver(
                        &ag, executable, resume_ref, None, false, None, true, launch_gen,
                    )
                    .await
                    .is_ok()
                {
                    info!(agent_id = %ag.agent_id, "Reattached surviving agent shim across daemon restart");
                    continue;
                }
                self.keep_awake.mark_agent_idle(&ag.agent_id);
            }

            // No surviving live shim -> normalize stale transient statuses to `Closed` and settle
            // any orphaned `Running` background-task tool calls in place so they never resurrect.
            if let Ok(stale_bg) = self.store.list_running_background_tool_calls(&ag.agent_id)
                && !stale_bg.is_empty()
            {
                let settlements: Vec<(String, ToolCallStatus)> = stale_bg
                    .into_iter()
                    .map(|(cid, _, _)| (cid, ToolCallStatus::Failed))
                    .collect();
                let _ = self
                    .store
                    .settle_timeline_tool_calls_in_place(&ag.agent_id, &settlements);
            }
            if matches!(
                ag.status,
                AgentStatus::Running { .. }
                    | AgentStatus::Initializing
                    | AgentStatus::AwaitingPermission { .. }
                    | AgentStatus::AutonomousWakeup { .. }
            ) {
                let _ = self
                    .store
                    .update_agent_status(&ag.agent_id, &AgentStatus::Closed);
                self.keep_awake.mark_agent_idle(&ag.agent_id);
                if was_running && cfg.resume_on_startup && !ag.background {
                    let mgr = Arc::clone(self);
                    let ag_id = ag.agent_id.clone();
                    let resume_prompt = cfg.resume_prompt.clone();
                    tokio::spawn(async move {
                        if let Err(e) = mgr.send_prompt(&ag_id, resume_prompt).await {
                            warn!(agent_id = %ag_id, error = %e, "Startup resume failed");
                        }
                    });
                }
            }
        }
    }

    // --- Workspace Lifecycle ---

    pub async fn ensure_workspace(
        &self,
        source_cwd: &str,
        title: Option<String>,
        worktree_branch: Option<String>,
        base_ref: Option<String>,
    ) -> anyhow::Result<WorkspaceSummary> {
        let isolation = if worktree_branch.is_some() {
            WorkspaceIsolation::Worktree
        } else {
            WorkspaceIsolation::Local
        };
        self.create_workspace_v2(CreateWorkspaceSpec {
            source_cwd: source_cwd.to_string(),
            title,
            worktree_branch,
            base_ref,
            isolation,
            worktree_mode: WorktreeCreateMode::BranchOff,
            pr_number: None,
            parent_workspace_id: None,
            background: false,
            project_root: None,
            mission_goal: None,
            allow_reuse_existing: true,
            is_curated: false,
        })
        .await
    }

    pub async fn create_workspace_v2(
        &self,
        spec: CreateWorkspaceSpec,
    ) -> anyhow::Result<WorkspaceSummary> {
        let (ws, _) = self.create_workspace_v2_inner(spec, false).await?;
        Ok(ws)
    }

    async fn create_workspace_v2_inner(
        &self,
        spec: CreateWorkspaceSpec,
        defer_setup: bool,
    ) -> anyhow::Result<(WorkspaceSummary, Option<DeferredWorktreeSetup>)> {
        let src_path = crate::expand_host_path(&spec.source_cwd);
        let use_worktree =
            spec.isolation == WorkspaceIsolation::Worktree || spec.worktree_branch.is_some();

        let (final_path, is_worktree, created_branch) = if use_worktree {
            if resolve_git_scope(&src_path).await.is_none() {
                anyhow::bail!(
                    "not_a_git_repo: `{}` is not a Git repository. Switch to Local mode or pick a nested Git repository inside this project.",
                    src_path.display()
                );
            }
            let (wt_path, br) = create_worktree_with_mode_pooled(
                &self.home_dir,
                &src_path,
                spec.worktree_mode,
                spec.worktree_branch.as_deref(),
                spec.base_ref.as_deref(),
                spec.pr_number,
                Some(&self.worktree_pool),
            )
            .await?;
            (wt_path, true, br)
        } else {
            (src_path.clone(), false, None)
        };

        let cwd_str = final_path.to_string_lossy().to_string();
        if !is_worktree
            && !spec.background
            && spec.allow_reuse_existing
            && let Ok(Some(existing)) = self.store.find_workspace_by_cwd(&cwd_str)
        {
            return Ok((existing, None));
        }

        let git_stat = inspect_git_status(&final_path).await;
        let branch_final = created_branch.or(git_stat.branch);
        let default_title = if is_worktree {
            branch_final
                .clone()
                .or_else(|| {
                    final_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .map(ToOwned::to_owned)
                })
                .unwrap_or_else(|| "worktree".into())
        } else {
            final_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("workspace")
                .to_string()
        };

        let has_setup = is_worktree
            && load_workspace_manifest_with_fallback(&final_path, Some(&src_path))
                .worktree
                .setup
                .is_some();

        let title_locked = spec.title.as_deref().is_some_and(|t| !t.trim().is_empty());
        let project_root = spec
            .project_root
            .filter(|s| !s.trim().is_empty())
            .or_else(|| Some(src_path.to_string_lossy().to_string()));
        let ws = WorkspaceSummary {
            workspace_id: format!("ws_{}", Uuid::new_v4().simple()),
            title: spec.title.unwrap_or(default_title),
            cwd: cwd_str,
            git_branch: branch_final.clone(),
            is_worktree,
            updated_at: Utc::now(),
            source_cwd: if is_worktree {
                Some(src_path.to_string_lossy().to_string())
            } else {
                None
            },
            base_ref: spec.base_ref,
            parent_workspace_id: spec.parent_workspace_id,
            archived_at: None,
            background: spec.background,
            setup_status: if has_setup && !defer_setup {
                WorkspaceSetupStatus::Running
            } else {
                WorkspaceSetupStatus::Idle
            },
            mission_goal: spec.mission_goal,
            project_root,
            board_revision: 0,
            title_locked,
            auto_clustered: false,
            is_curated: spec.is_curated,
        };
        self.store.upsert_workspace(&ws)?;
        // Share existing parent workspace scratch/artifacts directories in O(1) via reference link
        if let Some(ref parent_ws_id) = ws.parent_workspace_id {
            let parent_dir = board_file_path(&self.home_dir, parent_ws_id)
                .parent()
                .map(Path::to_path_buf);
            let child_dir = board_file_path(&self.home_dir, &ws.workspace_id)
                .parent()
                .map(Path::to_path_buf);
            if let (Some(p_dir), Some(c_dir)) = (parent_dir, child_dir) {
                let _ = tokio::task::spawn_blocking(move || {
                    for folder in ["scratch", "artifacts"] {
                        let src_folder = p_dir.join(folder);
                        if src_folder.is_dir() {
                            let _ = link_or_clone_dir(
                                &src_folder,
                                &c_dir.join(folder),
                                DirShareMode::ReferenceLink,
                            );
                        }
                    }
                })
                .await;
            }
        }
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceUpdated(ws.clone()));

        let deferred = if has_setup {
            let hook = DeferredWorktreeSetup {
                workspace_id: ws.workspace_id.clone(),
                source_cwd: src_path,
                worktree_cwd: final_path,
                branch_name: branch_final,
            };
            if defer_setup {
                Some(hook)
            } else {
                self.spawn_deferred_worktree_setup(hook, false);
                None
            }
        } else {
            None
        };

        Ok((ws, deferred))
    }

    fn spawn_deferred_worktree_setup(&self, hook: DeferredWorktreeSetup, mark_running: bool) {
        if mark_running {
            let _ = self
                .store
                .set_workspace_setup_status(&hook.workspace_id, WorkspaceSetupStatus::Running);
            if let Ok(Some(updated_ws)) = self.store.get_workspace(&hook.workspace_id) {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::WorkspaceUpdated(updated_ws));
            }
        }
        let store_bg = Arc::clone(&self.store);
        let bcast_bg = self.broadcast_tx.clone();
        tokio::spawn(async move {
            let res = run_worktree_setup_hooks(
                &hook.workspace_id,
                &hook.source_cwd,
                &hook.worktree_cwd,
                hook.branch_name.as_deref(),
                |progress| {
                    let _ = bcast_bg.send(DaemonBroadcast::WorkspaceSetupProgress(progress));
                },
            )
            .await;
            let final_status = if res.is_ok() {
                WorkspaceSetupStatus::Succeeded
            } else {
                WorkspaceSetupStatus::Failed
            };
            let _ = store_bg.set_workspace_setup_status(&hook.workspace_id, final_status);
            if let Ok(Some(updated_ws)) = store_bg.get_workspace(&hook.workspace_id) {
                let _ = bcast_bg.send(DaemonBroadcast::WorkspaceUpdated(updated_ws));
            }
        });
    }

    pub async fn rename_workspace(
        &self,
        workspace_id: &str,
        title: &str,
    ) -> anyhow::Result<WorkspaceSummary> {
        self.store.rename_workspace(workspace_id, title)?;
        let ws = self
            .store
            .get_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceUpdated(ws.clone()));
        Ok(ws)
    }

    /// Preview a bidirectional Git Worktree <-> Local checkout handoff without mutating disk.
    pub async fn preview_worktree_handoff(
        &self,
        workspace_id: &str,
        direction: WorktreeHandoffDirection,
        target_workspace_id: Option<&str>,
        include_committed: bool,
    ) -> anyhow::Result<WorktreeHandoffPreview> {
        let source_ws = self
            .store
            .get_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;
        let source_cwd = crate::expand_host_path(&source_ws.cwd);

        let target_cwd =
            if let Some(tid) = target_workspace_id.map(str::trim).filter(|s| !s.is_empty()) {
                let target_ws = self
                    .store
                    .get_workspace(tid)?
                    .ok_or_else(|| anyhow::anyhow!("target workspace `{tid}` not found"))?;
                Some(crate::expand_host_path(&target_ws.cwd))
            } else if direction == WorktreeHandoffDirection::WorktreeToLocal {
                if let Some(candidate) = source_ws
                    .source_cwd
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(crate::expand_host_path)
                    .filter(|p| *p != source_cwd)
                    && tokio::fs::try_exists(&candidate).await.unwrap_or(false)
                {
                    Some(candidate)
                } else {
                    None
                }
            } else {
                None
            };

        preview_worktree_handoff_paths(HandoffPreviewPathsSpec {
            source_cwd,
            target_cwd,
            direction,
            include_committed,
            base_ref: source_ws.base_ref.clone(),
        })
        .await
    }

    /// Execute a backup-protected bidirectional Git Worktree <-> Local checkout handoff.
    pub async fn execute_worktree_handoff(
        self: &Arc<Self>,
        spec: ExecuteWorktreeHandoffSpec,
    ) -> anyhow::Result<WorktreeHandoffOutcome> {
        let source_ws = self
            .store
            .get_workspace(&spec.workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{}` not found", spec.workspace_id))?;
        let source_cwd = crate::expand_host_path(&source_ws.cwd);

        match spec.direction {
            WorktreeHandoffDirection::WorktreeToLocal => {
                let target_cwd = if let Some(tid) = spec
                    .target_workspace_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    let target_ws = self
                        .store
                        .get_workspace(tid)?
                        .ok_or_else(|| anyhow::anyhow!("target workspace `{tid}` not found"))?;
                    crate::expand_host_path(&target_ws.cwd)
                } else if let Some(src_dir) = source_ws
                    .source_cwd
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(crate::expand_host_path)
                    .filter(|p| *p != source_cwd)
                    && tokio::fs::try_exists(&src_dir).await.unwrap_or(false)
                {
                    src_dir
                } else {
                    crate::worktree_handoff::resolve_primary_local_checkout(&source_cwd).await?
                };

                execute_worktree_handoff_paths(HandoffExecutePathsSpec {
                    source_cwd,
                    target_cwd,
                    direction: WorktreeHandoffDirection::WorktreeToLocal,
                    mode: spec.mode,
                    include_committed: spec.include_committed,
                    base_ref: source_ws.base_ref.clone(),
                    allow_three_way_conflicts: spec.allow_three_way_conflicts,
                    target_workspace: None,
                    backup_source_as_primary: false,
                })
                .await
            }
            WorktreeHandoffDirection::LocalToWorktree => {
                if let Some(tid) = spec
                    .target_workspace_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    let target_ws = self
                        .store
                        .get_workspace(tid)?
                        .ok_or_else(|| anyhow::anyhow!("target workspace `{tid}` not found"))?;
                    let target_cwd = crate::expand_host_path(&target_ws.cwd);
                    return execute_worktree_handoff_paths(HandoffExecutePathsSpec {
                        source_cwd,
                        target_cwd,
                        direction: WorktreeHandoffDirection::LocalToWorktree,
                        mode: spec.mode,
                        include_committed: false,
                        base_ref: None,
                        allow_three_way_conflicts: spec.allow_three_way_conflicts,
                        target_workspace: Some(target_ws),
                        backup_source_as_primary: false,
                    })
                    .await;
                }

                // Provision a fresh isolated worktree off current local HEAD.
                // Validate branch & tracked/untracked symlinks BEFORE provisioning the worktree!
                let source_info = resolve_git_checkout_info(&source_cwd).await?;
                let validated_branch = match spec
                    .branch
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    Some(b) => Some(validate_handoff_branch_name(&source_info.toplevel, b).await?),
                    None => None,
                };
                preflight_validate_source_safety(&source_info.toplevel).await?;

                let project_root = source_ws
                    .project_root
                    .clone()
                    .or_else(|| Some(source_info.toplevel.to_string_lossy().to_string()));
                let (new_ws, deferred_setup) = self
                    .create_workspace_v2_inner(
                        CreateWorkspaceSpec {
                            source_cwd: source_info.toplevel.to_string_lossy().to_string(),
                            title: validated_branch.clone(),
                            worktree_branch: validated_branch,
                            base_ref: source_info.head_oid.clone(),
                            isolation: WorkspaceIsolation::Worktree,
                            worktree_mode: WorktreeCreateMode::BranchOff,
                            pr_number: None,
                            parent_workspace_id: Some(source_ws.workspace_id.clone()),
                            background: source_ws.background,
                            project_root,
                            mission_goal: None,
                            allow_reuse_existing: false,
                            is_curated: false,
                        },
                        true,
                    )
                    .await?;
                let new_ws_id = new_ws.workspace_id.clone();
                let new_ws_branch = new_ws.git_branch.clone();
                let target_cwd = crate::expand_host_path(&new_ws.cwd);

                let exec_res = execute_worktree_handoff_paths(HandoffExecutePathsSpec {
                    source_cwd: source_info.toplevel.clone(),
                    target_cwd,
                    direction: WorktreeHandoffDirection::LocalToWorktree,
                    mode: spec.mode,
                    include_committed: false,
                    base_ref: None,
                    allow_three_way_conflicts: spec.allow_three_way_conflicts,
                    target_workspace: Some(new_ws),
                    backup_source_as_primary: true,
                })
                .await;

                match exec_res {
                    Ok(mut outcome) => {
                        if let Some(hook) = deferred_setup {
                            self.spawn_deferred_worktree_setup(hook, true);
                            if let Some(ref mut ws_out) = outcome.target_workspace {
                                ws_out.setup_status = WorkspaceSetupStatus::Running;
                            }
                        }
                        Ok(outcome)
                    }
                    Err(err) => {
                        // Roll back the orphaned workspace, worktree directory, and newly generated branch
                        let _ = self.archive_workspace(&new_ws_id).await;
                        if let Some(ref br) = new_ws_branch {
                            force_delete_git_branch(&source_info.toplevel, br).await;
                        }
                        Err(err)
                    }
                }
            }
        }
    }

    pub async fn archive_workspace(self: &Arc<Self>, workspace_id: &str) -> anyhow::Result<()> {
        self.archive_workspace_with_options(workspace_id, false)
            .await
    }

    pub async fn archive_workspace_with_options(
        self: &Arc<Self>,
        workspace_id: &str,
        keep_agents_unclassified: bool,
    ) -> anyhow::Result<()> {
        let ws = self
            .store
            .get_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;

        // 1. Stop any running scripts/services in this workspace
        let stopped_scripts = self
            .scripts
            .unregister_all_for_workspace(workspace_id)
            .await;
        for st in stopped_scripts {
            if let Some(ref tid) = st.info.terminal_id {
                let _ = self.kill_terminal(tid).await;
            }
            let mut updated = st.info;
            updated.status = WorkspaceScriptStatus::Stopped;
            updated.terminal_id = None;
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceScriptUpdated(updated));
        }

        // 2. Either disband (move active conversations to `待归类`) or soft-archive all active agents
        let agents = self
            .store
            .list_agents_filtered(Some(workspace_id), false, true, None)?;
        if keep_agents_unclassified {
            for ag in agents {
                if ag.background {
                    let _ = self.archive_agent(&ag.agent_id).await;
                } else {
                    let _ = self
                        .move_agent_to_workspace_v2(&ag.agent_id, None, None, true)
                        .await;
                }
            }
        } else {
            for ag in agents {
                let _ = self.archive_agent(&ag.agent_id).await;
            }
        }

        // 3. Mark workspace archived
        self.store.archive_workspace_soft(workspace_id)?;
        if let Ok(Some(updated_ws)) = self.store.get_workspace(workspace_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceUpdated(updated_ws));
        }

        // 4. If managed worktree and NO other active workspace references its directory -> teardown & remove
        if ws.is_worktree
            && self
                .store
                .count_active_workspaces_for_cwd(&ws.cwd)
                .unwrap_or(1)
                == 0
        {
            let src_opt = ws.source_cwd.as_deref().map(PathBuf::from);
            let wt_path = PathBuf::from(&ws.cwd);
            let br_opt = ws.git_branch.clone();
            teardown_and_remove_worktree(src_opt.as_deref(), &wt_path, br_opt.as_deref()).await;
            if let Some(ref src) = src_opt {
                self.reconcile_worktree_pool_after_archive(src).await;
            }
        }

        Ok(())
    }

    async fn resolve_canonical_repo_key(&self, path: &Path) -> String {
        if let Ok((repo_hash, _)) = self.worktree_pool.repo_pool_dir(path).await {
            return format!("git:{repo_hash}");
        }
        tokio::fs::canonicalize(path)
            .await
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    async fn reconcile_worktree_pool_after_archive(&self, source_cwd: &Path) {
        let target_key = self.resolve_canonical_repo_key(source_cwd).await;

        let mut ws_candidates = std::collections::HashSet::new();
        for w in self
            .store
            .list_workspaces_filtered(false, true)
            .unwrap_or_default()
        {
            ws_candidates.insert(w.cwd);
            if let Some(src) = w.source_cwd {
                ws_candidates.insert(src);
            }
        }
        let mut has_active_ref = false;
        for candidate in ws_candidates {
            if self.resolve_canonical_repo_key(Path::new(&candidate)).await == target_key {
                has_active_ref = true;
                break;
            }
        }

        if !has_active_ref {
            let mut agent_candidates = std::collections::HashSet::new();
            for a in self
                .store
                .list_agents_filtered(None, false, true, None)
                .unwrap_or_default()
            {
                if let Some(src) = a.labels.get("orbit.worktree_source_cwd") {
                    agent_candidates.insert(src.clone());
                }
            }
            for candidate in agent_candidates {
                if self.resolve_canonical_repo_key(Path::new(&candidate)).await == target_key {
                    has_active_ref = true;
                    break;
                }
            }
        }

        if !has_active_ref {
            let _ = self.worktree_pool.cleanup_repo_pool(source_cwd).await;
        } else {
            let _ = self.worktree_pool.prune_repo_pool(source_cwd).await;
        }
    }

    pub async fn unarchive_workspace(
        &self,
        workspace_id: &str,
    ) -> anyhow::Result<WorkspaceSummary> {
        self.store.unarchive_workspace(workspace_id)?;
        let ws = self
            .store
            .get_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceUpdated(ws.clone()));
        Ok(ws)
    }

    // --- Workspace Scripts & Services ---

    pub async fn list_workspace_scripts(
        &self,
        workspace_id: &str,
    ) -> anyhow::Result<Vec<WorkspaceScriptInfo>> {
        let ws = self
            .store
            .get_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;
        Ok(self
            .scripts
            .list_for_workspace(
                workspace_id,
                Path::new(&ws.cwd),
                ws.source_cwd.as_deref().map(Path::new),
            )
            .await)
    }

    pub async fn start_workspace_script(
        self: &Arc<Self>,
        workspace_id: &str,
        script_name: &str,
    ) -> anyhow::Result<WorkspaceScriptInfo> {
        let ws = self
            .store
            .get_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;
        let ws_path = PathBuf::from(&ws.cwd);
        let manifest = load_workspace_manifest_with_fallback(
            &ws_path,
            ws.source_cwd.as_deref().map(Path::new),
        );
        let entry = manifest
            .scripts
            .get(script_name)
            .ok_or_else(|| {
                anyhow::anyhow!("script `{script_name}` not found in orbit.json/paseo.json")
            })?
            .clone();

        // Stop previous instance if already running
        if let Some(prev) = self.scripts.unregister(workspace_id, script_name).await
            && let Some(tid) = prev.info.terminal_id
        {
            let _ = self.kill_terminal(&tid).await;
        }

        let kind = entry.script_kind();
        let daemon_port = *self.listen_port.read().await;
        let repo_name = ws
            .source_cwd
            .as_deref()
            .and_then(|s| Path::new(s).file_name().and_then(|n| n.to_str()))
            .or_else(|| ws_path.file_name().and_then(|n| n.to_str()))
            .unwrap_or("workspace");

        let (assigned_port, proxy_host, proxy_url) = if kind == WorkspaceScriptKind::Service {
            let port = match entry.port {
                Some(p) => p,
                None => allocate_ephemeral_port()?,
            };
            let host = build_service_proxy_host(script_name, ws.git_branch.as_deref(), repo_name);
            let url = format!("http://{host}:{daemon_port}");
            (Some(port), Some(host), Some(url))
        } else {
            (None, None, None)
        };

        // Build service environment variables (including peer services + Paseo compatibility aliases)
        let mut env = HashMap::new();
        env.insert("ORBIT_WORKSPACE_ID".into(), workspace_id.to_string());
        env.insert("ORBIT_WORKTREE_PATH".into(), ws.cwd.clone());
        env.insert("PASEO_WORKTREE_PATH".into(), ws.cwd.clone());
        if let Some(ref src) = ws.source_cwd {
            env.insert("ORBIT_SOURCE_CHECKOUT_PATH".into(), src.clone());
            env.insert("PASEO_SOURCE_CHECKOUT_PATH".into(), src.clone());
        }
        if let Some(ref br) = ws.git_branch {
            env.insert("ORBIT_BRANCH_NAME".into(), br.clone());
            env.insert("PASEO_BRANCH_NAME".into(), br.clone());
        }
        if let Some(p) = assigned_port {
            env.insert("PORT".into(), p.to_string());
            env.insert("ORBIT_PORT".into(), p.to_string());
            env.insert("PASEO_PORT".into(), p.to_string());
        }
        if let Some(ref u) = proxy_url {
            env.insert("ORBIT_URL".into(), u.clone());
            env.insert("PASEO_URL".into(), u.clone());
        }
        // Inject sibling running services
        for sib in self
            .scripts
            .list_for_workspace(
                workspace_id,
                &ws_path,
                ws.source_cwd.as_deref().map(Path::new),
            )
            .await
        {
            if sib.kind == WorkspaceScriptKind::Service
                && sib.status == WorkspaceScriptStatus::Running
            {
                let norm: String = sib
                    .name
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() {
                            c.to_ascii_uppercase()
                        } else {
                            '_'
                        }
                    })
                    .collect();
                if let Some(sp) = sib.port {
                    env.insert(format!("ORBIT_SERVICE_{norm}_PORT"), sp.to_string());
                    env.insert(format!("PASEO_SERVICE_{norm}_PORT"), sp.to_string());
                }
                if let Some(su) = sib.proxy_url {
                    env.insert(format!("ORBIT_SERVICE_{norm}_URL"), su.clone());
                    env.insert(format!("PASEO_SERVICE_{norm}_URL"), su);
                }
            }
        }

        let term = self
            .create_terminal_with_command(
                ws.cwd.clone(),
                Some(format!("script:{script_name}")),
                120,
                32,
                Some(entry.command.clone()),
                env,
            )
            .await?;

        let info = WorkspaceScriptInfo {
            workspace_id: workspace_id.to_string(),
            name: script_name.to_string(),
            command: entry.command,
            kind,
            status: WorkspaceScriptStatus::Running,
            terminal_id: Some(term.terminal_id),
            port: assigned_port,
            proxy_url,
            last_error: None,
        };
        self.scripts
            .register_running(workspace_id, script_name, info.clone(), proxy_host)
            .await;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceScriptUpdated(info.clone()));
        Ok(info)
    }

    pub async fn stop_workspace_script(
        &self,
        workspace_id: &str,
        script_name: &str,
    ) -> anyhow::Result<WorkspaceScriptInfo> {
        let removed = self.scripts.unregister(workspace_id, script_name).await;
        if let Some(st) = removed {
            if let Some(ref tid) = st.info.terminal_id {
                let _ = self.kill_terminal(tid).await;
            }
            let mut info = st.info;
            info.status = WorkspaceScriptStatus::Stopped;
            info.terminal_id = None;
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceScriptUpdated(info.clone()));
            return Ok(info);
        }
        let scripts = self.list_workspace_scripts(workspace_id).await?;
        scripts
            .into_iter()
            .find(|s| s.name == script_name)
            .ok_or_else(|| anyhow::anyhow!("script `{script_name}` not found"))
    }

    // --- Agent Lifecycle & Orchestration ---

    pub async fn create_and_start_agent(
        self: &Arc<Self>,
        spec: CreateAgentSpec,
    ) -> anyhow::Result<AgentSummary> {
        // Resolve optional Profile defaults if profile_id is supplied
        let profile = match spec.profile_id.as_deref() {
            Some(pid) => self
                .store
                .get_profile(pid)?
                .or_else(|| self.store.get_profile_by_name(pid).ok().flatten()),
            None => None,
        };

        let provider = profile
            .as_ref()
            .map(|p| p.provider)
            .unwrap_or(spec.provider);
        let acp_agent_id = if provider == ProviderKind::Acp {
            spec.acp_agent_id
                .or_else(|| profile.as_ref().and_then(|p| p.acp_agent_id.clone()))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        } else {
            None
        };
        let model = spec
            .model
            .or_else(|| profile.as_ref().and_then(|p| p.model.clone()));
        let effort = spec
            .effort
            .or_else(|| profile.as_ref().and_then(|p| p.effort));
        let supports_fast = self
            .model_supports_fast_mode(provider, model.as_deref())
            .await;
        let fast_mode = match spec.fast_mode {
            Some(true) => {
                if !supports_fast {
                    anyhow::bail!(
                        "Fast mode is not supported by {} model `{}`",
                        provider.display_name(),
                        model.as_deref().unwrap_or("default")
                    );
                }
                true
            }
            Some(false) => false,
            None => profile.as_ref().is_some_and(|p| p.fast_mode) && supports_fast,
        };
        let mode = if profile.is_some() && matches!(spec.mode, SessionMode::Default) {
            profile
                .as_ref()
                .map(|p| p.mode.clone())
                .unwrap_or(spec.mode)
        } else {
            spec.mode
        };

        let (executable, _acp_spec) = self
            .resolve_launch_executable(provider, acp_agent_id.as_deref())
            .await?;

        let human_seed = derive_human_seed(
            spec.initial_display_prompt.as_deref(),
            &spec.initial_badges,
            &spec.initial_prompt,
        );

        // Inherit workspace & background visibility when invoked by a parent agent
        let parent_agent = match spec.parent_agent_id.as_deref() {
            Some(pid) if spec.parent_host_id.is_none() => self.store.get_agent(pid).ok().flatten(),
            _ => None,
        };
        let effective_background =
            spec.background || parent_agent.as_ref().is_some_and(|p| p.background);

        let computed_title = spec
            .title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| summarize_prompt_title(&human_seed));

        // Derive logical `project_root`: if `extra_dirs` holds a parent project dir that contains
        // `spec.cwd` (e.g. a nested git repo inside a non-git parent project), anchor to that parent.
        let expanded_cwd = crate::expand_host_path(&spec.cwd);
        let expanded_cwd_str = expanded_cwd.to_string_lossy().to_string();
        let inferred_project_root = spec
            .extra_dirs
            .iter()
            .map(|d| crate::expand_host_path(d))
            .find(|p| expanded_cwd.starts_with(p) && *p != expanded_cwd)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| expanded_cwd_str.clone());

        let mut agent_cwd_override: Option<String> = None;
        let mut agent_labels: HashMap<String, String> = HashMap::new();
        let mut cluster_suggestion = None;

        let ws = if let Some(ref ws_id) = spec.workspace_id {
            let existing_ws = self
                .store
                .get_workspace(ws_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id}` not found"))?;
            // Support spawning an isolated worktree peer INSIDE the same logical Mission Workspace!
            if spec.worktree {
                let src_for_wt = if !spec.cwd.trim().is_empty() {
                    crate::expand_host_path(&spec.cwd)
                } else {
                    crate::expand_host_path(
                        existing_ws
                            .source_cwd
                            .as_deref()
                            .unwrap_or(&existing_ws.cwd),
                    )
                };
                if resolve_git_scope(&src_for_wt).await.is_none() {
                    anyhow::bail!(
                        "not_a_git_repo: `{}` is not a Git repository. Disable Worktree or pick a nested Git repository.",
                        src_for_wt.display()
                    );
                }
                let auto_branch = spec
                    .branch
                    .clone()
                    .unwrap_or_else(|| auto_worktree_branch(&human_seed));
                let (wt_path, created_br) = create_worktree_with_mode_pooled(
                    &self.home_dir,
                    &src_for_wt,
                    spec.worktree_mode,
                    Some(&auto_branch),
                    spec.base_ref.as_deref(),
                    spec.pr_number,
                    Some(&self.worktree_pool),
                )
                .await?;
                let wt_str = wt_path.to_string_lossy().to_string();
                agent_labels.insert(
                    "orbit.worktree_source_cwd".into(),
                    src_for_wt.to_string_lossy().to_string(),
                );
                if let Some(br) = created_br {
                    agent_labels.insert("orbit.worktree_branch".into(), br);
                }
                agent_cwd_override = Some(wt_str);
            }
            // Promoting an unclassified container to a curated Mission Workspace when explicitly targeted
            if !existing_ws.is_curated && !effective_background {
                let promoted = self
                    .store
                    .set_workspace_curated(&existing_ws.workspace_id, true)?
                    .unwrap_or(existing_ws);
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::WorkspaceUpdated(promoted.clone()));
                promoted
            } else {
                existing_ws
            }
        } else if spec.worktree {
            let auto_branch = spec
                .branch
                .clone()
                .unwrap_or_else(|| auto_worktree_branch(&human_seed));
            self.create_workspace_v2(CreateWorkspaceSpec {
                source_cwd: spec.cwd.clone(),
                title: Some(computed_title.clone()),
                worktree_branch: Some(auto_branch),
                base_ref: spec.base_ref.clone(),
                isolation: WorkspaceIsolation::Worktree,
                worktree_mode: spec.worktree_mode,
                pr_number: spec.pr_number,
                parent_workspace_id: parent_agent.as_ref().map(|p| p.workspace_id.clone()),
                background: effective_background,
                project_root: Some(inferred_project_root.clone()),
                mission_goal: None,
                allow_reuse_existing: false,
                is_curated: false,
            })
            .await?
        } else if let Some(ref parent) = parent_agent
            && (spec.cwd.trim().is_empty() || spec.cwd == parent.cwd)
            && let Ok(Some(parent_ws)) = self.store.get_workspace(&parent.workspace_id)
        {
            parent_ws
        } else {
            // Top-level local conversation: run two-stage Auto-Clustering when enabled
            let cfg_auto_cluster = self.orch_config.read().await.auto_cluster_enabled;
            let should_eval_cluster = !effective_background
                && parent_agent.is_none()
                && spec.auto_cluster.unwrap_or(cfg_auto_cluster)
                && !human_seed.trim().is_empty();

            let git_branch_now = inspect_git_status(&expanded_cwd).await.branch;
            let mut evaluated = if should_eval_cluster {
                evaluate_auto_cluster(
                    &self.store,
                    &inferred_project_root,
                    &expanded_cwd_str,
                    git_branch_now.as_deref(),
                    &human_seed,
                    &spec.initial_badges,
                    None,
                    true,
                )
            } else {
                None
            };

            if let Some(ref mut sug) = evaluated
                && sug.auto_applied
                && let Ok(Some(target_ws)) = self.store.get_workspace(&sug.target_workspace_id)
            {
                let _ = self.store.update_workspace_goal(
                    &target_ws.workspace_id,
                    None,
                    None,
                    None,
                    Some(true),
                );
                sug.previous_workspace_id = None;
                cluster_suggestion = evaluated;
                self.store
                    .get_workspace(&target_ws.workspace_id)?
                    .unwrap_or(target_ws)
            } else {
                cluster_suggestion = evaluated;
                self.create_workspace_v2(CreateWorkspaceSpec {
                    source_cwd: spec.cwd.clone(),
                    title: Some(computed_title.clone()),
                    worktree_branch: None,
                    base_ref: None,
                    isolation: WorkspaceIsolation::Local,
                    worktree_mode: WorktreeCreateMode::BranchOff,
                    pr_number: None,
                    parent_workspace_id: parent_agent.as_ref().map(|p| p.workspace_id.clone()),
                    background: effective_background,
                    project_root: Some(inferred_project_root.clone()),
                    mission_goal: None,
                    allow_reuse_existing: false,
                    is_curated: false,
                })
                .await?
            }
        };

        let now = Utc::now();
        let agent_id = format!("ag_{}", Uuid::new_v4().simple());
        let final_cwd = agent_cwd_override.unwrap_or_else(|| ws.cwd.clone());

        // Evaluate PrePromptSubmit lifecycle hooks BEFORE persisting the agent, acquiring
        // keep-awake, emitting PeerJoined, or broadcasting AgentUpdated so a blocked initial
        // prompt leaves zero DB rows or ghost UI state behind.
        let mut initial_prompt = spec.initial_prompt;
        if !initial_prompt.trim().is_empty() {
            let hook_outcome = self
                .hooks
                .dispatch_event(
                    orbit_protocol::HookEventKind::PrePromptSubmit,
                    Some(Path::new(&final_cwd)),
                    Some(provider.slug()),
                    serde_json::json!({
                        "event": "pre_prompt_submit",
                        "agent_id": &agent_id,
                        "workspace_id": &ws.workspace_id,
                        "cwd": &final_cwd,
                        "provider": provider.slug(),
                        "prompt": &initial_prompt,
                    }),
                )
                .await;
            if hook_outcome.decision == orbit_protocol::HookDecision::Block {
                let hook_name = hook_outcome
                    .blocking_hook_name
                    .as_deref()
                    .unwrap_or("lifecycle hook");
                let reason = hook_outcome
                    .reason
                    .as_deref()
                    .unwrap_or("blocked by policy");
                anyhow::bail!("Prompt blocked by hook `{hook_name}`: {reason}");
            }
            if hook_outcome.decision == orbit_protocol::HookDecision::Modify
                && let Some(updated) = hook_outcome.updated_text
            {
                initial_prompt = updated;
            }
        }

        let existing_peers = self
            .store
            .list_agents_filtered(Some(&ws.workspace_id), false, false, None)
            .unwrap_or_default();
        if spec.handle.as_deref().is_some_and(|h| !h.trim().is_empty()) {
            agent_labels.insert("orbit.handle_locked".into(), "1".into());
        }
        if spec.title.as_deref().is_some_and(|t| !t.trim().is_empty()) {
            agent_labels.insert("orbit.title_locked".into(), "1".into());
        }
        let allocated_handle = allocate_unique_handle(
            spec.handle.as_deref(),
            provider,
            profile.as_ref().map(|p| p.name.as_str()),
            Some(&human_seed),
            &existing_peers,
        );

        let extra_strs: Vec<String> = spec
            .extra_dirs
            .into_iter()
            .filter(|s| !s.trim().is_empty() && s.trim() != final_cwd)
            .collect();

        let summary = AgentSummary {
            agent_id: agent_id.clone(),
            workspace_id: ws.workspace_id.clone(),
            cwd: final_cwd,
            title: computed_title,
            provider,
            model,
            effort,
            fast_mode,
            mode,
            status: AgentStatus::Initializing,
            parent_agent_id: spec.parent_agent_id,
            parent_host_id: spec.parent_host_id,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: effective_background || ws.background,
            auto_archive: spec.auto_archive,
            profile_id: profile
                .as_ref()
                .map(|p| p.profile_id.clone())
                .or(spec.profile_id),
            labels: agent_labels,
            attention: AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: Some(allocated_handle.clone()),
            cluster_suggestion,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id,
        };
        self.store.upsert_agent(&summary, None)?;
        self.keep_awake
            .notify_agent_status(&agent_id, &summary.status);
        if !extra_strs.is_empty() {
            let _ = self.store.set_agent_extra_dirs(&agent_id, &extra_strs);
        }
        if !existing_peers.is_empty() && !summary.background {
            if let Ok(ws_ev) = self.store.append_workspace_event(
                &ws.workspace_id,
                Some(&agent_id),
                Some(&allocated_handle),
                WorkspaceEventKind::PeerJoined,
                &format!(
                    "@{} ({}) joined workspace",
                    allocated_handle,
                    provider.display_name()
                ),
                None,
            ) {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::WorkspaceEventRecorded(ws_ev));
            }
            if let Ok(Some(refreshed_ws)) = self.store.get_workspace(&ws.workspace_id) {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::WorkspaceUpdated(refreshed_ws));
            }
        }
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(summary.clone()));
        let launch_gen = self.bump_generation(&agent_id).await;

        // Fire SessionStart lifecycle hooks asynchronously (non-blocking).
        {
            let hooks = Arc::clone(&self.hooks);
            let cwd_hook = PathBuf::from(&summary.cwd);
            let prov_slug = summary.provider.slug().to_string();
            let payload = serde_json::json!({
                "event": "session_start",
                "agent_id": &summary.agent_id,
                "workspace_id": &summary.workspace_id,
                "cwd": &summary.cwd,
                "provider": &prov_slug,
                "model": &summary.model,
                "title": &summary.title,
            });
            tokio::spawn(async move {
                let _ = hooks
                    .dispatch_event(
                        orbit_protocol::HookEventKind::SessionStart,
                        Some(&cwd_hook),
                        Some(&prov_slug),
                        payload,
                    )
                    .await;
            });
        }

        let initial_rule_badges = spec.initial_badges.clone();
        // Persist the initial user prompt IMMEDIATELY (`< 1ms`) so `AgentCreated` returns right away
        // and the user sees their clean message + badges at 0ms without blocking the socket!
        let initial_turn = if !initial_prompt.trim().is_empty() {
            let tid = format!("turn_{}", Uuid::new_v4().simple());
            let visible_text = spec
                .initial_display_prompt
                .unwrap_or_else(|| initial_prompt.clone());
            let ev = self.store.append_timeline_event(
                &agent_id,
                1,
                Some(&tid),
                TimelinePayload::UserPrompt {
                    message_id: format!("u_{}", Uuid::new_v4().simple()),
                    text: visible_text,
                    badges: spec.initial_badges,
                },
            )?;
            let prompt_seq = ev.seq;
            // Authoritative multi-device sync: human's own prompt is already read by the creator.
            if summary.parent_agent_id.is_none() {
                let _ = self.store.mark_agent_read_seq(&agent_id, Some(prompt_seq));
            }
            let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            Some((tid, prompt_seq))
        } else {
            None
        };

        let mgr = Arc::clone(self);
        let summary_bg = summary.clone();
        let ag_id_bg = agent_id.clone();
        tokio::spawn(async move {
            if let Some((ref turn_id, prompt_seq)) = initial_turn
                && mgr.current_generation(&ag_id_bg).await == launch_gen
                && let Some(tree_oid) = capture_git_tree_snapshot(Path::new(&summary_bg.cwd)).await
                && mgr.current_generation(&ag_id_bg).await == launch_gen
            {
                let _ =
                    mgr.store
                        .save_turn_checkpoint(&ag_id_bg, prompt_seq, Some(turn_id), &tree_oid);
            }
            if mgr.current_generation(&ag_id_bg).await != launch_gen {
                return;
            }
            match mgr
                .spawn_and_wire_driver(
                    &summary_bg,
                    executable,
                    None,
                    None,
                    false,
                    None,
                    false,
                    launch_gen,
                )
                .await
            {
                Ok(cmd_tx) => {
                    if mgr.current_generation(&ag_id_bg).await != launch_gen {
                        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
                        return;
                    }
                    if let Some((turn_id, _)) = initial_turn {
                        let sanitized = sanitize_human_prompt_for_wire(&initial_prompt, &turn_id);
                        let with_rules = mgr
                            .augment_prompt_with_path_rules(
                                &ag_id_bg,
                                &initial_rule_badges,
                                &sanitized,
                                sanitized.clone(),
                            )
                            .await;
                        let wire = mgr
                            .augment_prompt_with_workspace_context(&ag_id_bg, with_rules)
                            .await;
                        let _ = cmd_tx
                            .send(DriverCommand::SendPrompt {
                                turn_id,
                                prompt: wire,
                                behavior: ActiveTurnBehavior::Steer,
                            })
                            .await;
                    }
                }
                Err(e) => {
                    if mgr.current_generation(&ag_id_bg).await != launch_gen {
                        return;
                    }
                    mgr.keep_awake.mark_agent_idle(&ag_id_bg);
                    if let Ok(ev) = mgr.store.append_timeline_event(
                        &ag_id_bg,
                        1,
                        None,
                        TimelinePayload::StatusChanged {
                            status: AgentStatus::Error {
                                message: e.to_string(),
                                recoverable: true,
                            },
                        },
                    ) {
                        let _ = mgr.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
                    }
                    let _ = mgr
                        .store
                        .set_agent_attention(&ag_id_bg, AttentionState::Error);
                    if let Ok(Some(ag)) = mgr.store.get_agent(&ag_id_bg) {
                        let _ = mgr.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));
                    }
                }
            }
        });

        Ok(self.store.get_agent(&agent_id)?.unwrap_or(summary))
    }

    /// Fork/branch an existing conversation into a new session (either in the same directory
    /// or inside a freshly provisioned git worktree), atomically cloning timeline history up to
    /// `spec.up_to_seq` (inclusive, or the full timeline when `None`).
    ///
    /// - Full-tail forks (`up_to_seq` is `None` or `>= source.latest_seq`) attempt native upstream
    ///   session forking (`--resume <sid> --fork-session` on Claude, `thread/fork` on Codex,
    ///   `session/fork` when advertised on Antigravity) with graceful fallback to a fresh session
    ///   seeded with compact handoff context built from the cloned events.
    /// - Mid-thread forks (`up_to_seq < source.latest_seq`) ALWAYS start a fresh upstream session
    ///   seeded strictly with handoff context built from the cloned prefix (`<= up_to_seq`) so
    ///   post-fork turns from the parent never leak into the branch.
    pub async fn fork_agent(
        self: &Arc<Self>,
        spec: ForkAgentSpec,
    ) -> anyhow::Result<ForkAgentOutcome> {
        // Preflight validations BEFORE creating any workspace/worktree or mutating SQLite
        if let Some(seq) = spec.up_to_seq {
            i64::try_from(seq).map_err(|_| anyhow::anyhow!("up_to_seq exceeds supported range"))?;
        }

        let source = self
            .store
            .get_agent(&spec.source_agent_id)?
            .ok_or_else(|| anyhow::anyhow!("source agent `{}` not found", spec.source_agent_id))?;

        let initial_prompt_text = spec
            .initial_prompt
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());

        let preflight_executable = if initial_prompt_text.is_some() {
            let (exe, _) = self
                .resolve_launch_executable(source.provider, source.acp_agent_id.as_deref())
                .await?;
            Some(exe)
        } else {
            None
        };

        let source_ws = self.store.get_workspace(&source.workspace_id)?;

        let explicit_title = spec
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let derived_title = explicit_title.clone().unwrap_or_else(|| {
            let trimmed = source.title.trim();
            let base = trimmed
                .strip_suffix(" (fork)")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(if trimmed.is_empty() {
                    "Session"
                } else {
                    trimmed
                });
            format!("{base} (fork)")
        });

        let (target_workspace_id, target_cwd, created_workspace, shared_curated_workspace) =
            match spec.target {
                ForkTarget::SameDirectory => {
                    let reuse_source_ws = source_ws
                        .as_ref()
                        .is_some_and(|w| w.is_curated || w.is_worktree);
                    if reuse_source_ws {
                        let is_curated = source_ws.as_ref().is_some_and(|w| w.is_curated);
                        (
                            source.workspace_id.clone(),
                            source.cwd.clone(),
                            None,
                            is_curated,
                        )
                    } else {
                        // Standalone non-curated local thread: provision a dedicated non-curated
                        // local workspace (matching `create_and_start_agent`) so archiving one
                        // thread never archives the other.
                        let project_root = source_ws
                            .as_ref()
                            .and_then(|w| w.project_root.clone())
                            .or_else(|| Some(source.cwd.clone()));
                        let new_ws = self
                            .create_workspace_v2(CreateWorkspaceSpec {
                                source_cwd: source.cwd.clone(),
                                title: Some(derived_title.clone()),
                                worktree_branch: None,
                                base_ref: None,
                                isolation: WorkspaceIsolation::Local,
                                worktree_mode: WorktreeCreateMode::BranchOff,
                                pr_number: None,
                                parent_workspace_id: Some(source.workspace_id.clone()),
                                background: source.background,
                                project_root,
                                mission_goal: None,
                                allow_reuse_existing: false,
                                is_curated: false,
                            })
                            .await?;
                        (
                            new_ws.workspace_id.clone(),
                            new_ws.cwd.clone(),
                            Some(new_ws),
                            false,
                        )
                    }
                }
                ForkTarget::NewWorktree { branch, base_ref } => {
                    let src_for_wt = source_ws
                        .as_ref()
                        .and_then(|w| w.source_cwd.as_deref())
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or(&source.cwd);
                    let expanded_src = crate::expand_host_path(src_for_wt);
                    if resolve_git_scope(&expanded_src).await.is_none() {
                        anyhow::bail!(
                            "not_a_git_repo: `{}` is not a Git repository. Fork in the same directory instead.",
                            expanded_src.display()
                        );
                    }
                    let branch_name =
                        match branch.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                            Some(b) => validate_git_branch_name(&expanded_src, b).await?,
                            None => {
                                format!("orbit/fork-{}", &Uuid::new_v4().simple().to_string()[..6])
                            }
                        };
                    let effective_base_ref =
                        match base_ref.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                            Some(b) => Some(validate_git_base_ref(&expanded_src, b).await?),
                            None => {
                                // If forking an agent already running inside a worktree, default
                                // `base_ref` to that worktree's branch so we branch from its commits
                                // instead of the main checkout's HEAD.
                                if let Some(ref ws) = source_ws
                                    && ws.is_worktree
                                    && let Some(ref wt_branch) = ws.git_branch
                                    && !wt_branch.trim().is_empty()
                                {
                                    validate_git_base_ref(&expanded_src, wt_branch).await.ok()
                                } else {
                                    None
                                }
                            }
                        };
                    let project_root = source_ws
                        .as_ref()
                        .and_then(|w| w.project_root.clone())
                        .or_else(|| Some(expanded_src.to_string_lossy().to_string()));
                    let new_ws = self
                        .create_workspace_v2(CreateWorkspaceSpec {
                            source_cwd: expanded_src.to_string_lossy().to_string(),
                            title: Some(derived_title.clone()),
                            worktree_branch: Some(branch_name),
                            base_ref: effective_base_ref,
                            isolation: WorkspaceIsolation::Worktree,
                            worktree_mode: WorktreeCreateMode::BranchOff,
                            pr_number: None,
                            parent_workspace_id: Some(source.workspace_id.clone()),
                            background: source.background,
                            project_root,
                            mission_goal: None,
                            allow_reuse_existing: false,
                            is_curated: false,
                        })
                        .await?;
                    (
                        new_ws.workspace_id.clone(),
                        new_ws.cwd.clone(),
                        Some(new_ws),
                        false,
                    )
                }
            };

        let now = Utc::now();
        let new_agent_id = format!("ag_{}", Uuid::new_v4().simple());
        let existing_peers = self
            .store
            .list_agents_filtered(Some(&target_workspace_id), false, false, None)
            .unwrap_or_default();
        let allocated_handle = allocate_unique_handle(
            None,
            source.provider,
            None,
            Some(&derived_title),
            &existing_peers,
        );

        let mut labels = HashMap::new();
        if explicit_title.is_some() {
            labels.insert("orbit.title_locked".into(), "1".into());
        }
        if let Some(ref ws) = created_workspace
            && ws.is_worktree
        {
            if let Some(ref br) = ws.git_branch {
                labels.insert("orbit.worktree_branch".into(), br.clone());
            }
            if let Some(ref src) = ws.source_cwd {
                labels.insert("orbit.worktree_source_cwd".into(), src.clone());
            }
        }

        let has_initial_prompt = initial_prompt_text.is_some();

        let initial_summary = AgentSummary {
            agent_id: new_agent_id.clone(),
            workspace_id: target_workspace_id.clone(),
            cwd: target_cwd.clone(),
            title: derived_title,
            provider: source.provider,
            model: source.model.clone(),
            effort: source.effort,
            fast_mode: source.fast_mode,
            mode: source.mode.clone(),
            status: if has_initial_prompt {
                AgentStatus::Initializing
            } else {
                AgentStatus::Idle
            },
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: source.background,
            auto_archive: source.auto_archive,
            profile_id: source.profile_id.clone(),
            labels,
            attention: AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: Some(allocated_handle.clone()),
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: Some(source.agent_id.clone()),
            forked_at_seq: None,
            acp_agent_id: source.acp_agent_id.clone(),
        };

        struct ForkInitialPromptEvent {
            turn_id: String,
            msg_id: String,
            prompt_seq: u64,
            event: TimelineEvent,
        }
        struct ForkDbSetup {
            final_summary: AgentSummary,
            handoff_context: Option<String>,
            fork_from_session_id: Option<String>,
            prompt_event: Option<ForkInitialPromptEvent>,
        }

        // Run all fallible DB writes inside a closure so any failure cleanly rolls back both
        // the inserted agent row AND any newly provisioned workspace/worktree!
        let setup_res: anyhow::Result<ForkDbSetup> = (|| {
            self.store.upsert_agent(&initial_summary, None)?;

            let source_extra_dirs = self
                .store
                .get_agent_extra_dirs(&source.agent_id)
                .unwrap_or_default();
            if !source_extra_dirs.is_empty() {
                let _ = self
                    .store
                    .set_agent_extra_dirs(&new_agent_id, &source_extra_dirs);
            }

            let cloned = self.store.clone_timeline_events_up_to(
                &source.agent_id,
                &new_agent_id,
                spec.up_to_seq,
            )?;

            // Build handoff context STRICTLY from the newly cloned timeline (never the source's tail!).
            let handoff_context = self.build_handoff_context(&new_agent_id);

            // Eliminate TOCTOU leak: native upstream session fork is ONLY eligible when the
            // atomic transaction cloned the true full tail AND the re-fetched source agent is
            // not currently foreground-busy.
            let source_now = self.store.get_agent(&source.agent_id)?;
            let can_native_fork = cloned.cloned_full_tail
                && source_now
                    .as_ref()
                    .is_some_and(|s| !s.status.is_foreground_busy());
            let fork_from_session_id = if can_native_fork {
                self.store
                    .get_persistence_ref(&source.agent_id)?
                    .filter(|s| !s.trim().is_empty())
            } else {
                None
            };

            let prompt_event = if let Some(ref prompt_text) = initial_prompt_text {
                let turn_id = format!("turn_{}", Uuid::new_v4().simple());
                let msg_id = format!("u_{}", Uuid::new_v4().simple());
                let ev = self.store.append_timeline_event(
                    &new_agent_id,
                    1,
                    Some(&turn_id),
                    TimelinePayload::UserPrompt {
                        message_id: msg_id.clone(),
                        text: prompt_text.clone(),
                        badges: Vec::new(),
                    },
                )?;
                let prompt_seq = ev.seq;
                let _ = self
                    .store
                    .mark_agent_read_seq(&new_agent_id, Some(prompt_seq));
                Some(ForkInitialPromptEvent {
                    turn_id,
                    msg_id,
                    prompt_seq,
                    event: ev,
                })
            } else {
                None
            };

            let final_summary = self
                .store
                .get_agent(&new_agent_id)?
                .unwrap_or_else(|| initial_summary.clone());

            Ok(ForkDbSetup {
                final_summary,
                handoff_context,
                fork_from_session_id,
                prompt_event,
            })
        })();

        let ForkDbSetup {
            final_summary,
            handoff_context,
            fork_from_session_id,
            prompt_event,
        } = match setup_res {
            Ok(v) => v,
            Err(err) => {
                let _ = self.store.delete_agent(&new_agent_id);
                if let Some(ref ws) = created_workspace {
                    let _ = self.archive_workspace(&ws.workspace_id).await;
                }
                return Err(err);
            }
        };

        // Emit PeerJoined when joining a curated multi-agent Mission Workspace
        if shared_curated_workspace && !final_summary.background {
            if let Ok(ws_ev) = self.store.append_workspace_event(
                &target_workspace_id,
                Some(&new_agent_id),
                Some(&allocated_handle),
                WorkspaceEventKind::PeerJoined,
                &format!(
                    "@{} ({}) forked into workspace",
                    allocated_handle,
                    final_summary.provider.display_name()
                ),
                None,
            ) {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::WorkspaceEventRecorded(ws_ev));
            }
            if let Ok(Some(refreshed_ws)) = self.store.get_workspace(&target_workspace_id) {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::WorkspaceUpdated(refreshed_ws));
            }
        }

        // Defer AgentUpdated broadcast until all fallible creation/cloning/prompt steps succeed
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(final_summary.clone()));

        if let (
            Some(prompt_text),
            Some(executable),
            Some(ForkInitialPromptEvent {
                turn_id,
                msg_id,
                prompt_seq,
                event: ev,
            }),
        ) = (initial_prompt_text, preflight_executable, prompt_event)
        {
            let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            self.keep_awake
                .notify_agent_status(&new_agent_id, &AgentStatus::Initializing);
            let launch_gen = self.bump_generation(&new_agent_id).await;
            let mgr = Arc::clone(self);
            let summary_bg = final_summary.clone();
            let ag_id_bg = new_agent_id.clone();
            tokio::spawn(async move {
                if mgr.current_generation(&ag_id_bg).await == launch_gen
                    && let Some(tree_oid) =
                        capture_git_tree_snapshot(Path::new(&summary_bg.cwd)).await
                    && mgr.current_generation(&ag_id_bg).await == launch_gen
                {
                    let _ = mgr.store.save_turn_checkpoint(
                        &ag_id_bg,
                        prompt_seq,
                        Some(&turn_id),
                        &tree_oid,
                    );
                }
                if mgr.current_generation(&ag_id_bg).await != launch_gen {
                    return;
                }
                match mgr
                    .spawn_and_wire_driver(
                        &summary_bg,
                        executable,
                        None,
                        fork_from_session_id,
                        false,
                        handoff_context,
                        false,
                        launch_gen,
                    )
                    .await
                {
                    Ok(cmd_tx) => {
                        if mgr.current_generation(&ag_id_bg).await != launch_gen {
                            let _ = cmd_tx.send(DriverCommand::Shutdown).await;
                            return;
                        }
                        let sanitized = sanitize_human_prompt_for_wire(&prompt_text, &msg_id);
                        let with_rules = mgr
                            .augment_prompt_with_path_rules(
                                &ag_id_bg,
                                &[],
                                &sanitized,
                                sanitized.clone(),
                            )
                            .await;
                        let wire = mgr
                            .augment_prompt_with_workspace_context(&ag_id_bg, with_rules)
                            .await;
                        let _ = cmd_tx
                            .send(DriverCommand::SendPrompt {
                                turn_id,
                                prompt: wire,
                                behavior: ActiveTurnBehavior::Steer,
                            })
                            .await;
                    }
                    Err(e) => {
                        if mgr.current_generation(&ag_id_bg).await != launch_gen {
                            return;
                        }
                        mgr.keep_awake.mark_agent_idle(&ag_id_bg);
                        if let Ok(ev) = mgr.store.append_timeline_event(
                            &ag_id_bg,
                            1,
                            None,
                            TimelinePayload::StatusChanged {
                                status: AgentStatus::Error {
                                    message: e.to_string(),
                                    recoverable: true,
                                },
                            },
                        ) {
                            let _ = mgr.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
                        }
                        let _ = mgr
                            .store
                            .set_agent_attention(&ag_id_bg, AttentionState::Error);
                        if let Ok(Some(ag)) = mgr.store.get_agent(&ag_id_bg) {
                            let _ = mgr.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));
                        }
                    }
                }
            });
        } else if fork_from_session_id.is_some()
            && let Ok((executable, _)) = self
                .resolve_launch_executable(
                    final_summary.provider,
                    final_summary.acp_agent_id.as_deref(),
                )
                .await
        {
            // Eagerly snapshot the parent's upstream session for full-tail forks before the
            // parent thread diverges with new turns. `native_fork_only = true` ensures that if
            // upstream native fork fails or is unsupported, we NEVER persist a blank fallback
            // session ID — letting the user's first prompt spawn fresh with `handoff_context`.
            let launch_gen = self.bump_generation(&new_agent_id).await;
            let mgr = Arc::clone(self);
            let summary_bg = final_summary.clone();
            let ag_id_bg = new_agent_id.clone();
            tokio::spawn(async move {
                if mgr.current_generation(&ag_id_bg).await != launch_gen {
                    return;
                }
                if let Err(e) = mgr
                    .spawn_and_wire_driver(
                        &summary_bg,
                        executable,
                        None,
                        fork_from_session_id,
                        true,
                        handoff_context,
                        false,
                        launch_gen,
                    )
                    .await
                {
                    warn!(
                        agent_id = %ag_id_bg,
                        error = %e,
                        "eager native fork driver startup deferred; next user prompt will launch with handoff context"
                    );
                }
            });
        }

        Ok(ForkAgentOutcome {
            agent: final_summary,
            workspace: created_workspace,
        })
    }

    pub async fn send_prompt(
        self: &Arc<Self>,
        agent_id: &str,
        prompt: String,
    ) -> anyhow::Result<()> {
        self.send_prompt_with_options(
            agent_id,
            prompt,
            None,
            Vec::new(),
            ActiveTurnBehavior::Steer,
            None,
        )
        .await
    }

    pub async fn send_prompt_with_meta(
        self: &Arc<Self>,
        agent_id: &str,
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
    ) -> anyhow::Result<()> {
        self.send_prompt_with_options(
            agent_id,
            prompt,
            display_prompt,
            badges,
            ActiveTurnBehavior::Steer,
            None,
        )
        .await
    }

    pub async fn send_prompt_with_options(
        self: &Arc<Self>,
        agent_id: &str,
        mut prompt: String,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
        behavior: ActiveTurnBehavior,
        message_id: Option<String>,
    ) -> anyhow::Result<()> {
        let existing_agent = self.store.get_agent(agent_id).ok().flatten();
        if let Some(ref ag) = existing_agent {
            let hook_outcome = self
                .hooks
                .dispatch_event(
                    orbit_protocol::HookEventKind::PrePromptSubmit,
                    Some(Path::new(&ag.cwd)),
                    Some(ag.provider.slug()),
                    serde_json::json!({
                        "event": "pre_prompt_submit",
                        "agent_id": agent_id,
                        "workspace_id": &ag.workspace_id,
                        "cwd": &ag.cwd,
                        "provider": ag.provider.slug(),
                        "prompt": &prompt,
                    }),
                )
                .await;
            if hook_outcome.decision == orbit_protocol::HookDecision::Block {
                let hook_name = hook_outcome
                    .blocking_hook_name
                    .as_deref()
                    .unwrap_or("lifecycle hook");
                let reason = hook_outcome
                    .reason
                    .as_deref()
                    .unwrap_or("blocked by policy");
                anyhow::bail!("Prompt blocked by hook `{hook_name}`: {reason}");
            }
            if hook_outcome.decision == orbit_protocol::HookDecision::Modify
                && let Some(updated) = hook_outcome.updated_text
            {
                prompt = updated;
            }
        }

        // Sending a human prompt acknowledges unread attention and unarchives if needed
        if let Some(ref ag) = existing_agent
            && ag.archived_at.is_some()
        {
            let _ = self.unarchive_agent(agent_id).await;
        }
        let _ = self
            .store
            .set_agent_attention(agent_id, AttentionState::None);
        let _ = self.store.touch_agent_activity(agent_id, Utc::now());

        let needs_handoff = !self.live_agents.read().await.contains_key(agent_id)
            && self.store.get_persistence_ref(agent_id)?.is_none();
        let handoff_context = if needs_handoff {
            self.build_handoff_context(agent_id)
        } else {
            None
        };
        let _ = self.get_or_resume_driver(agent_id).await?;
        self.dispatch_user_prompt_with_handoff(
            agent_id,
            prompt,
            display_prompt,
            badges,
            handoff_context,
            behavior,
            message_id,
        )
        .await
    }

    /// Server-authoritative queued follow-up prompt:
    /// - If the agent is foreground-idle (including background-only waiting), dispatches immediately.
    /// - Otherwise persists in SQLite and broadcasts `AgentUpdated` to all connected clients.
    pub async fn enqueue_user_prompt(
        self: &Arc<Self>,
        agent_id: &str,
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
        queue_id: Option<String>,
    ) -> anyhow::Result<()> {
        let Some(ag) = self.store.get_agent(agent_id)? else {
            anyhow::bail!("agent `{agent_id}` not found");
        };
        let is_busy = ag.status.is_foreground_busy()
            || matches!(ag.status, AgentStatus::AwaitingPermission { .. })
            || self.has_pending_permissions(agent_id).await;
        if !is_busy {
            return self
                .send_prompt_with_options(
                    agent_id,
                    prompt,
                    display_prompt,
                    badges,
                    ActiveTurnBehavior::Steer,
                    None,
                )
                .await;
        }

        let qid = queue_id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("q_{}", Uuid::new_v4().simple()));
        let visible = display_prompt.unwrap_or_else(|| prompt.clone());
        let saved = self
            .store
            .enqueue_agent_prompt(agent_id, &qid, &prompt, &visible, &badges)?;

        // Re-check status after persisting to close the lost-wakeup race where the agent
        // became foreground-idle between our initial check and SQLite insert.
        let still_busy = self.store.get_agent(agent_id)?.is_some_and(|cur| {
            cur.status.is_foreground_busy()
                || matches!(cur.status, AgentStatus::AwaitingPermission { .. })
        }) || self.has_pending_permissions(agent_id).await;
        if !still_busy
            && let Some(item) = self.store.take_queued_prompt(agent_id, &saved.queue_id)?
        {
            if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::AgentUpdated(updated));
            }
            return self
                .send_prompt_with_options(
                    agent_id,
                    item.wire_prompt,
                    Some(item.display_prompt),
                    item.badges,
                    ActiveTurnBehavior::Steer,
                    None,
                )
                .await;
        }

        if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    pub async fn remove_queued_user_prompt(
        &self,
        agent_id: &str,
        queue_id: &str,
    ) -> anyhow::Result<()> {
        let removed = self.store.remove_queued_prompt(agent_id, queue_id)?;
        if removed && let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    pub async fn flush_queued_user_prompt_now(
        self: &Arc<Self>,
        agent_id: &str,
        queue_id: &str,
    ) -> anyhow::Result<()> {
        let Some(item) = self.store.take_queued_prompt(agent_id, queue_id)? else {
            return Ok(());
        };
        if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        let should_interrupt = self
            .store
            .get_agent(agent_id)
            .ok()
            .flatten()
            .is_some_and(|ag| {
                ag.status.is_foreground_busy()
                    || matches!(ag.status, AgentStatus::AwaitingPermission { .. })
            });
        if should_interrupt {
            let _ = self.interrupt_agent(agent_id).await;
        }
        self.send_prompt_with_options(
            agent_id,
            item.wire_prompt,
            Some(item.display_prompt),
            item.badges,
            ActiveTurnBehavior::Steer,
            None,
        )
        .await
    }

    pub async fn reorder_queued_user_prompts(
        &self,
        agent_id: &str,
        queue_ids: &[String],
    ) -> anyhow::Result<()> {
        let changed = self.store.reorder_queued_prompts(agent_id, queue_ids)?;
        if changed && let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    /// Dispatch a coalesced batch of agent-originated messages / finish notifications / heartbeats.
    pub async fn dispatch_coalesced_batch(
        self: &Arc<Self>,
        batch: CoalescedDispatchBatch,
    ) -> anyhow::Result<()> {
        let Some(ag) = self.store.get_agent(&batch.recipient_id)? else {
            anyhow::bail!("recipient agent `{}` not found", batch.recipient_id);
        };
        if ag.archived_at.is_some() {
            if batch.unarchive {
                let _ = self.unarchive_agent(&batch.recipient_id).await;
            } else {
                anyhow::bail!("recipient agent `{}` is archived", batch.recipient_id);
            }
        }
        if let Some(ref new_mode) = batch.session_mode {
            let _ = self
                .set_session_mode(&batch.recipient_id, new_mode.clone())
                .await;
        }

        let _ = self
            .store
            .touch_agent_activity(&batch.recipient_id, Utc::now());
        let cmd_tx = self.get_or_resume_driver(&batch.recipient_id).await?;
        let turn_id = format!("turn_{}", Uuid::new_v4().simple());

        // Persist structured `TimelinePayload::AgentMessage` cards for each entry in the coalesced batch
        for entry in &batch.entries {
            let ev = self.store.append_timeline_event(
                &batch.recipient_id,
                1,
                Some(&turn_id),
                TimelinePayload::AgentMessage {
                    message_id: entry.message_id.clone(),
                    kind: entry.source.kind,
                    source_agent_id: entry.source.source_agent_id.clone(),
                    source_host_id: entry.source.source_host_id.clone(),
                    source_title: entry.source.source_title.clone(),
                    event: entry.source.event.clone(),
                    text: entry.body.clone(),
                    permission_context: entry.source.permission_context.clone(),
                },
            )?;
            let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
        }

        let wire_prompt = self
            .augment_prompt_with_workspace_context(&batch.recipient_id, batch.wire_prompt)
            .await;

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id,
                prompt: wire_prompt,
                behavior: batch.active_turn_behavior,
            })
            .await
            .map_err(|_| anyhow::anyhow!("agent session channel closed"))?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn dispatch_user_prompt_with_handoff(
        &self,
        agent_id: &str,
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
        handoff_context: Option<String>,
        behavior: ActiveTurnBehavior,
        message_id: Option<String>,
    ) -> anyhow::Result<()> {
        let cmd_tx = self
            .live_agent_tx(agent_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("agent session not active"))?;
        let turn_id = format!("turn_{}", Uuid::new_v4().simple());
        let msg_id = message_id.unwrap_or_else(|| format!("u_{}", Uuid::new_v4().simple()));
        let visible_text = display_prompt.unwrap_or_else(|| prompt.clone());
        let rule_badges = badges.clone();
        let ev = self.store.append_timeline_event(
            agent_id,
            1,
            Some(&turn_id),
            TimelinePayload::UserPrompt {
                message_id: msg_id.clone(),
                text: visible_text,
                badges,
            },
        )?;
        let prompt_seq = ev.seq;
        // Advance authoritative read_seq on human prompt so other devices never flash unread for user input
        if let Ok(Some(read_ag)) = self.store.mark_agent_read_seq(agent_id, Some(prompt_seq)) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(read_ag));
        }
        let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));

        let sanitized = sanitize_human_prompt_for_wire(&prompt, &msg_id);
        // Capture a zero-side-effect pre-turn workspace tree snapshot before the agent mutates files.
        if let Ok(Some(ag)) = self.store.get_agent(agent_id)
            && let Some(tree_oid) = capture_git_tree_snapshot(Path::new(&ag.cwd)).await
        {
            let _ =
                self.store
                    .save_turn_checkpoint(agent_id, prompt_seq, Some(&turn_id), &tree_oid);
        }
        let with_handoff = match handoff_context {
            Some(ctx) => format!("{ctx}\n\n[Latest User Request]\n{sanitized}"),
            None => sanitized.clone(),
        };
        let with_rules = self
            .augment_prompt_with_path_rules(agent_id, &rule_badges, &sanitized, with_handoff)
            .await;
        let wire_prompt = self
            .augment_prompt_with_workspace_context(agent_id, with_rules)
            .await;

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id,
                prompt: wire_prompt,
                behavior,
            })
            .await
            .map_err(|_| anyhow::anyhow!("agent session channel closed"))?;
        Ok(())
    }

    fn build_handoff_context(&self, agent_id: &str) -> Option<String> {
        let events = self.store.fetch_recent_timeline(agent_id, 200).ok()?;
        let mut turns = Vec::new();
        for ev in events {
            match ev.payload {
                TimelinePayload::UserPrompt { text, badges, .. } => {
                    if !text.trim().is_empty() {
                        turns.push(format!("User: {text}"));
                    } else if !badges.is_empty() {
                        let summary: Vec<&str> = badges.iter().map(|b| b.label.as_str()).collect();
                        turns.push(format!("User: {}", summary.join(" ")));
                    }
                }
                TimelinePayload::AssistantMessage {
                    text,
                    streaming: false,
                    ..
                } => {
                    let clipped: String = text.chars().take(1200).collect();
                    turns.push(format!("Assistant: {clipped}"));
                }
                _ => {}
            }
        }
        if turns.is_empty() {
            return None;
        }
        let start = turns.len().saturating_sub(12);
        Some(format!(
            "[Prior Conversation Context in this Workspace]\n{}",
            turns[start..].join("\n\n")
        ))
    }

    pub async fn interrupt_agent(&self, agent_id: &str) -> anyhow::Result<()> {
        self.suppress_auto_drain
            .write()
            .await
            .insert(agent_id.to_string());
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx.send(DriverCommand::Interrupt).await;
        }
        let cancelled_perms: Vec<String> = {
            let mut guard = self.pending_permissions.write().await;
            let req_ids: Vec<String> = guard
                .keys()
                .filter(|(aid, _)| aid == agent_id)
                .map(|(_, rid)| rid.clone())
                .collect();
            guard.retain(|(aid, _), _| aid != agent_id);
            req_ids
        };
        for request_id in cancelled_perms {
            if let Ok(ev) = self.store.append_timeline_event(
                agent_id,
                1,
                None,
                TimelinePayload::PermissionResolved {
                    request_id,
                    decision: "cancelled".into(),
                },
            ) {
                let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            }
        }
        let cancelled_questions = self
            .pending_questions
            .write()
            .await
            .remove(agent_id)
            .unwrap_or_default();
        for request_id in cancelled_questions {
            if let Ok(ev) = self.store.append_timeline_event(
                agent_id,
                1,
                None,
                TimelinePayload::QuestionResolved {
                    request_id,
                    answers: Vec::new(),
                },
            ) {
                let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            }
        }
        Ok(())
    }

    pub async fn set_session_mode(&self, agent_id: &str, mode: SessionMode) -> anyhow::Result<()> {
        self.store.update_agent_mode(agent_id, &mode)?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx.send(DriverCommand::SetMode { mode }).await;
        }
        if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    async fn model_supports_fast_mode(&self, provider: ProviderKind, model: Option<&str>) -> bool {
        let cached = self.provider_catalogs.read().await;
        provider_model_supports_fast_mode(provider, model, cached.as_deref())
    }

    pub async fn set_agent_config(
        &self,
        agent_id: &str,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
    ) -> anyhow::Result<()> {
        let clear_fast = if let Some(existing) = self.store.get_agent(agent_id)? {
            let supports_fast = self
                .model_supports_fast_mode(existing.provider, model.as_deref())
                .await;
            if existing.fast_mode && !supports_fast {
                Some(false)
            } else {
                None
            }
        } else {
            None
        };
        self.store
            .update_agent_config(agent_id, model.as_deref(), effort, clear_fast)?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx
                .send(DriverCommand::SetConfig {
                    model,
                    effort,
                    fast_mode: clear_fast,
                })
                .await;
        }
        if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    pub async fn set_agent_model(
        &self,
        agent_id: &str,
        model: Option<String>,
    ) -> anyhow::Result<()> {
        let clear_fast = if let Some(existing) = self.store.get_agent(agent_id)? {
            let supports_fast = self
                .model_supports_fast_mode(existing.provider, model.as_deref())
                .await;
            if existing.fast_mode && !supports_fast {
                Some(false)
            } else {
                None
            }
        } else {
            None
        };
        self.store
            .update_agent_model(agent_id, model.as_deref(), clear_fast)?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx
                .send(DriverCommand::SetConfig {
                    model,
                    effort: None,
                    fast_mode: clear_fast,
                })
                .await;
        }
        if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_agent(
        &self,
        agent_id: &str,
        title: Option<String>,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
        fast_mode: Option<bool>,
        mode: Option<SessionMode>,
        labels: Option<HashMap<String, String>>,
        handle: Option<String>,
        task_summary: Option<String>,
    ) -> anyhow::Result<AgentSummary> {
        let existing = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        let effective_model = model.as_deref().or(existing.model.as_deref());
        let supports_fast = self
            .model_supports_fast_mode(existing.provider, effective_model)
            .await;
        let effective_fast_mode = match fast_mode {
            Some(true) => {
                if !supports_fast {
                    anyhow::bail!(
                        "Fast mode is not supported by {} model `{}`",
                        existing.provider.display_name(),
                        effective_model.unwrap_or("default")
                    );
                }
                Some(true)
            }
            Some(false) => Some(false),
            None => {
                if model.is_some() && existing.fast_mode && !supports_fast {
                    Some(false)
                } else {
                    None
                }
            }
        };

        let clean_handle = handle.map(|h| sanitize_handle(&h));
        let mut effective_labels = labels;
        // Explicit user/API title edits automatically lock the title against future AI auto-naming
        // unless the caller explicitly sets `orbit.title_locked` (e.g. `""` to unlock).
        if title.as_deref().is_some_and(|t| !t.trim().is_empty()) {
            let map = effective_labels.get_or_insert_with(HashMap::new);
            map.entry("orbit.title_locked".into())
                .or_insert_with(|| "1".into());
        }
        self.store.update_agent_fields(
            agent_id,
            title.as_deref(),
            model.as_deref().map(Some),
            effort.map(Some),
            mode.as_ref(),
            effective_labels.as_ref(),
            clean_handle.as_deref(),
            task_summary.as_deref(),
            effective_fast_mode,
        )?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            if let Some(md) = mode {
                let _ = tx.send(DriverCommand::SetMode { mode: md }).await;
            }
            if model.is_some() || effort.is_some() || effective_fast_mode.is_some() {
                let _ = tx
                    .send(DriverCommand::SetConfig {
                        model,
                        effort,
                        fast_mode: effective_fast_mode,
                    })
                    .await;
            }
        }
        let updated = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(updated.clone()));
        Ok(updated)
    }

    pub async fn switch_agent_provider(
        &self,
        agent_id: &str,
        provider: ProviderKind,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
        acp_agent_id: Option<String>,
    ) -> anyhow::Result<()> {
        let (_, acp_spec) = self
            .resolve_launch_executable(provider, acp_agent_id.as_deref())
            .await?;

        let prev_fast_mode = self
            .store
            .get_agent(agent_id)?
            .map(|a| a.fast_mode)
            .unwrap_or(false);
        let supports_fast = self
            .model_supports_fast_mode(provider, model.as_deref())
            .await;
        let next_fast_mode = prev_fast_mode && supports_fast;
        if prev_fast_mode
            && !next_fast_mode
            && let Some(tx) = self.live_agent_tx(agent_id).await
        {
            let _ = tx
                .send(DriverCommand::SetConfig {
                    model: None,
                    effort: None,
                    fast_mode: Some(false),
                })
                .await;
        }

        let _ = self.stop_and_wait_driver(agent_id).await;
        self.store.switch_agent_provider(
            agent_id,
            provider,
            model.as_deref(),
            effort,
            next_fast_mode,
            acp_agent_id.as_deref(),
        )?;

        let switched_label = acp_spec
            .as_ref()
            .map(|s| s.name.as_str())
            .unwrap_or_else(|| provider.display_name());
        if let Ok(ev) = self.store.append_timeline_event(
            agent_id,
            1,
            None,
            TimelinePayload::Notice {
                level: "info".into(),
                message: format!("已切换至 {switched_label} 接力当前对话"),
            },
        ) {
            let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
        }
        if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    async fn record_pending_request(
        map: &RwLock<HashMap<String, Vec<String>>>,
        agent_id: &str,
        request_id: &str,
    ) {
        let mut guard = map.write().await;
        let list = guard.entry(agent_id.to_string()).or_default();
        if !list.iter().any(|id| id == request_id) {
            list.push(request_id.to_string());
        }
    }

    async fn remove_pending_request(
        map: &RwLock<HashMap<String, Vec<String>>>,
        agent_id: &str,
        request_id: &str,
    ) {
        let mut guard = map.write().await;
        if let Some(list) = guard.get_mut(agent_id) {
            list.retain(|id| id != request_id);
            if list.is_empty() {
                guard.remove(agent_id);
            }
        }
    }

    async fn clear_pending_requests(&self, agent_id: &str) {
        self.pending_permissions
            .write()
            .await
            .retain(|(aid, _), _| aid != agent_id);
        self.pending_questions.write().await.remove(agent_id);
    }

    pub async fn resolve_permission(
        &self,
        agent_id: &str,
        request_id: String,
        decision: String,
    ) -> anyhow::Result<()> {
        self.pending_permissions
            .write()
            .await
            .remove(&(agent_id.to_string(), request_id.clone()));
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            tx.send(DriverCommand::ResolvePermission {
                request_id: request_id.clone(),
                decision: decision.clone(),
            })
            .await
            .map_err(|_| anyhow::anyhow!("agent session not active"))?;

            if let Ok(ev) = self.store.append_timeline_event(
                agent_id,
                1,
                None,
                TimelinePayload::PermissionResolved {
                    request_id,
                    decision,
                },
            ) {
                let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            }
            if !self.has_pending_permissions(agent_id).await {
                let _ = self
                    .store
                    .set_agent_attention(agent_id, AttentionState::None);
            }
        }
        Ok(())
    }

    pub async fn answer_question(
        &self,
        agent_id: &str,
        request_id: String,
        answers: Vec<Vec<String>>,
    ) -> anyhow::Result<()> {
        self.pending_permissions
            .write()
            .await
            .remove(&(agent_id.to_string(), request_id.clone()));
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            tx.send(DriverCommand::AnswerQuestion {
                request_id: request_id.clone(),
                answers: answers.clone(),
            })
            .await
            .map_err(|_| anyhow::anyhow!("agent session not active"))?;

            Self::remove_pending_request(&self.pending_questions, agent_id, &request_id).await;

            if let Ok(ev) = self.store.append_timeline_event(
                agent_id,
                1,
                None,
                TimelinePayload::QuestionResolved {
                    request_id,
                    answers,
                },
            ) {
                let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            }
        }
        Ok(())
    }

    /// Soft-archive an agent, closing its runtime and cascading to same-workspace non-open-tab subagents.
    pub fn archive_agent<'a>(
        self: &'a Arc<Self>,
        agent_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let existing = self.store.get_agent(agent_id)?;
            let parent_ws_id = existing.as_ref().map(|a| a.workspace_id.clone());

            // 1. Stop live runtime & clear pending permissions
            let _ = self.stop_and_wait_driver(agent_id).await;

            // 2. Retire any heartbeats bound to this agent
            if let Ok(removed_hbs) = self.store.delete_heartbeats_for_agent(agent_id) {
                for hb_id in removed_hbs {
                    let _ = self
                        .broadcast_tx
                        .send(DaemonBroadcast::ScheduleDeleted(hb_id));
                }
            }

            // 3. Soft-archive record in SQLite & broadcast
            self.store.archive_agent_soft(agent_id)?;
            if let Some(ref ag) = existing {
                let hooks = Arc::clone(&self.hooks);
                let cwd_hook = PathBuf::from(&ag.cwd);
                let prov_slug = ag.provider.slug().to_string();
                let payload = serde_json::json!({
                    "event": "session_end",
                    "agent_id": agent_id,
                    "workspace_id": &ag.workspace_id,
                    "cwd": &ag.cwd,
                    "provider": &prov_slug,
                    "reason": "archived",
                });
                tokio::spawn(async move {
                    let _ = hooks
                        .dispatch_event(
                            orbit_protocol::HookEventKind::SessionEnd,
                            Some(&cwd_hook),
                            Some(&prov_slug),
                            payload,
                        )
                        .await;
                });
            }
            if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
                let wt_src = updated.labels.get("orbit.worktree_source_cwd").cloned();
                let wt_branch = updated.labels.get("orbit.worktree_branch").cloned();
                let wt_cwd = updated.cwd.clone();
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::AgentUpdated(updated));
                // Clean up peer-owned isolated worktree if no other active agent uses it
                if let Some(src) = wt_src {
                    let still_used = self
                        .store
                        .list_agents_filtered(None, false, true, None)
                        .unwrap_or_default()
                        .iter()
                        .any(|a| a.cwd == wt_cwd);
                    if !still_used {
                        teardown_and_remove_worktree(
                            Some(Path::new(&src)),
                            Path::new(&wt_cwd),
                            wt_branch.as_deref(),
                        )
                        .await;
                        self.reconcile_worktree_pool_after_archive(Path::new(&src))
                            .await;
                    }
                }
            }

            // 4. Resolve managed children: detach cross-workspace / open-tab children, cascade-archive the rest
            let children = self.store.list_child_agents(agent_id)?;
            for child in children {
                let cross_ws = parent_ws_id
                    .as_deref()
                    .is_some_and(|pws| pws != child.workspace_id);
                let has_open_tab = child
                    .labels
                    .iter()
                    .any(|(k, v)| k.starts_with("orbit.open_tab.") && v == "true");
                if cross_ws || has_open_tab {
                    let _ = self.detach_agent(&child.agent_id).await;
                } else {
                    let _ = self.archive_agent(&child.agent_id).await;
                }
            }
            Ok(())
        })
    }

    pub async fn unarchive_agent(&self, agent_id: &str) -> anyhow::Result<AgentSummary> {
        self.store.unarchive_agent(agent_id)?;
        let updated = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(updated.clone()));
        Ok(updated)
    }

    pub async fn detach_agent(&self, agent_id: &str) -> anyhow::Result<AgentSummary> {
        self.store.detach_agent(agent_id)?;
        let updated = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(updated.clone()));
        Ok(updated)
    }

    pub async fn kill_agent(&self, agent_id: &str) -> anyhow::Result<()> {
        let _ = self.stop_and_wait_driver(agent_id).await;
        self.store
            .update_agent_status(agent_id, &AgentStatus::Closed)?;
        if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    pub async fn delete_agent(self: &Arc<Self>, agent_id: &str) -> anyhow::Result<()> {
        let _ = self.kill_agent(agent_id).await;
        if let Ok(removed_hbs) = self.store.delete_heartbeats_for_agent(agent_id) {
            for hb_id in removed_hbs {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::ScheduleDeleted(hb_id));
            }
        }
        self.store.delete_agent(agent_id)?;
        self.driver_start_locks.write().await.remove(agent_id);
        self.agent_generations.write().await.remove(agent_id);
        self.roster_fingerprints.write().await.remove(agent_id);
        self.suppress_auto_drain.write().await.remove(agent_id);
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentDeleted(agent_id.to_string()));
        Ok(())
    }

    pub async fn mark_agent_read(
        &self,
        agent_id: &str,
        seen_seq: Option<u64>,
    ) -> anyhow::Result<()> {
        if let Some((updated, changed)) = self
            .store
            .mark_agent_read_seq_with_changed(agent_id, seen_seq)?
            && changed
        {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    pub async fn mark_agent_unread(&self, agent_id: &str) -> anyhow::Result<()> {
        if let Some(updated) = self.store.mark_agent_unread(agent_id)? {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    pub async fn set_agent_pinned(
        &self,
        agent_id: &str,
        pinned: bool,
        pinned_order: Option<i64>,
    ) -> anyhow::Result<()> {
        if let Some(updated) = self
            .store
            .set_agent_pinned(agent_id, pinned, pinned_order)?
        {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    // --- Host-Authoritative Multi-Device Sync (Projects & Order) ---

    pub fn get_host_sync_state(&self) -> anyhow::Result<HostSyncState> {
        Ok(self.store.get_host_sync_state()?)
    }

    pub fn upsert_host_project(&self, project: HostProjectEntry) -> anyhow::Result<HostSyncState> {
        let state = self.store.upsert_host_project(&project)?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::HostSyncStateUpdated(state.clone()));
        Ok(state)
    }

    pub fn delete_host_project(&self, project_id: &str) -> anyhow::Result<HostSyncState> {
        let state = self.store.delete_host_project(project_id)?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::HostSyncStateUpdated(state.clone()));
        Ok(state)
    }

    pub fn set_host_project_order(
        &self,
        project_order: Vec<String>,
        project_sort_mode: Option<String>,
    ) -> anyhow::Result<HostSyncState> {
        let state = self
            .store
            .set_host_project_order(&project_order, project_sort_mode.as_deref())?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::HostSyncStateUpdated(state.clone()));
        Ok(state)
    }

    // --- Mission Workspace Shared Blackboard, Peer Collaboration & Conflict Radar ---

    pub fn get_workspace_board(
        &self,
        workspace_id: &str,
    ) -> anyhow::Result<WorkspaceBoardSnapshot> {
        if let Some(existing) = self.store.get_workspace_board(workspace_id)? {
            return Ok(existing);
        }
        let ws = self
            .store
            .get_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;
        let path_str = board_file_path(&self.home_dir, workspace_id)
            .to_string_lossy()
            .to_string();
        Ok(WorkspaceBoardSnapshot {
            workspace_id: workspace_id.to_string(),
            content: default_board_template(&ws.title, ws.mission_goal.as_deref()),
            revision: ws.board_revision,
            updated_by: None,
            updated_at: ws.updated_at,
            board_path: Some(path_str),
        })
    }

    pub fn update_workspace_board(
        &self,
        workspace_id: &str,
        content: &str,
        section: Option<&str>,
        updated_by: Option<&str>,
        editor_agent_id: Option<&str>,
    ) -> anyhow::Result<WorkspaceBoardSnapshot> {
        let ws = self
            .store
            .get_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;
        let final_content = match section.map(str::trim).filter(|s| !s.is_empty()) {
            Some(sec) => {
                let current = self
                    .store
                    .get_workspace_board(workspace_id)?
                    .map(|b| b.content)
                    .unwrap_or_else(|| {
                        default_board_template(&ws.title, ws.mission_goal.as_deref())
                    });
                upsert_markdown_section(&current, sec, content)
            }
            None => content.to_string(),
        };
        let disk_path = write_board_file(&self.home_dir, workspace_id, &final_content);
        let snap = self.store.upsert_workspace_board(
            workspace_id,
            &final_content,
            updated_by,
            disk_path.as_deref(),
        )?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceBoardUpdated(snap.clone()));
        let event_summary = match section.map(str::trim).filter(|s| !s.is_empty()) {
            Some(sec) => format!(
                "Updated Shared Board section `{sec}` (rev {})",
                snap.revision
            ),
            None => format!("Updated Shared Board (rev {})", snap.revision),
        };
        if let Ok(ev) = self.store.append_workspace_event(
            workspace_id,
            editor_agent_id,
            updated_by,
            WorkspaceEventKind::BoardUpdated,
            &event_summary,
            None,
        ) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
        }
        if let Ok(Some(updated_ws)) = self.store.get_workspace(workspace_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceUpdated(updated_ws));
        }
        Ok(snap)
    }

    pub fn list_workspace_events(
        &self,
        workspace_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<WorkspaceEventRecord>> {
        Ok(self.store.list_workspace_events(workspace_id, limit)?)
    }

    pub fn update_workspace_goal(
        &self,
        workspace_id: &str,
        title: Option<String>,
        mission_goal: Option<String>,
    ) -> anyhow::Result<WorkspaceSummary> {
        let title_locked = title.as_deref().is_some_and(|t| !t.trim().is_empty());
        let updated = self
            .store
            .update_workspace_goal(
                workspace_id,
                title.as_deref(),
                mission_goal.as_deref(),
                title_locked.then_some(true),
                None,
            )?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceUpdated(updated.clone()));
        Ok(updated)
    }

    pub async fn move_agent_to_workspace(
        &self,
        agent_id: &str,
        target_workspace_id: Option<String>,
        new_workspace_title: Option<String>,
    ) -> anyhow::Result<AgentSummary> {
        self.move_agent_to_workspace_v2(agent_id, target_workspace_id, new_workspace_title, false)
            .await
    }

    pub async fn move_agent_to_workspace_v2(
        &self,
        agent_id: &str,
        target_workspace_id: Option<String>,
        new_workspace_title: Option<String>,
        make_unclassified: bool,
    ) -> anyhow::Result<AgentSummary> {
        let agent = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        let target_ws = if make_unclassified {
            let old_ws = self.store.get_workspace(&agent.workspace_id).ok().flatten();
            self.create_workspace_v2(CreateWorkspaceSpec {
                source_cwd: agent.cwd.clone(),
                title: Some(agent.title.clone()),
                worktree_branch: None,
                base_ref: None,
                isolation: WorkspaceIsolation::Local,
                worktree_mode: WorktreeCreateMode::BranchOff,
                pr_number: None,
                parent_workspace_id: None,
                background: agent.background,
                project_root: old_ws.and_then(|w| w.project_root),
                mission_goal: None,
                allow_reuse_existing: false,
                is_curated: false,
            })
            .await?
        } else {
            match target_workspace_id {
                Some(ref ws_id) if !ws_id.trim().is_empty() => {
                    let ws = self
                        .store
                        .get_workspace(ws_id)?
                        .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id}` not found"))?;
                    if !ws.is_curated {
                        self.store
                            .set_workspace_curated(&ws.workspace_id, true)?
                            .unwrap_or(ws)
                    } else {
                        ws
                    }
                }
                _ => {
                    let old_ws = self.store.get_workspace(&agent.workspace_id).ok().flatten();
                    let title = new_workspace_title
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| agent.title.clone());
                    self.create_workspace_v2(CreateWorkspaceSpec {
                        source_cwd: agent.cwd.clone(),
                        title: Some(title),
                        worktree_branch: None,
                        base_ref: None,
                        isolation: WorkspaceIsolation::Local,
                        worktree_mode: WorktreeCreateMode::BranchOff,
                        pr_number: None,
                        parent_workspace_id: None,
                        background: agent.background,
                        project_root: old_ws.and_then(|w| w.project_root),
                        mission_goal: None,
                        allow_reuse_existing: false,
                        is_curated: true,
                    })
                    .await?
                }
            }
        };

        // Ensure unique handle in the destination workspace
        let dest_peers: Vec<AgentSummary> = self
            .store
            .list_agents_filtered(Some(&target_ws.workspace_id), false, false, None)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.agent_id != agent_id)
            .collect();
        let next_handle = allocate_unique_handle(
            agent.handle.as_deref(),
            agent.provider,
            None,
            Some(&agent.title),
            &dest_peers,
        );
        let _ = self.store.set_agent_handle(agent_id, Some(&next_handle));
        let updated = self
            .store
            .move_agent_to_workspace(agent_id, &target_ws.workspace_id, true)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        // Reset cached roster fingerprint so next turn reflects the new workspace peers
        self.roster_fingerprints.write().await.remove(agent_id);
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(updated.clone()));
        if target_ws.is_curated
            && let Ok(ev) = self.store.append_workspace_event(
                &target_ws.workspace_id,
                Some(agent_id),
                Some(&next_handle),
                WorkspaceEventKind::PeerJoined,
                &format!("@{} joined workspace `{}`", next_handle, target_ws.title),
                None,
            )
        {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
        }
        if let Ok(Some(refreshed_ws)) = self.store.get_workspace(&target_ws.workspace_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceUpdated(refreshed_ws));
        }
        Ok(updated)
    }

    pub async fn organize_project_workspaces(
        self: &Arc<Self>,
        request_id: Option<Uuid>,
        project_root: &str,
        agent_ids: Vec<String>,
        auto_apply: bool,
    ) -> anyhow::Result<WorkspaceOrganizeEvent> {
        crate::workspace_organizer::organize_project_workspaces(
            self,
            request_id,
            project_root,
            agent_ids,
            auto_apply,
        )
        .await
    }

    pub async fn summarize_agent(self: &Arc<Self>, agent_id: &str) -> anyhow::Result<AgentSummary> {
        crate::workspace_organizer::summarize_agent_context(self, agent_id).await
    }

    pub async fn merge_workspaces(
        &self,
        source_workspace_id: &str,
        target_workspace_id: &str,
    ) -> anyhow::Result<usize> {
        let src_ws = self
            .store
            .get_workspace(source_workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("source workspace `{source_workspace_id}` not found"))?;
        let dst_ws = self
            .store
            .get_workspace(target_workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("target workspace `{target_workspace_id}` not found"))?;
        let src_agents =
            self.store
                .list_agents_filtered(Some(source_workspace_id), false, true, None)?;
        let mut dst_peers =
            self.store
                .list_agents_filtered(Some(target_workspace_id), false, false, None)?;
        for ag in &src_agents {
            let unique_handle = allocate_unique_handle(
                ag.handle.as_deref(),
                ag.provider,
                None,
                Some(&ag.title),
                &dst_peers,
            );
            let _ = self
                .store
                .set_agent_handle(&ag.agent_id, Some(&unique_handle));
            let mut moved_copy = ag.clone();
            moved_copy.handle = Some(unique_handle);
            dst_peers.push(moved_copy);
            self.roster_fingerprints.write().await.remove(&ag.agent_id);
        }
        // Merge non-empty source board into target board if present
        if let Ok(Some(src_board)) = self.store.get_workspace_board(source_workspace_id)
            && !src_board.content.trim().is_empty()
        {
            let _ = self.update_workspace_board(
                target_workspace_id,
                &src_board.content,
                Some(&format!("Merged from {}", src_ws.title)),
                Some("workspace"),
                None,
            );
        }
        let moved = self
            .store
            .merge_workspaces(source_workspace_id, target_workspace_id)?;
        if let Ok(ev) = self.store.append_workspace_event(
            target_workspace_id,
            None,
            None,
            WorkspaceEventKind::WorkspaceMerged,
            &format!(
                "Merged workspace `{}` ({moved} peer(s)) into `{}`",
                src_ws.title, dst_ws.title
            ),
            None,
        ) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
        }
        for ag in src_agents {
            if let Ok(Some(updated)) = self.store.get_agent(&ag.agent_id) {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::AgentUpdated(updated));
            }
        }
        if let Ok(Some(updated_src)) = self.store.get_workspace(source_workspace_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceUpdated(updated_src));
        }
        if let Ok(Some(updated_dst)) = self.store.get_workspace(target_workspace_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceUpdated(updated_dst));
        }
        Ok(moved)
    }

    pub fn dismiss_cluster_suggestion(&self, agent_id: &str) -> anyhow::Result<()> {
        if let Some(updated) = self.store.set_agent_cluster_suggestion(agent_id, None)? {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    /// Resolve a sibling peer inside `workspace_id` by `@handle`, `agent_id`, or case-insensitive title prefix.
    pub fn resolve_peer_in_workspace(
        &self,
        workspace_id: &str,
        target: &str,
    ) -> anyhow::Result<Option<AgentSummary>> {
        let clean = target.trim().trim_start_matches('@');
        if clean.is_empty() {
            return Ok(None);
        }
        let peers = self
            .store
            .list_agents_filtered(Some(workspace_id), false, true, None)?;
        if let Some(exact_id) = peers.iter().find(|a| a.agent_id == clean) {
            return Ok(Some(exact_id.clone()));
        }
        if let Some(exact_handle) = peers.iter().find(|a| {
            a.handle
                .as_deref()
                .is_some_and(|h| h.eq_ignore_ascii_case(clean))
        }) {
            return Ok(Some(exact_handle.clone()));
        }
        let lower = clean.to_ascii_lowercase();
        Ok(peers
            .into_iter()
            .find(|a| a.title.to_ascii_lowercase().contains(&lower)))
    }

    /// Match workspace/user path-scoped glob rules against mentioned/edited paths and `@rule:` tokens,
    /// prepending `<WORKSPACE_RULES>` cleanly when non-empty and leaving the prompt untouched otherwise.
    /// `signal_source` is strictly the current turn's user prompt (never historical handoff turns).
    async fn augment_prompt_with_path_rules(
        &self,
        agent_id: &str,
        badges: &[PromptBadge],
        signal_source: &str,
        prompt: String,
    ) -> String {
        if prompt.trim_start().starts_with("<WORKSPACE_RULES>") {
            return prompt;
        }
        let Ok(Some(agent)) = self.store.get_agent(agent_id) else {
            return prompt;
        };
        if agent.background {
            return prompt;
        }
        let edited_paths: Vec<String> = self
            .store
            .list_workspace_events(&agent.workspace_id, 24)
            .unwrap_or_default()
            .into_iter()
            .filter(|ev| ev.kind == WorkspaceEventKind::FileEdited)
            .filter_map(|ev| ev.file_path)
            .collect();
        let (active_paths, manual_rules) = crate::rules::extract_prompt_rule_signals(
            signal_source,
            badges,
            &agent.cwd,
            &edited_paths,
        );

        let home_dir = self.home_dir.clone();
        let cwd = PathBuf::from(&agent.cwd);
        let matched = tokio::task::spawn_blocking(move || {
            crate::rules::match_rules_for_workspace(
                &home_dir,
                Some(&cwd),
                &active_paths,
                &manual_rules,
            )
        })
        .await
        .unwrap_or_default();

        if matched.injected_markdown.is_empty() {
            prompt
        } else {
            format!("{}\n\n{prompt}", matched.injected_markdown)
        }
    }

    /// Prepend a deduplicated `<orbit-workspace>` live awareness block (peers + shared board +
    /// recent events) whenever the workspace has 2+ peers or board content that changed since the
    /// last turn sent to `agent_id`.
    async fn augment_prompt_with_workspace_context(
        &self,
        agent_id: &str,
        prompt: String,
    ) -> String {
        let Ok(Some(agent)) = self.store.get_agent(agent_id) else {
            return prompt;
        };
        if agent.background {
            return prompt;
        }
        let Ok(Some(ws)) = self.store.get_workspace(&agent.workspace_id) else {
            return prompt;
        };
        let peers = self
            .store
            .list_agents_filtered(Some(&ws.workspace_id), false, false, None)
            .unwrap_or_default();
        let events = self
            .store
            .list_workspace_events(&ws.workspace_id, 16)
            .unwrap_or_default();
        let board = self
            .store
            .get_workspace_board(&ws.workspace_id)
            .ok()
            .flatten();

        let Some((xml, fingerprint)) = build_workspace_roster_context(
            &self.store,
            &self.home_dir,
            &ws,
            agent_id,
            &peers,
            &events,
            board.as_ref(),
        ) else {
            return prompt;
        };

        let should_inject = {
            let mut guard = self.roster_fingerprints.write().await;
            let changed = guard.get(agent_id).map(String::as_str) != Some(fingerprint.as_str());
            if changed {
                guard.insert(agent_id.to_string(), fingerprint);
            }
            changed
        };

        if should_inject {
            format!("{xml}\n\n{prompt}")
        } else {
            prompt
        }
    }

    /// Track file edits inside a Mission Workspace and emit live Conflict Radar alerts when two
    /// peers sharing the SAME physical working directory edit the same file within 10 minutes.
    async fn record_file_edit_and_check_conflict(&self, agent_id: &str, raw_path: &str) {
        let trimmed = raw_path.trim();
        if trimmed.is_empty() {
            return;
        }
        let Ok(Some(agent)) = self.store.get_agent(agent_id) else {
            return;
        };
        if agent.background {
            return;
        }
        let norm_path = Path::new(trimmed)
            .strip_prefix(&agent.cwd)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| trimmed.trim_start_matches("./").replace('\\', "/"));

        let handle = agent.handle.as_deref().unwrap_or("peer");
        if let Ok(ev) = self.store.append_workspace_event(
            &agent.workspace_id,
            Some(agent_id),
            Some(handle),
            WorkspaceEventKind::FileEdited,
            &format!("@{handle} edited `{norm_path}`"),
            Some(&norm_path),
        ) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
        }

        // Check for active peer conflicts in the SAME workspace AND SAME physical cwd
        let since = Utc::now() - chrono::Duration::minutes(10);
        let Ok(other_editors) = self.store.recent_workspace_file_editors(
            &agent.workspace_id,
            &norm_path,
            agent_id,
            since,
        ) else {
            return;
        };
        if other_editors.is_empty() {
            return;
        }

        let mut conflicting_handles: Vec<String> = Vec::new();
        for (other_id, other_handle) in other_editors {
            if let Ok(Some(other_ag)) = self.store.get_agent(&other_id)
                && other_ag.archived_at.is_none()
                && !other_ag.background
                && other_ag.cwd == agent.cwd
            {
                conflicting_handles.push(
                    other_handle
                        .or(other_ag.handle)
                        .unwrap_or_else(|| other_ag.title.clone()),
                );
            }
        }
        if conflicting_handles.is_empty() {
            return;
        }

        // Throttle per (workspace_id, norm_path) to at most once every 60 seconds
        let key = (agent.workspace_id.clone(), norm_path.clone());
        let should_alert = {
            let mut guard = self.conflict_cooldowns.write().await;
            let now = std::time::Instant::now();
            guard.retain(|_, prev| now.duration_since(*prev) < Duration::from_secs(60));
            match guard.get(&key) {
                Some(_) => false,
                None => {
                    guard.insert(key, now);
                    true
                }
            }
        };
        if !should_alert {
            return;
        }

        let others_str = conflicting_handles
            .iter()
            .map(|h| format!("@{h}"))
            .collect::<Vec<_>>()
            .join(", ");
        let msg = format!(
            "Conflict Radar: @{handle} and {others_str} both edited `{norm_path}` in the same directory (`{}`). Coordinate via `message_peer` / `workspace_board_update` or fork one peer into a worktree.",
            agent.cwd
        );
        if let Ok(ev) = self.store.append_workspace_event(
            &agent.workspace_id,
            Some(agent_id),
            Some(handle),
            WorkspaceEventKind::ConflictWarning,
            &msg,
            Some(&norm_path),
        ) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
        }
        if let Ok(notice_ev) = self.store.append_timeline_event(
            agent_id,
            1,
            None,
            TimelinePayload::Notice {
                level: "warning".into(),
                message: msg,
            },
        ) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::TimelineLive(notice_ev));
        }
    }

    pub async fn list_live_agent_ids(&self) -> Vec<String> {
        self.live_agents.read().await.keys().cloned().collect()
    }

    /// Release an idle resumable provider runtime without archiving or altering `last_activity_at`/`attention`.
    pub async fn close_idle_runtime(&self, agent_id: &str) -> bool {
        let had_live = self.live_agents.read().await.contains_key(agent_id);
        if !had_live {
            return false;
        }
        let _ = self.stop_and_wait_driver(agent_id).await;
        let _ = self
            .store
            .update_agent_status(agent_id, &AgentStatus::Closed);
        if let Ok(Some(updated)) = self.store.get_agent(agent_id) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        true
    }

    pub async fn list_pending_permissions(
        &self,
        agent_id: Option<&str>,
    ) -> Vec<PendingPermissionSummary> {
        let guard = self.pending_permissions.read().await;
        let mut out: Vec<PendingPermissionSummary> = guard
            .values()
            .filter(|p| agent_id.is_none_or(|id| p.agent_id == id))
            .cloned()
            .collect();
        out.sort_by_key(|p| p.created_at);
        out
    }

    pub async fn get_pending_permission(
        &self,
        agent_id: &str,
        request_id: &str,
    ) -> Option<PendingPermissionSummary> {
        self.pending_permissions
            .read()
            .await
            .get(&(agent_id.to_string(), request_id.to_string()))
            .cloned()
    }

    pub async fn has_pending_permissions(&self, agent_id: &str) -> bool {
        self.pending_permissions
            .read()
            .await
            .keys()
            .any(|(aid, _)| aid == agent_id)
    }

    /// Rewind an agent to strictly before `to_before_seq`:
    /// 1. Increments the agent's lifecycle generation, gracefully stops any live provider driver,
    ///    and awaits its shutdown/ingestion drain before touching workspace files or SQLite.
    /// 2. Optionally restores workspace files via the pre-turn Git tree snapshot (or falls back
    ///    to reverse-applying recorded `FileEdit` unified diffs for pre-checkpoint turns).
    /// 3. Atomically truncates SQLite `timeline_events` & `turn_checkpoints`, increments
    ///    `timeline_epoch`, clears `persistence_ref`, and broadcasts `DaemonBroadcast::AgentRewound`
    ///    to all other connected clients.
    pub async fn rewind_agent(
        &self,
        agent_id: &str,
        to_before_seq: u64,
        restore_files: bool,
        origin_session_id: Uuid,
    ) -> anyhow::Result<RewindAgentOutcome> {
        if to_before_seq == 0 {
            anyhow::bail!("invalid rewind target sequence");
        }
        let existing = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;

        let _ = self.stop_and_wait_driver(agent_id).await;

        let mut restored_files: u32 = 0;
        let mut notice: Option<String> = None;

        if restore_files {
            let cwd_path = PathBuf::from(&existing.cwd);
            let checkpoint_oid = self.store.get_turn_checkpoint(agent_id, to_before_seq)?;

            let from_events = self
                .store
                .fetch_timeline_from(agent_id, to_before_seq)
                .unwrap_or_default();
            let fallback_edits: Vec<(String, String)> = from_events
                .iter()
                .filter_map(|ev| match &ev.payload {
                    TimelinePayload::ToolCall {
                        status: ToolCallStatus::Completed,
                        detail:
                            ToolCallDetail::FileEdit {
                                path,
                                unified_diff: Some(diff),
                            },
                        ..
                    } if !diff.trim().is_empty() => Some((path.clone(), diff.clone())),
                    _ => None,
                })
                .collect();

            if let Some(tree_oid) = checkpoint_oid {
                match restore_git_tree_snapshot(&cwd_path, &tree_oid).await {
                    Ok(count) => {
                        restored_files = count;
                    }
                    Err(err) => {
                        if !fallback_edits.is_empty() {
                            restored_files =
                                reverse_apply_file_edit_diffs(&cwd_path, &fallback_edits).await;
                            notice = Some(format!(
                                "Checkpoint unavailable ({err}); reverted {restored_files} file(s) from recorded diffs."
                            ));
                        } else {
                            notice = Some(format!(
                                "Conversation rewound, but workspace checkpoint could not be restored: {err}"
                            ));
                        }
                    }
                }
            } else if !fallback_edits.is_empty() {
                let expected_paths: std::collections::HashSet<&str> =
                    fallback_edits.iter().map(|(p, _)| p.as_str()).collect();
                restored_files = reverse_apply_file_edit_diffs(&cwd_path, &fallback_edits).await;
                if (restored_files as usize) < expected_paths.len() {
                    notice = Some(format!(
                        "Reverted {restored_files} of {} edited file(s) from recorded diffs (some files changed externally).",
                        expected_paths.len()
                    ));
                }
            } else {
                notice = Some(
                    "Conversation rewound. No pre-turn file checkpoint was recorded for this earlier turn."
                        .into(),
                );
            }
        }

        let rewound = self
            .store
            .rewind_agent_timeline(agent_id, to_before_seq)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;

        let outcome = RewindAgentOutcome {
            agent: rewound.agent.clone(),
            to_before_seq,
            events: rewound.events,
            restored_files,
            restored_prompt: rewound.restored_prompt,
            notice,
        };

        let _ = self.broadcast_tx.send(DaemonBroadcast::AgentRewound {
            origin_session_id,
            outcome: outcome.clone(),
        });
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(rewound.agent));

        Ok(outcome)
    }

    async fn agent_start_lock(&self, agent_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        if let Some(lock) = self.driver_start_locks.read().await.get(agent_id) {
            return Arc::clone(lock);
        }
        let mut guard = self.driver_start_locks.write().await;
        Arc::clone(
            guard
                .entry(agent_id.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    async fn get_or_resume_driver(
        self: &Arc<Self>,
        agent_id: &str,
    ) -> anyhow::Result<mpsc::Sender<DriverCommand>> {
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            return Ok(tx);
        }

        let start_lock = self.agent_start_lock(agent_id).await;
        let _start_guard = start_lock.lock().await;

        // Re-check after acquiring the per-agent startup lock so concurrent eager-fork startup
        // and user `send_prompt` share the same live shim without socket collision.
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            return Ok(tx);
        }

        let summary = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        let resume_ref = self.store.get_persistence_ref(agent_id)?;
        let (executable, _) = self
            .resolve_launch_executable(summary.provider, summary.acp_agent_id.as_deref())
            .await?;
        let launch_gen = self.bump_generation(agent_id).await;

        self.spawn_and_wire_driver_locked(
            &summary, executable, resume_ref, None, false, None, false, launch_gen,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn spawn_and_wire_driver(
        self: &Arc<Self>,
        summary: &AgentSummary,
        executable: PathBuf,
        resume_session_id: Option<String>,
        fork_from_session_id: Option<String>,
        native_fork_only: bool,
        explicit_handoff_context: Option<String>,
        reattach_existing_shim: bool,
        expected_gen: u64,
    ) -> anyhow::Result<mpsc::Sender<DriverCommand>> {
        let start_lock = self.agent_start_lock(&summary.agent_id).await;
        let _start_guard = start_lock.lock().await;
        if self.current_generation(&summary.agent_id).await != expected_gen {
            anyhow::bail!("agent session was reset before startup");
        }
        self.spawn_and_wire_driver_locked(
            summary,
            executable,
            resume_session_id,
            fork_from_session_id,
            native_fork_only,
            explicit_handoff_context,
            reattach_existing_shim,
            expected_gen,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn spawn_and_wire_driver_locked(
        self: &Arc<Self>,
        summary: &AgentSummary,
        executable: PathBuf,
        resume_session_id: Option<String>,
        fork_from_session_id: Option<String>,
        native_fork_only: bool,
        explicit_handoff_context: Option<String>,
        reattach_existing_shim: bool,
        expected_gen: u64,
    ) -> anyhow::Result<mpsc::Sender<DriverCommand>> {
        let primary_sock = self.shim_socket_path("a", &summary.agent_id);
        let legacy_sock = self.legacy_shim_socket_path("agent", &summary.agent_id);
        let shim_socket =
            if reattach_existing_shim && !primary_sock.exists() && legacy_sock.exists() {
                legacy_sock
            } else {
                primary_sock
            };

        let mut extra_dirs: Vec<PathBuf> = self
            .store
            .get_agent_extra_dirs(&summary.agent_id)
            .unwrap_or_default()
            .into_iter()
            .map(|s| crate::expand_host_path(&s))
            .collect();

        let attachments_dir = self.home_dir.join("attachments");
        let _ = std::fs::create_dir_all(&attachments_dir);
        if !extra_dirs.contains(&attachments_dir) {
            extra_dirs.push(attachments_dir.clone());
        }

        // Load profile policy & system prompt overlay
        let profile = summary
            .profile_id
            .as_deref()
            .and_then(|pid| self.store.get_profile(pid).ok().flatten());
        let orch_cfg = self.orch_config.read().await.clone();
        let mcp_enabled = orch_cfg.mcp_inject_enabled
            && profile
                .as_ref()
                .map(|p| p.mcp_policy.enabled)
                .unwrap_or(true);
        let daemon_port = *self.listen_port.read().await;

        let mcp_spec = if mcp_enabled && daemon_port > 0 {
            Some(OrbitMcpLaunchSpec {
                server_name: "orbit".to_string(),
                http_url: format!(
                    "http://127.0.0.1:{daemon_port}/mcp/agents?caller_agent_id={}",
                    summary.agent_id
                ),
                bearer_token: self.mcp_token.clone(),
                stdio_command: self.orbit_exe.clone(),
                stdio_args: vec![
                    "--home".into(),
                    self.home_dir.to_string_lossy().to_string(),
                    "mcp".into(),
                    "--caller-agent-id".into(),
                    summary.agent_id.clone(),
                ],
            })
        } else {
            None
        };

        let mut extra_env = HashMap::new();
        extra_env.insert("ORBIT_AGENT_ID".into(), summary.agent_id.clone());
        extra_env.insert("ORBIT_WORKSPACE_ID".into(), summary.workspace_id.clone());
        extra_env.insert("ORBIT_HOST_ID".into(), self.host_id.clone());
        if let Some(ref parent_id) = summary.parent_agent_id {
            extra_env.insert("ORBIT_PARENT_AGENT_ID".into(), parent_id.clone());
        }
        if let Some(ref parent_host) = summary.parent_host_id {
            extra_env.insert("ORBIT_PARENT_HOST_ID".into(), parent_host.clone());
        }
        extra_env.insert(
            "ORBIT_HOME".into(),
            self.home_dir.to_string_lossy().to_string(),
        );
        if daemon_port > 0 {
            extra_env.insert(
                "ORBIT_SERVER".into(),
                format!("ws://127.0.0.1:{daemon_port}/ws"),
            );
        }

        let persisted_running_bg_tasks: Vec<PersistedBackgroundTask> = self
            .store
            .list_running_background_tool_calls(&summary.agent_id)
            .unwrap_or_default()
            .into_iter()
            .map(|(call_id, detail, info)| PersistedBackgroundTask {
                call_id,
                detail,
                info,
            })
            .collect();

        let persisted_bg_wait = matches!(
            &summary.status,
            AgentStatus::AutonomousWakeup { source, .. }
                if source.trim().starts_with("waiting:")
        );

        let shim_after_seq = if reattach_existing_shim {
            self.store.get_agent_shim_cursor(&summary.agent_id)
        } else {
            let _ = self.store.clear_agent_shim_cursor(&summary.agent_id);
            None
        };
        let shim_seq_tracker = Arc::new(AtomicU64::new(shim_after_seq.unwrap_or(0)));
        let persisted_active_turn_id = if reattach_existing_shim {
            match &summary.status {
                AgentStatus::Running { turn_id } => Some(turn_id.clone()),
                _ => None,
            }
        } else {
            None
        };
        let persisted_turn_step_seq = persisted_active_turn_id
            .as_deref()
            .map(|tid| self.store.max_turn_segment_step(&summary.agent_id, tid))
            .unwrap_or(0);
        let fallback_handoff_context = explicit_handoff_context.or_else(|| {
            if !reattach_existing_shim
                && (resume_session_id.is_some()
                    || fork_from_session_id.is_some()
                    || summary.latest_seq > 0)
            {
                self.build_handoff_context(&summary.agent_id)
            } else {
                None
            }
        });

        let acp_spec = if summary.provider == ProviderKind::Acp {
            summary
                .acp_agent_id
                .as_deref()
                .and_then(|id| orbit_providers::get_acp_agent_spec(&self.home_dir, id))
        } else {
            None
        };

        let handle = spawn_provider_session(SessionLaunchConfig {
            agent_id: summary.agent_id.clone(),
            provider: summary.provider,
            executable,
            orbit_exe: self.orbit_exe.clone(),
            shim_socket,
            cwd: PathBuf::from(&summary.cwd),
            extra_dirs,
            attachments_dir: Some(attachments_dir),
            model: summary.model.clone(),
            effort: summary.effort,
            fast_mode: summary.fast_mode,
            mode: summary.mode.clone(),
            resume_session_id,
            fork_from_session_id,
            native_fork_only,
            mcp_spec,
            system_prompt_append: profile.and_then(|p| p.system_prompt_append),
            extra_env,
            reattach_existing_shim,
            shim_after_seq,
            shim_seq_tracker: Arc::clone(&shim_seq_tracker),
            persisted_active_turn_id,
            persisted_turn_step_seq,
            fallback_handoff_context,
            persisted_running_bg_tasks,
            persisted_bg_wait,
            acp_agent_id: summary.acp_agent_id.clone(),
            acp_spec,
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

        if self.current_generation(&summary.agent_id).await != expected_gen {
            let _ = handle.cmd_tx.send(DriverCommand::Shutdown).await;
            anyhow::bail!("agent session was reset during startup");
        }

        let cmd_tx = handle.cmd_tx.clone();
        let done_notify = Arc::new(tokio::sync::Notify::new());
        let done_notify_bg = Arc::clone(&done_notify);
        let mgr = Arc::clone(self);
        let ag_id = summary.agent_id.clone();
        let mut evt_rx = handle.evt_rx;
        let shim_seq_bg = Arc::clone(&shim_seq_tracker);

        let ingest_task = tokio::spawn(async move {
            const STREAM_COALESCE_WINDOW: std::time::Duration =
                std::time::Duration::from_millis(65);
            let mut stream_buf = StreamingEventBuffer::default();
            let mut last_persisted_shim_seq = shim_seq_bg.load(Ordering::Relaxed);
            let sleep = tokio::time::sleep(STREAM_COALESCE_WINDOW);
            tokio::pin!(sleep);

            loop {
                tokio::select! {
                    biased;
                    maybe_evt = evt_rx.recv() => {
                        match maybe_evt {
                            Some(evt) => {
                                if mgr.current_generation(&ag_id).await != expected_gen {
                                    stream_buf.clear();
                                    continue;
                                }
                                let was_empty = stream_buf.is_empty();
                                match stream_buf.push(evt) {
                                    StreamPushOutcome::Buffered => {
                                        if was_empty {
                                            sleep.as_mut().reset(
                                                tokio::time::Instant::now() + STREAM_COALESCE_WINDOW,
                                            );
                                        }
                                    }
                                    StreamPushOutcome::FlushAndIngest { flushed, current } => {
                                        if !stream_buf.is_empty() {
                                            sleep.as_mut().reset(
                                                tokio::time::Instant::now()
                                                    + STREAM_COALESCE_WINDOW,
                                            );
                                        }
                                        if let Some(prev) = flushed {
                                            mgr.ingest_driver_event(&ag_id, expected_gen, *prev)
                                                .await;
                                        }
                                        mgr.ingest_driver_event(&ag_id, expected_gen, *current)
                                            .await;
                                        let cur_seq = shim_seq_bg.load(Ordering::Relaxed);
                                        if cur_seq > last_persisted_shim_seq {
                                            let _ = mgr.store.set_agent_shim_cursor(&ag_id, cur_seq);
                                            last_persisted_shim_seq = cur_seq;
                                        }
                                    }
                                }
                            }
                            None => {
                                if let Some(pending) = stream_buf.take()
                                    && mgr.current_generation(&ag_id).await == expected_gen
                                {
                                    mgr.ingest_driver_event(&ag_id, expected_gen, pending).await;
                                }
                                let cur_seq = shim_seq_bg.load(Ordering::Relaxed);
                                if cur_seq > last_persisted_shim_seq {
                                    let _ = mgr.store.set_agent_shim_cursor(&ag_id, cur_seq);
                                }
                                break;
                            }
                        }
                    }
                    _ = &mut sleep, if !stream_buf.is_empty() => {
                        if let Some(pending) = stream_buf.take()
                            && mgr.current_generation(&ag_id).await == expected_gen
                        {
                            mgr.ingest_driver_event(&ag_id, expected_gen, pending).await;
                            let cur_seq = shim_seq_bg.load(Ordering::Relaxed);
                            if cur_seq > last_persisted_shim_seq {
                                let _ = mgr.store.set_agent_shim_cursor(&ag_id, cur_seq);
                                last_persisted_shim_seq = cur_seq;
                            }
                        }
                    }
                }
            }
            let mut live_guard = mgr.live_agents.write().await;
            if live_guard
                .get(&ag_id)
                .is_some_and(|e| e.generation == expected_gen)
            {
                live_guard.remove(&ag_id);
            }
            drop(live_guard);
            if mgr.current_generation(&ag_id).await == expected_gen {
                mgr.clear_pending_requests(&ag_id).await;
                mgr.keep_awake.mark_agent_idle(&ag_id);
            }
            done_notify_bg.notify_waiters();
        });

        self.live_agents.write().await.insert(
            summary.agent_id.clone(),
            LiveAgentEntry {
                generation: expected_gen,
                cmd_tx: cmd_tx.clone(),
                done_notify,
                ingest_abort: ingest_task.abort_handle(),
                shim_seq_tracker,
            },
        );

        Ok(cmd_tx)
    }

    async fn ingest_driver_event(
        self: &Arc<Self>,
        agent_id: &str,
        expected_gen: u64,
        evt: DriverEvent,
    ) {
        if self.current_generation(agent_id).await != expected_gen {
            return;
        }
        match evt {
            DriverEvent::SessionInitialized {
                persistence_ref,
                model,
            } => {
                let _ = self.store.set_persistence_ref(agent_id, &persistence_ref);
                if let Some(m) = model
                    && let Ok(Some(mut ag)) = self.store.get_agent(agent_id)
                {
                    ag.model = Some(m);
                    let _ = self.store.upsert_agent(&ag, Some(&persistence_ref));
                }
                return;
            }
            DriverEvent::ProviderSubagentUpsert(desc) => {
                let _ = self.store.upsert_provider_subagent(&desc);
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::ProviderSubagentUpdated(desc));
                return;
            }
            DriverEvent::ProviderSubagentTimeline {
                subagent_id,
                mut payload,
            } => {
                sanitize_timeline_payload(&mut payload);
                if let Ok((saved, mutated)) = self.store.append_provider_subagent_event_upsert(
                    agent_id,
                    &subagent_id,
                    payload,
                ) && mutated
                {
                    let _ = self
                        .broadcast_tx
                        .send(DaemonBroadcast::ProviderSubagentTimelineLive(saved));
                }
                return;
            }
            DriverEvent::ReconcileBackgroundTasks(settlements) => {
                if let Ok(updated) = self
                    .store
                    .settle_timeline_tool_calls_in_place(agent_id, &settlements)
                    && updated > 0
                    && let Ok(Some(ag)) = self.store.get_agent(agent_id)
                {
                    let _ = self.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));
                }
                return;
            }
            DriverEvent::TurnUsage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                last_call_prompt_tokens,
                model,
            } => {
                let recorded_at_ms = Utc::now().timestamp_millis();
                self.turn_usage_recorded
                    .write()
                    .await
                    .insert(agent_id.to_string());
                if let Some(prompt_toks) = last_call_prompt_tokens
                    && prompt_toks > 0
                {
                    self.last_call_prompt_occupancy
                        .write()
                        .await
                        .insert(agent_id.to_string(), (prompt_toks, recorded_at_ms));
                }
                let store_bg = Arc::clone(&self.store);
                let aid_bg = agent_id.to_string();
                tokio::task::spawn_blocking(move || {
                    if let Ok(Some(ag)) = store_bg.get_agent(&aid_bg) {
                        let provider_slug = ag.provider.slug().to_string();
                        let effective_model = model
                            .as_deref()
                            .filter(|m| !m.trim().is_empty())
                            .or(ag.model.as_deref())
                            .filter(|m| !m.trim().is_empty())
                            .unwrap_or(&provider_slug)
                            .to_string();
                        let (estimated_cost_usd, cache_savings_usd) =
                            crate::usage::estimate_turn_cost_usd(
                                &provider_slug,
                                &effective_model,
                                input_tokens,
                                output_tokens,
                                cache_read_tokens,
                                cache_write_tokens,
                            );
                        let entry = orbit_protocol::UsageLedgerEntry {
                            id: format!("ul_{}", Uuid::new_v4().simple()),
                            agent_id: aid_bg,
                            workspace_cwd: ag.cwd,
                            provider: provider_slug,
                            model: effective_model,
                            input_tokens,
                            output_tokens,
                            cache_read_tokens,
                            cache_write_tokens,
                            estimated_cost_usd,
                            cache_savings_usd,
                            recorded_at_ms,
                        };
                        let _ = store_bg.record_usage_entry(&entry);
                    }
                });
                return;
            }
            _ => {}
        }

        let mut should_check_auto_archive = false;
        let mut should_drain_queued_prompt = false;
        let mut should_mark_finished = false;
        let mut completed_running_turn = false;
        let payload = match evt {
            DriverEvent::StatusChanged(status) => {
                self.keep_awake.notify_agent_status(agent_id, &status);
                let prev = self.store.get_agent(agent_id).ok().flatten();
                let was_running_turn = prev
                    .as_ref()
                    .is_some_and(|a| matches!(a.status, AgentStatus::Running { .. }));
                let was_foreground_busy =
                    prev.as_ref().is_some_and(|a| a.status.is_foreground_busy());
                let was_awaiting_permission = prev
                    .as_ref()
                    .is_some_and(|a| matches!(a.status, AgentStatus::AwaitingPermission { .. }));
                let was_background_waiting = prev
                    .as_ref()
                    .is_some_and(|a| a.status.is_background_waiting());
                match &status {
                    AgentStatus::Idle => {
                        self.clear_pending_requests(agent_id).await;
                        let suppressed = self.suppress_auto_drain.write().await.remove(agent_id);
                        if (was_foreground_busy || was_awaiting_permission) && !suppressed {
                            should_drain_queued_prompt = true;
                        }
                        if was_running_turn {
                            completed_running_turn = true;
                        }
                        if was_foreground_busy {
                            let _ = self.store.touch_agent_activity(agent_id, Utc::now());
                            if prev.as_ref().is_some_and(|a| !a.background) {
                                should_mark_finished = true;
                            }
                            should_check_auto_archive = true;
                        } else if was_background_waiting {
                            should_check_auto_archive = true;
                        }
                    }
                    AgentStatus::AutonomousWakeup { .. } if status.is_background_waiting() => {
                        self.clear_pending_requests(agent_id).await;
                        let suppressed = self.suppress_auto_drain.write().await.remove(agent_id);
                        if (was_foreground_busy || was_awaiting_permission) && !suppressed {
                            should_drain_queued_prompt = true;
                        }
                        if was_foreground_busy {
                            let _ = self.store.touch_agent_activity(agent_id, Utc::now());
                            if prev.as_ref().is_some_and(|a| !a.background) {
                                should_mark_finished = true;
                            }
                        }
                    }
                    AgentStatus::AutonomousWakeup { .. } => {
                        self.suppress_auto_drain.write().await.remove(agent_id);
                        let _ = self.store.touch_agent_activity(agent_id, Utc::now());
                    }
                    AgentStatus::Error {
                        message,
                        recoverable,
                    } => {
                        self.suppress_auto_drain.write().await.remove(agent_id);
                        self.turn_usage_recorded.write().await.remove(agent_id);
                        tracing::warn!(
                            agent_id = %agent_id,
                            recoverable,
                            error = %message,
                            "Agent transitioned to Error state"
                        );
                        let _ = self.store.touch_agent_activity(agent_id, Utc::now());
                        if prev.as_ref().is_some_and(|a| !a.background) {
                            let _ = self
                                .store
                                .set_agent_attention(agent_id, AttentionState::Error);
                        }
                        should_check_auto_archive = true;
                    }
                    AgentStatus::AwaitingPermission { .. } => {
                        if prev.as_ref().is_some_and(|a| !a.background) {
                            let _ = self
                                .store
                                .set_agent_attention(agent_id, AttentionState::Permission);
                        }
                    }
                    AgentStatus::Running { .. } => {
                        self.suppress_auto_drain.write().await.remove(agent_id);
                        self.turn_usage_recorded.write().await.remove(agent_id);
                        let _ = self.store.touch_agent_activity(agent_id, Utc::now());
                    }
                    _ => {}
                }
                TimelinePayload::StatusChanged { status }
            }
            DriverEvent::ThinkingText { item_id, text } => {
                TimelinePayload::AssistantThinking { item_id, text }
            }
            DriverEvent::AssistantText {
                item_id,
                text,
                streaming,
            } => {
                if !streaming {
                    let mgr = Arc::clone(self);
                    let aid = agent_id.to_string();
                    let text_clone = text.clone();
                    tokio::task::spawn_blocking(move || {
                        let _ = mgr.auto_extract_artifacts_from_message(&aid, &text_clone);
                    });
                }
                TimelinePayload::AssistantMessage {
                    item_id,
                    text,
                    streaming,
                }
            }
            DriverEvent::ToolCall {
                call_id,
                status,
                detail,
                background_task,
            } => {
                if status == ToolCallStatus::Completed {
                    match &detail {
                        ToolCallDetail::FileEdit { path, .. } => {
                            self.record_file_edit_and_check_conflict(agent_id, path)
                                .await;
                            if let Ok(Some(ag)) = self.store.get_agent(agent_id) {
                                let hooks = Arc::clone(&self.hooks);
                                let cwd_hook = PathBuf::from(&ag.cwd);
                                let target_path = path.clone();
                                let payload = serde_json::json!({
                                    "event": "post_file_edit",
                                    "agent_id": agent_id,
                                    "workspace_id": &ag.workspace_id,
                                    "cwd": &ag.cwd,
                                    "path": &target_path,
                                    "source": "agent_tool",
                                });
                                tokio::spawn(async move {
                                    let _ = hooks
                                        .dispatch_event(
                                            orbit_protocol::HookEventKind::PostFileEdit,
                                            Some(&cwd_hook),
                                            Some(&target_path),
                                            payload,
                                        )
                                        .await;
                                });
                            }
                        }
                        ToolCallDetail::Generic {
                            tool_name,
                            arguments_json,
                        } if !crate::artifacts::is_orbit_artifact_mcp_tool(tool_name)
                            && arguments_json.contains("ui://") =>
                        {
                            let mgr = Arc::clone(self);
                            let aid = agent_id.to_string();
                            let tname = tool_name.clone();
                            let payload_json = arguments_json.clone();
                            tokio::task::spawn_blocking(move || {
                                let _ = mgr.auto_extract_artifacts_from_tool_call(
                                    &aid,
                                    &tname,
                                    &payload_json,
                                );
                            });
                        }
                        ToolCallDetail::Shell {
                            output_preview: Some(out),
                            ..
                        } if out.contains("ui://") => {
                            let mgr = Arc::clone(self);
                            let aid = agent_id.to_string();
                            let out_clone = out.clone();
                            tokio::task::spawn_blocking(move || {
                                let _ = mgr.auto_extract_artifacts_from_tool_call(
                                    &aid, "shell", &out_clone,
                                );
                            });
                        }
                        _ => {}
                    }
                }
                TimelinePayload::ToolCall {
                    call_id,
                    status,
                    detail,
                    background_task,
                }
            }
            DriverEvent::PermissionRequest {
                request_id,
                title,
                detail,
                allowed_decisions,
            } => {
                if let Ok(Some(ag)) = self.store.get_agent(agent_id) {
                    self.pending_permissions.write().await.insert(
                        (agent_id.to_string(), request_id.clone()),
                        PendingPermissionSummary {
                            agent_id: agent_id.to_string(),
                            agent_title: ag.title,
                            workspace_id: ag.workspace_id,
                            request_id: request_id.clone(),
                            title: title.clone(),
                            detail: detail.clone(),
                            allowed_decisions: allowed_decisions.clone(),
                            created_at: Utc::now(),
                        },
                    );
                }
                TimelinePayload::PermissionRequest {
                    request_id,
                    title,
                    detail,
                    allowed_decisions,
                }
            }
            DriverEvent::QuestionRequest {
                request_id,
                questions,
            } => {
                Self::record_pending_request(&self.pending_questions, agent_id, &request_id).await;
                TimelinePayload::QuestionRequest {
                    request_id,
                    questions: questions as Vec<QuestionItem>,
                }
            }
            DriverEvent::Notice { level, message } => {
                if level == "error" || level == "warning" {
                    tracing::warn!(
                        agent_id = %agent_id,
                        level = %level,
                        notice = %message,
                        "Agent emitted runtime notice"
                    );
                }
                TimelinePayload::Notice { level, message }
            }
            DriverEvent::Exited { code } => {
                if code != Some(0) {
                    tracing::warn!(
                        agent_id = %agent_id,
                        exit_code = ?code,
                        "Agent driver process exited"
                    );
                }
                self.clear_pending_requests(agent_id).await;
                self.turn_usage_recorded.write().await.remove(agent_id);
                self.keep_awake
                    .notify_agent_status(agent_id, &AgentStatus::Closed);
                if let Ok(Some(ag)) = self.store.get_agent(agent_id) {
                    let hooks = Arc::clone(&self.hooks);
                    let cwd_hook = PathBuf::from(&ag.cwd);
                    let prov_slug = ag.provider.slug().to_string();
                    let payload = serde_json::json!({
                        "event": "session_end",
                        "agent_id": agent_id,
                        "workspace_id": &ag.workspace_id,
                        "cwd": &ag.cwd,
                        "provider": &prov_slug,
                        "reason": "exited",
                        "exit_code": code,
                    });
                    tokio::spawn(async move {
                        let _ = hooks
                            .dispatch_event(
                                orbit_protocol::HookEventKind::SessionEnd,
                                Some(&cwd_hook),
                                Some(&prov_slug),
                                payload,
                            )
                            .await;
                    });
                }
                should_check_auto_archive = true;
                TimelinePayload::StatusChanged {
                    status: AgentStatus::Closed,
                }
            }
            DriverEvent::SessionInitialized { .. }
            | DriverEvent::ProviderSubagentUpsert(_)
            | DriverEvent::ProviderSubagentTimeline { .. }
            | DriverEvent::ReconcileBackgroundTasks(_)
            | DriverEvent::TurnUsage { .. } => unreachable!(),
        };

        let is_status_change = matches!(payload, TimelinePayload::StatusChanged { .. });
        let mut payload = payload;
        sanitize_timeline_payload(&mut payload);
        if let Ok((saved_ev, mutated)) = self
            .store
            .append_timeline_event_deduped(agent_id, 1, None, payload)
            && mutated
        {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::TimelineLive(saved_ev));
        }
        if is_status_change && let Ok(Some(mut ag)) = self.store.get_agent(agent_id) {
            let mut drained_prompt = None;
            if should_drain_queued_prompt
                && ag.archived_at.is_none()
                && !self.has_pending_permissions(agent_id).await
                && let Ok(Some(next_item)) = self.store.pop_next_queued_prompt(agent_id)
            {
                if let Ok(Some(refreshed)) = self.store.get_agent(agent_id) {
                    ag = refreshed;
                }
                drained_prompt = Some(next_item);
            }

            // Only emit `AttentionState::Finished` when no queued follow-up is about to run immediately.
            if should_mark_finished && drained_prompt.is_none() {
                let _ = self
                    .store
                    .set_agent_attention(agent_id, AttentionState::Finished);
                ag.attention = AttentionState::Finished;
            }

            let auto_arch = ag.auto_archive && ag.archived_at.is_none() && drained_prompt.is_none();
            let ws_id = ag.workspace_id.clone();
            let finished_idle = should_check_auto_archive && matches!(ag.status, AgentStatus::Idle);
            let ag_handle = ag.handle.clone();
            let ag_bg = ag.background;
            let ag_is_subagent = ag.parent_agent_id.is_some();
            let ag_needs_auto_naming = completed_running_turn
                && !ag_is_subagent
                && !crate::workspace_organizer::is_auto_named(&ag.labels);
            if completed_running_turn {
                let already_recorded = self.turn_usage_recorded.write().await.remove(agent_id);
                if !already_recorded {
                    let store_bg = Arc::clone(&self.store);
                    let aid_bg = agent_id.to_string();
                    let cwd_bg = ag.cwd.clone();
                    let provider_slug = ag.provider.slug().to_string();
                    let effective_model = ag
                        .model
                        .as_deref()
                        .filter(|m| !m.trim().is_empty())
                        .unwrap_or(&provider_slug)
                        .to_string();
                    tokio::task::spawn_blocking(move || {
                        if let Ok(events) = store_bg.fetch_recent_timeline(&aid_bg, 400) {
                            let (input_tokens, output_tokens) =
                                crate::usage::estimate_fallback_turn_tokens(&events);
                            if input_tokens > 0 || output_tokens > 0 {
                                let (estimated_cost_usd, cache_savings_usd) =
                                    crate::usage::estimate_turn_cost_usd(
                                        &provider_slug,
                                        &effective_model,
                                        input_tokens,
                                        output_tokens,
                                        0,
                                        0,
                                    );
                                let entry = orbit_protocol::UsageLedgerEntry {
                                    id: format!("ul_{}", Uuid::new_v4().simple()),
                                    agent_id: aid_bg,
                                    workspace_cwd: cwd_bg,
                                    provider: provider_slug,
                                    model: effective_model,
                                    input_tokens,
                                    output_tokens,
                                    cache_read_tokens: 0,
                                    cache_write_tokens: 0,
                                    estimated_cost_usd,
                                    cache_savings_usd,
                                    recorded_at_ms: Utc::now().timestamp_millis(),
                                };
                                let _ = store_bg.record_usage_entry(&entry);
                            }
                        }
                    });
                }
                let hooks = Arc::clone(&self.hooks);
                let cwd_hook = PathBuf::from(&ag.cwd);
                let prov_slug = ag.provider.slug().to_string();
                let payload = serde_json::json!({
                    "event": "turn_stop",
                    "agent_id": agent_id,
                    "workspace_id": &ag.workspace_id,
                    "cwd": &ag.cwd,
                    "provider": &prov_slug,
                    "title": &ag.title,
                });
                tokio::spawn(async move {
                    let _ = hooks
                        .dispatch_event(
                            orbit_protocol::HookEventKind::TurnStop,
                            Some(&cwd_hook),
                            Some(&prov_slug),
                            payload,
                        )
                        .await;
                });
            }
            let _ = self.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));

            if finished_idle && !ag_bg {
                let ws_opt = self.store.get_workspace(&ws_id).ok().flatten();
                let is_curated_ws = ws_opt.as_ref().is_some_and(|w| w.is_curated);
                let peer_count = self
                    .store
                    .list_agents_filtered(Some(&ws_id), false, false, None)
                    .map(|p| {
                        p.into_iter()
                            .filter(|a| a.parent_agent_id.is_none())
                            .count()
                    })
                    .unwrap_or(0);
                if is_curated_ws && !ag_is_subagent && peer_count >= 2 {
                    let who = ag_handle.as_deref().unwrap_or("peer");
                    let snippet = self
                        .store
                        .last_assistant_message_text(agent_id)
                        .ok()
                        .flatten()
                        .map(|s| {
                            let one_line: String =
                                s.split_whitespace().collect::<Vec<_>>().join(" ");
                            one_line.chars().take(110).collect::<String>()
                        })
                        .unwrap_or_else(|| "finished turn".into());
                    if let Ok(ev) = self.store.append_workspace_event(
                        &ws_id,
                        Some(agent_id),
                        Some(who),
                        WorkspaceEventKind::PeerFinished,
                        &format!("@{who} finished: {snippet}"),
                        None,
                    ) {
                        let _ = self
                            .broadcast_tx
                            .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
                    }
                }

                // Bounded non-blocking Turn-1 AI auto-naming & semantic summary
                // (respects `orbit.title_locked = "1"` and `orbit.auto_named = "1"`).
                if ag_needs_auto_naming && drained_prompt.is_none() && !auto_arch {
                    let mgr_bg = Arc::clone(self);
                    let aid_bg = agent_id.to_string();
                    tokio::spawn(async move {
                        let _ = crate::workspace_organizer::auto_name_agent_after_first_turn(
                            &mgr_bg, &aid_bg,
                        )
                        .await;
                    });
                }
            }

            if let Some(next_item) = drained_prompt {
                let mgr = Arc::clone(self);
                let aid = agent_id.to_string();
                tokio::spawn(async move {
                    let _ = mgr.store.set_agent_attention(&aid, AttentionState::None);
                    let _ = mgr.store.touch_agent_activity(&aid, Utc::now());
                    if let Err(e) = mgr
                        .dispatch_user_prompt_with_handoff(
                            &aid,
                            next_item.wire_prompt,
                            Some(next_item.display_prompt),
                            next_item.badges,
                            None,
                            ActiveTurnBehavior::Steer,
                            None,
                        )
                        .await
                    {
                        tracing::warn!(
                            agent_id = %aid,
                            error = %e,
                            "Failed auto-draining daemon queued prompt"
                        );
                    }
                });
            } else if should_check_auto_archive && auto_arch {
                let mgr = Arc::clone(self);
                let aid = agent_id.to_string();
                tokio::spawn(async move {
                    // Brief delay so finish subscriptions capture the settled snapshot before archive
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    let _ = mgr.archive_agent(&aid).await;
                    if let Ok(Some(ws)) = mgr.store.get_workspace(&ws_id)
                        && ws.is_worktree
                        && mgr
                            .store
                            .list_agents_filtered(Some(&ws_id), false, true, None)
                            .map(|a| a.is_empty())
                            .unwrap_or(false)
                    {
                        let _ = mgr.archive_workspace(&ws_id).await;
                    }
                });
            }
        }
    }

    // --- Detached PTY Terminals ---

    /// Return the latest provider-reported single-call prompt token count `(tokens, recorded_at_ms)`
    /// observed in-memory for `agent_id`, if available.
    pub async fn latest_single_call_prompt_tokens(&self, agent_id: &str) -> Option<(u64, i64)> {
        self.last_call_prompt_occupancy
            .read()
            .await
            .get(agent_id)
            .copied()
    }

    pub async fn list_live_terminals(&self) -> Vec<TerminalSummary> {
        let mut live: Vec<TerminalSummary> = self
            .live_terminals
            .read()
            .await
            .values()
            .map(|t| t.summary.clone())
            .collect();
        live.sort_by_key(|b| std::cmp::Reverse(b.created_at));
        live
    }

    pub async fn create_terminal(
        self: &Arc<Self>,
        cwd: String,
        title: Option<String>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<TerminalSummary> {
        self.create_terminal_with_command(cwd, title, cols, rows, None, HashMap::new())
            .await
    }

    pub async fn create_terminal_with_command(
        self: &Arc<Self>,
        cwd: String,
        title: Option<String>,
        cols: u16,
        rows: u16,
        initial_command: Option<String>,
        env: HashMap<String, String>,
    ) -> anyhow::Result<TerminalSummary> {
        let term_id = format!("term_{}", Uuid::new_v4().simple());
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let sock_path = self.shim_socket_path("p", &term_id);
        let candidate_cwd = crate::expand_host_path(&cwd);
        let resolved_cwd = if candidate_cwd.is_dir() {
            candidate_cwd
        } else {
            crate::expand_host_path("~")
        };
        let resolved_cwd_str = resolved_cwd.to_string_lossy().to_string();

        let mut env = env;
        let (program, args) = if let Some(cmd_line) = initial_command {
            let shell_flag = if orbit_providers::merge_cached_shell_env_into_map(&mut env) {
                "-c"
            } else {
                "-lc"
            };
            (
                "/bin/sh".to_string(),
                vec![shell_flag.to_string(), cmd_line],
            )
        } else {
            (shell.clone(), Vec::new())
        };

        let spec = ShimSpawnSpec {
            shim_id: format!("pty_{term_id}"),
            mode: ShimMode::Pty { cols, rows },
            program,
            args,
            cwd: resolved_cwd,
            env,
        };

        let shim = spawn_detached_shim(&self.orbit_exe, &sock_path, &spec).await?;
        let summary = TerminalSummary {
            terminal_id: term_id,
            cwd: resolved_cwd_str,
            title: title.unwrap_or_else(|| {
                Path::new(&shell)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("shell")
                    .to_string()
            }),
            cols,
            rows,
            created_at: Utc::now(),
        };
        self.store.upsert_terminal(&summary)?;
        self.bind_terminal_shim(summary.clone(), shim).await;
        Ok(summary)
    }

    async fn bind_terminal_shim(self: &Arc<Self>, summary: TerminalSummary, mut shim: ShimClient) {
        let (shim_tx, mut shim_rx) = mpsc::channel::<PtyAction>(256);
        let (out_tx, _) = broadcast::channel::<PtyBroadcast>(512);
        let last_snapshot = Arc::new(RwLock::new(std::collections::VecDeque::with_capacity(
            64 * 1024,
        )));

        let live = Arc::new(LiveTerminal {
            summary: summary.clone(),
            shim_tx,
            out_tx: out_tx.clone(),
            last_snapshot: last_snapshot.clone(),
        });
        self.live_terminals
            .write()
            .await
            .insert(summary.terminal_id.clone(), live);

        let mgr = Arc::clone(self);
        let tid = summary.terminal_id;
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    maybe_act = shim_rx.recv() => {
                        let Some(act) = maybe_act else { break; };
                        match act {
                            PtyAction::Input(bytes) => {
                                let _ = shim.write_pty_input(&bytes).await;
                            }
                            PtyAction::Resize { cols, rows } => {
                                let _ = shim.resize_pty(cols, rows).await;
                            }
                            PtyAction::Kill => {
                                let _ = shim.kill().await;
                                break;
                            }
                        }
                    }
                    maybe_frame = shim.rx.recv() => {
                        let Some(frame) = maybe_frame else { break; };
                        match frame {
                            ShimFrame::PtySnapshot { data_b64 } => {
                                if let Ok(bytes) = B64.decode(data_b64) {
                                    {
                                        let mut sb = last_snapshot.write().await;
                                        sb.clear();
                                        let start = bytes.len().saturating_sub(64 * 1024);
                                        sb.extend(bytes[start..].iter().copied());
                                    }
                                    let _ = out_tx.send(PtyBroadcast::Snapshot(bytes));
                                }
                            }
                            ShimFrame::PtyOutput { data_b64 } => {
                                if let Ok(bytes) = B64.decode(data_b64) {
                                    {
                                        let mut sb = last_snapshot.write().await;
                                        if bytes.len() >= 64 * 1024 {
                                            sb.clear();
                                            let start = bytes.len() - 64 * 1024;
                                            sb.extend(bytes[start..].iter().copied());
                                        } else {
                                            let total = sb.len() + bytes.len();
                                            if total > 64 * 1024 {
                                                let excess = total - 64 * 1024;
                                                for _ in 0..excess {
                                                    sb.pop_front();
                                                }
                                            }
                                            sb.extend(bytes.iter().copied());
                                        }
                                    }
                                    let _ = out_tx.send(PtyBroadcast::Output(bytes));
                                }
                            }
                            ShimFrame::Exited { .. } => break,
                            _ => {}
                        }
                    }
                }
            }
            let _ = out_tx.send(PtyBroadcast::Closed);
            mgr.live_terminals.write().await.remove(&tid);
            let _ = mgr.store.delete_terminal(&tid);
        });
    }

    pub async fn attach_terminal(
        &self,
        terminal_id: &str,
    ) -> Option<(TerminalSummary, Vec<u8>, broadcast::Receiver<PtyBroadcast>)> {
        let live = self.live_terminals.read().await.get(terminal_id).cloned()?;
        let snap: Vec<u8> = live.last_snapshot.read().await.iter().copied().collect();
        Some((live.summary.clone(), snap, live.out_tx.subscribe()))
    }

    pub async fn terminal_snapshot(&self, terminal_id: &str) -> Option<Vec<u8>> {
        let live = self.live_terminals.read().await.get(terminal_id).cloned()?;
        let snap: Vec<u8> = live.last_snapshot.read().await.iter().copied().collect();
        Some(snap)
    }

    pub async fn write_terminal_input(&self, terminal_id: &str, data: Vec<u8>) {
        if let Some(live) = self.live_terminals.read().await.get(terminal_id).cloned() {
            let _ = live.shim_tx.send(PtyAction::Input(data)).await;
        }
    }

    /// Capture clean plain-text terminal screen + scrollback using `vt100` (strips raw ANSI escapes).
    pub async fn capture_terminal_text(
        &self,
        terminal_id: &str,
        max_lines: Option<u32>,
    ) -> anyhow::Result<String> {
        let live = self
            .live_terminals
            .read()
            .await
            .get(terminal_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("terminal `{terminal_id}` not found"))?;
        let raw: Vec<u8> = live.last_snapshot.read().await.iter().copied().collect();
        let rows = live.summary.rows.max(24);
        let cols = live.summary.cols.max(80);
        let nz_rows = std::num::NonZeroU16::new(rows.max(1)).unwrap_or(std::num::NonZeroU16::MIN);
        let nz_cols = std::num::NonZeroU16::new(cols.max(1)).unwrap_or(std::num::NonZeroU16::MIN);
        let mut parser = vt100::Parser::new(nz_rows, nz_cols, 2000);
        parser.process(&raw);
        let screen = parser.screen();
        let contents = screen.contents();
        let lines: Vec<&str> = contents.lines().collect();
        let limit = max_lines.unwrap_or(200) as usize;
        let slice = if lines.len() > limit {
            &lines[lines.len() - limit..]
        } else {
            &lines[..]
        };
        Ok(slice.join("\n"))
    }

    /// Send raw text and/or symbolic key tokens (`Enter`, `Ctrl+C`, `Tab`, `Esc`, etc.) to a PTY.
    pub async fn send_terminal_keys(
        &self,
        terminal_id: &str,
        text: Option<&str>,
        keys: &[String],
    ) -> anyhow::Result<()> {
        let live = self
            .live_terminals
            .read()
            .await
            .get(terminal_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("terminal `{terminal_id}` not found"))?;
        let mut bytes = Vec::new();
        if let Some(t) = text {
            bytes.extend_from_slice(t.as_bytes());
        }
        for key in keys {
            bytes.extend_from_slice(decode_terminal_key_token(key));
        }
        if !bytes.is_empty() {
            live.shim_tx
                .send(PtyAction::Input(bytes))
                .await
                .map_err(|_| anyhow::anyhow!("terminal `{terminal_id}` closed"))?;
        }
        Ok(())
    }

    pub async fn resize_terminal(&self, terminal_id: &str, cols: u16, rows: u16) {
        if let Some(live) = self.live_terminals.read().await.get(terminal_id).cloned() {
            let _ = live.shim_tx.send(PtyAction::Resize { cols, rows }).await;
        }
    }

    pub async fn kill_terminal(&self, terminal_id: &str) -> anyhow::Result<()> {
        if let Some(live) = self.live_terminals.write().await.remove(terminal_id) {
            let _ = live.shim_tx.send(PtyAction::Kill).await;
        }
        self.store.delete_terminal(terminal_id)?;
        Ok(())
    }
}

fn decode_terminal_key_token(token: &str) -> &'static [u8] {
    match token.trim().to_ascii_lowercase().as_str() {
        "enter" | "return" | "cr" => b"\r",
        "tab" => b"\t",
        "esc" | "escape" => b"\x1b",
        "backspace" | "bs" => b"\x7f",
        "space" => b" ",
        "ctrl+c" | "c-c" => b"\x03",
        "ctrl+d" | "c-d" => b"\x04",
        "ctrl+z" | "c-z" => b"\x1a",
        "ctrl+l" | "c-l" => b"\x0c",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        _ => b"",
    }
}

fn resolve_orbit_exe() -> PathBuf {
    if let Ok(explicit) = std::env::var("ORBIT_BIN")
        && !explicit.trim().is_empty()
    {
        return PathBuf::from(explicit);
    }
    if let Ok(cur) = std::env::current_exe() {
        let is_orbit_bin = cur
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s == "orbit");
        if is_orbit_bin {
            return cur;
        }
        let mut anc = cur.parent();
        while let Some(dir) = anc {
            let candidate = dir.join("orbit");
            if candidate.is_file() {
                return candidate;
            }
            anc = dir.parent();
        }
        return cur;
    }
    PathBuf::from("orbit")
}

fn derive_human_seed(
    display_prompt: Option<&str>,
    badges: &[PromptBadge],
    fallback_wire: &str,
) -> String {
    if let Some(dp) = display_prompt {
        let trimmed = dp.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(slash) = badges.iter().find(|b| b.kind == "slash") {
        return match slash.label.as_str() {
            "/review" => "Code review".to_string(),
            "/plan" => "Implementation plan".to_string(),
            "/compact" => "Compact context".to_string(),
            other => other.to_string(),
        };
    }
    if let Some(first) = badges.first() {
        return first.label.clone();
    }
    fallback_wire.to_string()
}

fn summarize_prompt_title(prompt: &str) -> String {
    let first_line = prompt
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("New Session");
    let trimmed = first_line.trim();
    if trimmed.chars().count() <= 48 {
        trimmed.to_string()
    } else {
        let mut s: String = trimmed.chars().take(48).collect();
        s.push('…');
        s
    }
}

fn auto_worktree_branch(prompt: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in prompt.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !slug.is_empty() && !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
        if slug.len() >= 20 {
            break;
        }
    }
    let clean = slug.trim_matches('-');
    let short_id = &Uuid::new_v4().simple().to_string()[..4];
    if clean.is_empty() {
        format!("orbit/task-{short_id}")
    } else {
        format!("orbit/{clean}-{short_id}")
    }
}

enum StreamPushOutcome {
    Buffered,
    FlushAndIngest {
        flushed: Option<Box<DriverEvent>>,
        current: Box<DriverEvent>,
    },
}

#[derive(Default)]
struct StreamingEventBuffer {
    pending: Option<DriverEvent>,
}

impl StreamingEventBuffer {
    fn is_empty(&self) -> bool {
        self.pending.is_none()
    }

    fn clear(&mut self) {
        self.pending = None;
    }

    fn take(&mut self) -> Option<DriverEvent> {
        self.pending.take()
    }

    fn push(&mut self, evt: DriverEvent) -> StreamPushOutcome {
        match evt {
            DriverEvent::AssistantText {
                ref item_id,
                streaming: true,
                ..
            } => {
                let same_pending = matches!(
                    self.pending.as_ref(),
                    Some(DriverEvent::AssistantText {
                        item_id: prev_id,
                        streaming: true,
                        ..
                    }) if prev_id == item_id
                );
                if same_pending || self.pending.is_none() {
                    self.pending = Some(evt);
                    StreamPushOutcome::Buffered
                } else {
                    let flushed = self.pending.replace(evt);
                    StreamPushOutcome::FlushAndIngest {
                        flushed: None,
                        current: Box::new(flushed.unwrap_or_else(|| unreachable!())),
                    }
                }
            }
            DriverEvent::ThinkingText { ref item_id, .. } => {
                let same_pending = matches!(
                    self.pending.as_ref(),
                    Some(DriverEvent::ThinkingText {
                        item_id: prev_id,
                        ..
                    }) if prev_id == item_id
                );
                if same_pending || self.pending.is_none() {
                    self.pending = Some(evt);
                    StreamPushOutcome::Buffered
                } else {
                    let flushed = self.pending.replace(evt);
                    StreamPushOutcome::FlushAndIngest {
                        flushed: None,
                        current: Box::new(flushed.unwrap_or_else(|| unreachable!())),
                    }
                }
            }
            DriverEvent::AssistantText {
                ref item_id,
                streaming: false,
                ..
            } => {
                let supersedes_pending = matches!(
                    self.pending.as_ref(),
                    Some(DriverEvent::AssistantText {
                        item_id: prev_id,
                        streaming: true,
                        ..
                    }) if prev_id == item_id
                );
                let flushed = if supersedes_pending {
                    self.pending = None;
                    None
                } else {
                    self.pending.take().map(Box::new)
                };
                StreamPushOutcome::FlushAndIngest {
                    flushed,
                    current: Box::new(evt),
                }
            }
            other => {
                let flushed = self.pending.take().map(Box::new);
                StreamPushOutcome::FlushAndIngest {
                    flushed,
                    current: Box::new(other),
                }
            }
        }
    }
}

fn sanitize_timeline_payload(payload: &mut TimelinePayload) {
    if let TimelinePayload::ToolCall {
        detail:
            ToolCallDetail::FileEdit {
                unified_diff: Some(diff),
                ..
            },
        ..
    } = payload
    {
        *diff =
            orbit_providers::compact_unified_diff(diff, orbit_providers::MAX_UNIFIED_DIFF_BYTES);
    }
}

#[cfg(test)]
mod streaming_buffer_tests {
    use super::*;

    #[test]
    fn coalesces_same_item_streaming_assistant_and_drops_superseded_on_final() {
        let mut buf = StreamingEventBuffer::default();
        assert!(matches!(
            buf.push(DriverEvent::AssistantText {
                item_id: "m1".into(),
                text: "Hel".into(),
                streaming: true,
            }),
            StreamPushOutcome::Buffered
        ));
        assert!(matches!(
            buf.push(DriverEvent::AssistantText {
                item_id: "m1".into(),
                text: "Hello world".into(),
                streaming: true,
            }),
            StreamPushOutcome::Buffered
        ));
        match buf.push(DriverEvent::AssistantText {
            item_id: "m1".into(),
            text: "Hello world!".into(),
            streaming: false,
        }) {
            StreamPushOutcome::FlushAndIngest { flushed, current } => {
                assert!(flushed.is_none(), "final message supersedes buffered chunk");
                assert!(matches!(
                    *current,
                    DriverEvent::AssistantText {
                        ref item_id,
                        ref text,
                        streaming: false
                    } if item_id == "m1" && text == "Hello world!"
                ));
            }
            StreamPushOutcome::Buffered => panic!("final text must flush immediately"),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn flushes_pending_streaming_chunk_before_tool_call_or_different_item() {
        let mut buf = StreamingEventBuffer::default();
        assert!(matches!(
            buf.push(DriverEvent::ThinkingText {
                item_id: "t1".into(),
                text: "step 1".into(),
            }),
            StreamPushOutcome::Buffered
        ));
        assert!(matches!(
            buf.push(DriverEvent::ThinkingText {
                item_id: "t1".into(),
                text: "step 1 + 2".into(),
            }),
            StreamPushOutcome::Buffered
        ));
        match buf.push(DriverEvent::AssistantText {
            item_id: "m1".into(),
            text: "ans".into(),
            streaming: true,
        }) {
            StreamPushOutcome::FlushAndIngest { flushed, current } => {
                assert!(flushed.is_none());
                assert!(matches!(
                    *current,
                    DriverEvent::ThinkingText { ref item_id, ref text }
                        if item_id == "t1" && text == "step 1 + 2"
                ));
            }
            StreamPushOutcome::Buffered => panic!("switching item kind must flush previous"),
        }
        assert!(!buf.is_empty());
        match buf.push(DriverEvent::Notice {
            level: "info".into(),
            message: "done".into(),
        }) {
            StreamPushOutcome::FlushAndIngest { flushed, current } => {
                assert!(matches!(
                    flushed.as_deref(),
                    Some(DriverEvent::AssistantText {
                        item_id,
                        text,
                        streaming: true,
                    }) if item_id == "m1" && text == "ans"
                ));
                assert!(matches!(*current, DriverEvent::Notice { .. }));
            }
            StreamPushOutcome::Buffered => panic!("non-streaming event must flush immediately"),
        }
        assert!(buf.is_empty());
    }

    async fn setup_test_orchestrator(home_dir: &Path) -> (Arc<AgentOrchestrator>, Arc<OrbitStore>) {
        std::fs::create_dir_all(home_dir).expect("mkdir home");
        let store = Arc::new(OrbitStore::open_in_memory().expect("open mem store"));
        let scripts = Arc::new(WorkspaceScriptRegistry::default());
        let keep_awake = Arc::new(KeepAwakeManager::new(false));
        let hooks = crate::hooks::HooksEngine::new(home_dir.to_path_buf());
        let orch = AgentOrchestrator::new(
            home_dir.to_path_buf(),
            "host_test".into(),
            "tok_test".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            scripts,
            keep_awake,
            Arc::new(RwLock::new(None)),
            hooks,
        )
        .await;
        (orch, store)
    }

    fn seed_source_conversation(
        store: &OrbitStore,
        ws_id: &str,
        cwd: &str,
        agent_id: &str,
        persistence_ref: Option<&str>,
    ) -> (u64, u64, u64, u64) {
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: ws_id.into(),
                title: "demo".into(),
                cwd: cwd.into(),
                git_branch: Some("main".into()),
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: Some(cwd.into()),
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: agent_id.into(),
                    workspace_id: ws_id.into(),
                    cwd: cwd.into(),
                    title: "Refactor Auth".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: Some("sonnet".into()),
                    effort: Some(ReasoningEffort::High),
                    fast_mode: false,
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: Some("prof_1".into()),
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: Some(now),
                    queued_prompts: Vec::new(),
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: Some("lead".into()),
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                persistence_ref,
            )
            .expect("upsert agent");

        let s1 = store
            .append_timeline_event(
                agent_id,
                1,
                Some("t1"),
                TimelinePayload::UserPrompt {
                    message_id: "u1".into(),
                    text: "Design token rotation for AuthService".into(),
                    badges: Vec::new(),
                },
            )
            .expect("e1")
            .seq;
        let s2 = store
            .append_timeline_event(
                agent_id,
                1,
                Some("t1"),
                TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "Use rotating refresh tokens stored hashed in SQLite.".into(),
                    streaming: false,
                },
            )
            .expect("e2")
            .seq;
        let s3 = store
            .append_timeline_event(
                agent_id,
                1,
                Some("t2"),
                TimelinePayload::UserPrompt {
                    message_id: "u2".into(),
                    text: "POST_FORK_SECRET_PROMPT_XYZ switch to OAuth2 PKCE".into(),
                    badges: Vec::new(),
                },
            )
            .expect("e3")
            .seq;
        let s4 = store
            .append_timeline_event(
                agent_id,
                1,
                Some("t2"),
                TimelinePayload::AssistantMessage {
                    item_id: "a2".into(),
                    text: "POST_FORK_SECRET_REPLY_XYZ PKCE flow implemented.".into(),
                    streaming: false,
                },
            )
            .expect("e4")
            .seq;
        (s1, s2, s3, s4)
    }

    #[tokio::test]
    async fn fork_agent_same_dir_mid_thread_and_new_worktree_e2e() {
        let tmp_root =
            std::env::temp_dir().join(format!("orbit_fork_e2e_{}", Uuid::new_v4().simple()));
        let home_dir = tmp_root.join("orbit_home");
        let repo_dir = tmp_root.join("repo");
        let non_git_dir = tmp_root.join("plain_dir");
        std::fs::create_dir_all(&repo_dir).expect("mkdir repo");
        std::fs::create_dir_all(&non_git_dir).expect("mkdir plain");

        // Initialize a real git repository for the NewWorktree fork test
        let run_git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo_dir)
                .output()
                .expect("run git");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run_git(&["init", "-b", "main"]);
        run_git(&["config", "user.name", "Orbit Test"]);
        run_git(&["config", "user.email", "test@orbit.local"]);
        std::fs::write(repo_dir.join("README.md"), "# Orbit Fork Test\n").expect("write readme");
        run_git(&["add", "README.md"]);
        run_git(&["commit", "-m", "initial commit"]);

        let (orch, store) = setup_test_orchestrator(&home_dir).await;
        let repo_str = repo_dir.to_string_lossy().to_string();
        let (_s1, s2, _s3, s4) =
            seed_source_conversation(&store, "ws_repo", &repo_str, "ag_src", None);

        // (a) Same-directory full fork from standalone non-curated workspace creates a dedicated
        // isolated local workspace so archiving one thread never archives the other.
        let full_out = orch
            .fork_agent(ForkAgentSpec {
                source_agent_id: "ag_src".into(),
                up_to_seq: None,
                target: ForkTarget::SameDirectory,
                title: None,
                initial_prompt: None,
            })
            .await
            .expect("same-dir full fork");
        let full_ws = full_out
            .workspace
            .as_ref()
            .expect("standalone same-dir fork must provision dedicated local workspace");
        assert!(!full_ws.is_worktree);
        assert!(!full_ws.is_curated);
        assert_ne!(full_out.agent.workspace_id, "ws_repo");
        assert_eq!(full_out.agent.workspace_id, full_ws.workspace_id);
        assert_eq!(full_out.agent.cwd, repo_str);
        assert_eq!(full_out.agent.title, "Refactor Auth (fork)");
        assert!(
            !full_out.agent.labels.contains_key("orbit.title_locked"),
            "default derived fork title must remain unlocked"
        );
        assert_eq!(
            full_out.agent.forked_from_agent_id.as_deref(),
            Some("ag_src")
        );
        assert_eq!(full_out.agent.forked_at_seq, Some(s4));
        assert_eq!(full_out.agent.provider, ProviderKind::ClaudeCode);
        assert_eq!(full_out.agent.model.as_deref(), Some("sonnet"));
        assert_eq!(full_out.agent.effort, Some(ReasoningEffort::High));
        assert_eq!(full_out.agent.profile_id.as_deref(), Some("prof_1"));

        let (full_events, _) = store
            .fetch_timeline_since(&full_out.agent.agent_id, 0, 50)
            .expect("fetch full");
        assert_eq!(full_events.len(), 4);

        // (a2) Same-directory fork inside a curated Mission Workspace reuses the workspace and
        // emits a WorkspaceEventKind::PeerJoined event.
        let mut curated_ws = store
            .get_workspace("ws_repo")
            .expect("get ws")
            .expect("exists");
        curated_ws.workspace_id = "ws_curated".into();
        curated_ws.is_curated = true;
        store.upsert_workspace(&curated_ws).expect("upsert curated");
        let mut curated_ag = store.get_agent("ag_src").expect("get ag").expect("exists");
        curated_ag.agent_id = "ag_curated_src".into();
        curated_ag.workspace_id = "ws_curated".into();
        store
            .upsert_agent(&curated_ag, None)
            .expect("upsert curated ag");
        let curated_fork = orch
            .fork_agent(ForkAgentSpec {
                source_agent_id: "ag_curated_src".into(),
                up_to_seq: None,
                target: ForkTarget::SameDirectory,
                title: None,
                initial_prompt: None,
            })
            .await
            .expect("curated same-dir fork");
        assert!(curated_fork.workspace.is_none());
        assert_eq!(curated_fork.agent.workspace_id, "ws_curated");
        let ws_events = store
            .list_workspace_events("ws_curated", 20)
            .expect("ws events");
        assert!(
            ws_events
                .iter()
                .any(|e| e.kind == WorkspaceEventKind::PeerJoined
                    && e.agent_id.as_deref() == Some(curated_fork.agent.agent_id.as_str())),
            "curated workspace fork must record PeerJoined event"
        );

        // (b) Mid-thread fork at `s2` (with source holding a persistence_ref!) must strictly
        // truncate timeline AND handoff context and NEVER inherit parent persistence_ref.
        store
            .set_persistence_ref("ag_src", "parent_claude_sess_999")
            .expect("set parent pref");
        let mid_out = orch
            .fork_agent(ForkAgentSpec {
                source_agent_id: "ag_src".into(),
                up_to_seq: Some(s2),
                target: ForkTarget::SameDirectory,
                title: Some("Alt Auth Branch".into()),
                initial_prompt: None,
            })
            .await
            .expect("mid-thread fork");
        assert_eq!(mid_out.agent.title, "Alt Auth Branch");
        assert_eq!(
            mid_out
                .agent
                .labels
                .get("orbit.title_locked")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            mid_out.agent.forked_from_agent_id.as_deref(),
            Some("ag_src")
        );
        assert_eq!(mid_out.agent.forked_at_seq, Some(s2));
        assert_eq!(
            store
                .get_persistence_ref(&mid_out.agent.agent_id)
                .expect("mid pref"),
            None,
            "mid-thread fork must not inherit parent persistence_ref"
        );

        let (mid_events, _) = store
            .fetch_timeline_since(&mid_out.agent.agent_id, 0, 50)
            .expect("fetch mid");
        assert_eq!(mid_events.len(), 2);
        let mid_handoff = orch
            .build_handoff_context(&mid_out.agent.agent_id)
            .expect("mid handoff");
        assert!(mid_handoff.contains("Design token rotation for AuthService"));
        assert!(mid_handoff.contains("rotating refresh tokens"));
        assert!(
            !mid_handoff.contains("POST_FORK_SECRET_PROMPT_XYZ"),
            "post-fork prompt must never leak into handoff context"
        );
        assert!(
            !mid_handoff.contains("POST_FORK_SECRET_REPLY_XYZ"),
            "post-fork reply must never leak into handoff context"
        );

        // (c) Git flag / ref injection MUST be rejected upfront with zero leaked workspaces!
        let ws_count_before = store.list_workspaces().expect("ws list").len();
        let bad_branch_err = orch
            .fork_agent(ForkAgentSpec {
                source_agent_id: "ag_src".into(),
                up_to_seq: None,
                target: ForkTarget::NewWorktree {
                    branch: Some("-f".into()),
                    base_ref: None,
                },
                title: None,
                initial_prompt: None,
            })
            .await
            .expect_err("leading-dash branch must be rejected");
        assert!(
            bad_branch_err.to_string().contains("invalid_git_ref"),
            "unexpected branch error: {bad_branch_err}"
        );

        let bad_base_err = orch
            .fork_agent(ForkAgentSpec {
                source_agent_id: "ag_src".into(),
                up_to_seq: None,
                target: ForkTarget::NewWorktree {
                    branch: Some("orbit/fork-safe-name".into()),
                    base_ref: Some("--detach".into()),
                },
                title: None,
                initial_prompt: None,
            })
            .await
            .expect_err("leading-dash base_ref must be rejected");
        assert!(
            bad_base_err.to_string().contains("invalid_git_ref"),
            "unexpected base_ref error: {bad_base_err}"
        );
        assert_eq!(
            store.list_workspaces().expect("ws list").len(),
            ws_count_before,
            "rejected git ref forks must not leave leaked workspace rows"
        );

        // (d) NewWorktree fork creates a real git worktree via the accelerated path
        let wt_src = seed_source_conversation(&store, "ws_repo", &repo_str, "ag_src_wt", None);
        let wt_out = orch
            .fork_agent(ForkAgentSpec {
                source_agent_id: "ag_src_wt".into(),
                up_to_seq: Some(wt_src.1),
                target: ForkTarget::NewWorktree {
                    branch: Some("orbit/fork-alt-auth".into()),
                    base_ref: Some("main".into()),
                },
                title: None,
                initial_prompt: None,
            })
            .await
            .expect("new worktree fork");
        let wt_ws = wt_out
            .workspace
            .as_ref()
            .expect("worktree fork must return created workspace");
        assert!(wt_ws.is_worktree);
        assert_eq!(wt_ws.git_branch.as_deref(), Some("orbit/fork-alt-auth"));
        assert_eq!(wt_out.agent.workspace_id, wt_ws.workspace_id);
        assert_eq!(wt_out.agent.cwd, wt_ws.cwd);
        assert_ne!(wt_out.agent.cwd, repo_str);
        assert!(Path::new(&wt_ws.cwd).join("README.md").is_file());
        assert_eq!(
            wt_out.agent.forked_from_agent_id.as_deref(),
            Some("ag_src_wt")
        );
        assert_eq!(wt_out.agent.forked_at_seq, Some(wt_src.1));

        // (e) Forking an agent INSIDE a worktree into a second NewWorktree with `base_ref: None`
        // must branch from the source worktree's branch (carrying its unique commit!), NOT main!
        let wt_dir = PathBuf::from(&wt_ws.cwd);
        std::fs::write(wt_dir.join("WT_ONLY.txt"), "from worktree branch\n")
            .expect("write wt file");
        let run_git_wt = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&wt_dir)
                .output()
                .expect("run git wt");
            assert!(
                out.status.success(),
                "git {args:?} in wt failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run_git_wt(&["add", "WT_ONLY.txt"]);
        run_git_wt(&["commit", "-m", "worktree branch commit"]);

        let wt_child = orch
            .fork_agent(ForkAgentSpec {
                source_agent_id: wt_out.agent.agent_id.clone(),
                up_to_seq: None,
                target: ForkTarget::NewWorktree {
                    branch: Some("orbit/fork-from-wt".into()),
                    base_ref: None,
                },
                title: None,
                initial_prompt: None,
            })
            .await
            .expect("nested worktree fork");
        let wt_child_ws = wt_child
            .workspace
            .as_ref()
            .expect("nested worktree fork workspace");
        assert!(
            Path::new(&wt_child_ws.cwd).join("WT_ONLY.txt").is_file(),
            "forking a worktree agent with base_ref=None must branch from the worktree's branch"
        );

        // Non-git directory returns clean not_a_git_repo error for NewWorktree
        let plain_str = non_git_dir.to_string_lossy().to_string();
        seed_source_conversation(&store, "ws_plain", &plain_str, "ag_plain", None);
        let err = orch
            .fork_agent(ForkAgentSpec {
                source_agent_id: "ag_plain".into(),
                up_to_seq: None,
                target: ForkTarget::NewWorktree {
                    branch: None,
                    base_ref: None,
                },
                title: None,
                initial_prompt: None,
            })
            .await
            .expect_err("non-git worktree fork must fail cleanly");
        assert!(
            err.to_string().contains("not_a_git_repo"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(&tmp_root);
    }
}

#[cfg(test)]
mod fast_mode_orchestrator_tests {
    use super::*;
    use std::sync::OnceLock;

    fn ensure_mock_provider_binaries() {
        static INIT: OnceLock<()> = OnceLock::new();
        INIT.get_or_init(|| {
            let exe = if Path::new("/usr/bin/true").exists() {
                PathBuf::from("/usr/bin/true")
            } else {
                PathBuf::from("/bin/true")
            };
            orbit_providers::set_provider_executable_override(
                ProviderKind::ClaudeCode,
                exe.clone(),
            );
            orbit_providers::set_provider_executable_override(ProviderKind::Codex, exe.clone());
            orbit_providers::set_provider_executable_override(ProviderKind::Antigravity, exe);
        });
    }

    async fn test_orchestrator(tag: &str) -> (PathBuf, Arc<OrbitStore>, Arc<AgentOrchestrator>) {
        ensure_mock_provider_binaries();
        let dir =
            std::env::temp_dir().join(format!("orbit_mgr_fast_{tag}_{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join("shims")).expect("create shims dir");
        let store = Arc::new(OrbitStore::open(dir.join("orbit.db")).expect("open store"));
        let listen_port = Arc::new(RwLock::new(0u16));
        let orch_config = Arc::new(RwLock::new(OrchestrationConfig::default()));
        let scripts = WorkspaceScriptRegistry::new();
        let keep_awake = Arc::new(KeepAwakeManager::new(false));
        let provider_catalogs = Arc::new(RwLock::new(None));
        let hooks = crate::hooks::HooksEngine::new(dir.clone());
        let orch = AgentOrchestrator::new(
            dir.clone(),
            "host_test".into(),
            "tok_test".into(),
            listen_port,
            orch_config,
            store.clone(),
            scripts,
            keep_awake,
            provider_catalogs,
            hooks,
        )
        .await;
        (dir, store, orch)
    }

    fn mk_spec(
        cwd: &Path,
        provider: ProviderKind,
        model: Option<&str>,
        profile_id: Option<&str>,
        fast_mode: Option<bool>,
    ) -> CreateAgentSpec {
        CreateAgentSpec {
            cwd: cwd.to_string_lossy().to_string(),
            extra_dirs: Vec::new(),
            title: Some("Test Agent".into()),
            provider,
            model: model.map(ToOwned::to_owned),
            effort: None,
            fast_mode,
            mode: SessionMode::AcceptEdits,
            initial_prompt: String::new(),
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree: false,
            workspace_id: None,
            parent_agent_id: None,
            parent_host_id: None,
            profile_id: profile_id.map(ToOwned::to_owned),
            background: false,
            auto_archive: false,
            base_ref: None,
            worktree_mode: WorktreeCreateMode::BranchOff,
            branch: None,
            pr_number: None,
            handle: None,
            auto_cluster: Some(false),
            acp_agent_id: None,
        }
    }

    #[tokio::test]
    async fn create_agent_explicit_false_overrides_fast_profile_and_gates_capability() {
        let (dir, store, orch) = test_orchestrator("create_override").await;
        let now = Utc::now();
        store
            .upsert_profile(&orbit_protocol::AgentProfile {
                profile_id: "prof_fast".into(),
                name: "Fast Opus".into(),
                provider: ProviderKind::ClaudeCode,
                acp_agent_id: None,
                model: Some("claude-opus-4-6".into()),
                effort: Some(ReasoningEffort::High),
                fast_mode: true,
                mode: SessionMode::AcceptEdits,
                when_to_use: "Fast coding".into(),
                system_prompt_append: None,
                mcp_policy: orbit_protocol::McpToolPolicy::default(),
                created_at: now,
                updated_at: now,
            })
            .expect("save profile");

        // 1. Explicit `Some(false)` overrides `profile.fast_mode == true`
        let ag_override = orch
            .create_and_start_agent(mk_spec(
                &dir,
                ProviderKind::ClaudeCode,
                None,
                Some("prof_fast"),
                Some(false),
            ))
            .await
            .expect("create with explicit false");
        assert!(
            !ag_override.fast_mode,
            "explicit fast_mode=Some(false) must override profile.fast_mode=true"
        );
        let persisted = store
            .get_agent(&ag_override.agent_id)
            .expect("get")
            .expect("exists");
        assert!(!persisted.fast_mode);

        // 2. `None` inherits `profile.fast_mode == true` on a fast-capable model
        let ag_inherit = orch
            .create_and_start_agent(mk_spec(
                &dir,
                ProviderKind::ClaudeCode,
                None,
                Some("prof_fast"),
                None,
            ))
            .await
            .expect("create inheriting profile fast_mode");
        assert!(
            ag_inherit.fast_mode,
            "fast_mode=None must inherit profile.fast_mode=true"
        );

        // 3. `None` inheriting fast profile while overridden to unsupported model clamps to `false`
        let ag_clamped = orch
            .create_and_start_agent(mk_spec(
                &dir,
                ProviderKind::ClaudeCode,
                Some("claude-sonnet-4-6"),
                Some("prof_fast"),
                None,
            ))
            .await
            .expect("create clamped to false on unsupported model");
        assert!(
            !ag_clamped.fast_mode,
            "inherited profile fast_mode must clamp to false on unsupported model"
        );

        // 4. Explicit `Some(true)` on an unsupported model returns an actionable error
        let err = orch
            .create_and_start_agent(mk_spec(
                &dir,
                ProviderKind::ClaudeCode,
                Some("claude-haiku-4-5"),
                None,
                Some(true),
            ))
            .await
            .expect_err("explicit fast_mode=true on haiku must be rejected");
        assert!(
            err.to_string().contains("Fast mode is not supported"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn model_and_provider_switches_enforce_fast_mode_capability() {
        let (dir, store, orch) = test_orchestrator("capability_switch").await;
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_cap".into(),
                title: "Cap".into(),
                cwd: dir.to_string_lossy().to_string(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");

        let ag = AgentSummary {
            agent_id: "ag_cap".into(),
            workspace_id: "ws_cap".into(),
            cwd: dir.to_string_lossy().to_string(),
            title: "Cap Agent".into(),
            provider: ProviderKind::ClaudeCode,
            model: Some("claude-opus-4-6".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: true,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: Some("lead".into()),
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };
        store.upsert_agent(&ag, None).expect("seed agent");

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<DriverCommand>(16);
        let done_notify = Arc::new(tokio::sync::Notify::new());
        done_notify.notify_one();
        let dummy_task = tokio::spawn(std::future::pending::<()>());
        orch.live_agents.write().await.insert(
            "ag_cap".into(),
            LiveAgentEntry {
                generation: 1,
                cmd_tx,
                done_notify,
                ingest_abort: dummy_task.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        // 1. `update_agent` switching to non-fast model auto-clears fast_mode in DB + live driver
        let updated = orch
            .update_agent(
                "ag_cap",
                None,
                Some("claude-sonnet-4-6".into()),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("update_agent to sonnet");
        assert!(!updated.fast_mode);
        let cmd = cmd_rx.try_recv().expect("driver SetConfig on update_agent");
        assert!(matches!(
            cmd,
            DriverCommand::SetConfig {
                model: Some(ref m),
                fast_mode: Some(false),
                ..
            } if m == "claude-sonnet-4-6"
        ));

        // 2. `update_agent` rejects explicit `fast_mode: Some(true)` on non-fast model
        let err = orch
            .update_agent(
                "ag_cap",
                None,
                None,
                None,
                Some(true),
                None,
                None,
                None,
                None,
            )
            .await
            .expect_err("explicit fast_mode=true on sonnet must fail");
        assert!(err.to_string().contains("Fast mode is not supported"));

        // 3. Re-enable fast_mode on opus, then `set_agent_config` to haiku clears DB + sends Some(false)
        let re_enabled = orch
            .update_agent(
                "ag_cap",
                None,
                Some("claude-opus-4-6".into()),
                None,
                Some(true),
                None,
                None,
                None,
                None,
            )
            .await
            .expect("re-enable fast_mode on opus");
        assert!(re_enabled.fast_mode);
        let _ = cmd_rx.try_recv();

        orch.set_agent_config(
            "ag_cap",
            Some("claude-haiku-4-5".into()),
            Some(ReasoningEffort::Low),
        )
        .await
        .expect("set_agent_config");
        assert!(
            !store
                .get_agent("ag_cap")
                .expect("get")
                .expect("some")
                .fast_mode
        );
        let cmd2 = cmd_rx
            .try_recv()
            .expect("driver SetConfig on set_agent_config");
        assert!(matches!(
            cmd2,
            DriverCommand::SetConfig {
                model: Some(ref m),
                fast_mode: Some(false),
                ..
            } if m == "claude-haiku-4-5"
        ));

        // 4. Re-enable fast_mode on opus, then `set_agent_model` to sonnet clears DB + sends Some(false)
        orch.update_agent(
            "ag_cap",
            None,
            Some("claude-opus-4-6".into()),
            None,
            Some(true),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("re-enable on opus");
        let _ = cmd_rx.try_recv();

        orch.set_agent_model("ag_cap", Some("claude-sonnet-4-6".into()))
            .await
            .expect("set_agent_model");
        assert!(
            !store
                .get_agent("ag_cap")
                .expect("get")
                .expect("some")
                .fast_mode
        );
        let cmd3 = cmd_rx
            .try_recv()
            .expect("driver SetConfig on set_agent_model");
        assert!(matches!(
            cmd3,
            DriverCommand::SetConfig {
                model: Some(ref m),
                fast_mode: Some(false),
                ..
            } if m == "claude-sonnet-4-6"
        ));

        // 5. Re-enable fast_mode on opus, then `switch_agent_provider` to Codex `gpt-oss-120b` clears fast_mode
        orch.update_agent(
            "ag_cap",
            None,
            Some("claude-opus-4-6".into()),
            None,
            Some(true),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("re-enable on opus");
        let _ = cmd_rx.try_recv();

        orch.switch_agent_provider(
            "ag_cap",
            ProviderKind::Codex,
            Some("gpt-oss-120b".into()),
            None,
            None,
        )
        .await
        .expect("switch_agent_provider");
        let switched = store.get_agent("ag_cap").expect("get").expect("some");
        assert_eq!(switched.provider, ProviderKind::Codex);
        assert!(!switched.fast_mode);
        let cmd4 = cmd_rx
            .try_recv()
            .expect("driver SetConfig on switch_agent_provider");
        assert!(matches!(
            cmd4,
            DriverCommand::SetConfig {
                fast_mode: Some(false),
                ..
            }
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn orchestrator_local_to_worktree_handoff_creates_worktree_moves_changes_and_cleans_local()
     {
        let root = std::env::temp_dir().join(format!("orbit_mgr_handoff_{}", Uuid::new_v4()));
        let repo_dir = root.join("demo-repo");
        std::fs::create_dir_all(&repo_dir).expect("mkdir repo");

        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@orbit.local"],
            vec!["config", "user.name", "Orbit Test"],
        ] {
            let st = tokio::process::Command::new("git")
                .args(&args)
                .current_dir(&repo_dir)
                .status()
                .await
                .expect("git init/config");
            assert!(st.success());
        }
        std::fs::write(repo_dir.join("main.rs"), "fn main() {}\n").expect("write main.rs");
        for args in [vec!["add", "-A"], vec!["commit", "-q", "-m", "init"]] {
            let st = tokio::process::Command::new("git")
                .args(&args)
                .current_dir(&repo_dir)
                .status()
                .await
                .expect("git commit");
            assert!(st.success());
        }

        let (home_dir, _store, orch) = test_orchestrator("handoff").await;
        let local_ws = orch
            .ensure_workspace(repo_dir.to_str().expect("utf8"), None, None, None)
            .await
            .expect("ensure local workspace");

        // Create staged + unstaged + untracked edits in local checkout
        std::fs::write(
            repo_dir.join("main.rs"),
            "fn main() { println!(\"moved\"); }\n",
        )
        .expect("edit main");
        let st_add = tokio::process::Command::new("git")
            .args(["add", "main.rs"])
            .current_dir(&repo_dir)
            .status()
            .await
            .expect("git add");
        assert!(st_add.success());
        std::fs::write(repo_dir.join("extra.txt"), "untracked-note\n").expect("write extra");

        // Preview Local -> New worktree
        let preview = orch
            .preview_worktree_handoff(
                &local_ws.workspace_id,
                WorktreeHandoffDirection::LocalToWorktree,
                None,
                false,
            )
            .await
            .expect("preview");
        assert!(preview.can_apply_cleanly);
        assert_eq!(preview.files.len(), 2);

        // Reject flag-injection branch name upfront
        let bad = orch
            .execute_worktree_handoff(ExecuteWorktreeHandoffSpec {
                workspace_id: local_ws.workspace_id.clone(),
                direction: WorktreeHandoffDirection::LocalToWorktree,
                mode: WorktreeHandoffMode::Move,
                target_workspace_id: None,
                include_committed: false,
                branch: Some("--bad-branch".into()),
                allow_three_way_conflicts: false,
            })
            .await;
        assert!(bad.is_err());

        // Execute Local -> New worktree Move
        let outcome = orch
            .execute_worktree_handoff(ExecuteWorktreeHandoffSpec {
                workspace_id: local_ws.workspace_id.clone(),
                direction: WorktreeHandoffDirection::LocalToWorktree,
                mode: WorktreeHandoffMode::Move,
                target_workspace_id: None,
                include_committed: false,
                branch: Some("feat/from-local".into()),
                allow_three_way_conflicts: false,
            })
            .await
            .expect("execute move to new worktree");

        assert_eq!(outcome.applied_files, 2);
        assert!(outcome.backup_ref.is_some());
        let new_ws = outcome.target_workspace.expect("created target workspace");
        assert!(new_ws.is_worktree);
        assert_eq!(new_ws.git_branch.as_deref(), Some("feat/from-local"));

        // Verify new worktree has both files
        let wt_path = PathBuf::from(&new_ws.cwd);
        assert_eq!(
            std::fs::read_to_string(wt_path.join("main.rs")).expect("wt main"),
            "fn main() { println!(\"moved\"); }\n"
        );
        assert_eq!(
            std::fs::read_to_string(wt_path.join("extra.txt")).expect("wt extra"),
            "untracked-note\n"
        );

        // Verify local checkout is clean
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("main.rs")).expect("local main"),
            "fn main() {}\n"
        );
        assert!(!repo_dir.join("extra.txt").exists());
        let status = inspect_git_status(&repo_dir).await;
        assert!(status.files.is_empty(), "local checkout must be clean");

        orch.shutdown_worktree_pool().await;
        let _ = std::fs::remove_dir_all(&home_dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn orchestrator_local_to_worktree_failure_rolls_back_worktree_and_branch() {
        let root = std::env::temp_dir().join(format!("orbit_mgr_handoff_rb_{}", Uuid::new_v4()));
        let repo_dir = root.join("demo-repo");
        std::fs::create_dir_all(&repo_dir).expect("mkdir repo");

        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "test@orbit.local"],
            vec!["config", "user.name", "Orbit Test"],
        ] {
            let st = tokio::process::Command::new("git")
                .args(&args)
                .current_dir(&repo_dir)
                .status()
                .await
                .expect("git init/config");
            assert!(st.success());
        }
        std::fs::write(repo_dir.join("main.rs"), "fn main() { /* v1 */ }\n")
            .expect("write main.rs");
        for args in [vec!["add", "-A"], vec!["commit", "-q", "-m", "init"]] {
            let st = tokio::process::Command::new("git")
                .args(&args)
                .current_dir(&repo_dir)
                .status()
                .await
                .expect("git commit");
            assert!(st.success());
        }

        // Trigger a genuine post-provision conflict: `.worktreeinclude` copies modified `main.rs`
        // into the freshly created worktree, causing `git apply` of the `HEAD -> modified` diff
        // to fail cleanly inside `execute_worktree_handoff_paths`.
        std::fs::write(repo_dir.join(".worktreeinclude"), "main.rs\n")
            .expect("write .worktreeinclude");
        std::fs::write(
            repo_dir.join("main.rs"),
            "fn main() { println!(\"v2-dirty\"); }\n",
        )
        .expect("modify main.rs");

        let (home_dir, store, orch) = test_orchestrator("handoff_rollback").await;
        let local_ws = orch
            .ensure_workspace(repo_dir.to_str().expect("utf8"), None, None, None)
            .await
            .expect("ensure local workspace");

        let err = orch
            .execute_worktree_handoff(ExecuteWorktreeHandoffSpec {
                workspace_id: local_ws.workspace_id.clone(),
                direction: WorktreeHandoffDirection::LocalToWorktree,
                mode: WorktreeHandoffMode::Move,
                target_workspace_id: None,
                include_committed: false,
                branch: Some("feat/rollback-branch".into()),
                allow_three_way_conflicts: false,
            })
            .await
            .expect_err("must fail due to post-provision patch conflict");
        assert!(err.to_string().contains("handoff_conflict"));

        // 1. No active worktree workspace left in store
        let active_ws = store
            .list_workspaces_filtered(false, true)
            .expect("list active ws");
        assert!(
            active_ws.iter().all(|w| !w.is_worktree),
            "orphaned worktree workspace must be archived on rollback"
        );

        // 2. Newly created branch MUST be deleted on rollback
        let branch_check = tokio::process::Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                "refs/heads/feat/rollback-branch",
            ])
            .current_dir(&repo_dir)
            .status()
            .await
            .expect("show-ref");
        assert!(
            !branch_check.success(),
            "newly created branch `feat/rollback-branch` must be deleted on rollback"
        );

        // 3. No linked worktree directory remains in `git worktree list`
        let wt_list = tokio::process::Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(&repo_dir)
            .output()
            .await
            .expect("worktree list");
        let wt_text = String::from_utf8_lossy(&wt_list.stdout);
        assert!(
            !wt_text.contains("feat/rollback-branch"),
            "orphaned worktree must be removed: {wt_text}"
        );

        // 4. Local dirty changes were NEVER reset
        assert_eq!(
            std::fs::read_to_string(repo_dir.join("main.rs")).expect("local main intact"),
            "fn main() { println!(\"v2-dirty\"); }\n"
        );

        orch.shutdown_worktree_pool().await;
        let _ = std::fs::remove_dir_all(&home_dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sanitize_timeline_payload_caps_oversized_file_edit_diff() {
        let mut giant = String::from("--- a/huge.rs\n+++ b/huge.rs\n@@ -1,1 +1,1500 @@\n");
        for i in 0..1500 {
            giant.push_str(&format!(
                "+let line_{i} = \"payload_padding_0123456789\";\n"
            ));
        }
        assert!(giant.len() > orbit_providers::MAX_UNIFIED_DIFF_BYTES);
        let mut payload = TimelinePayload::ToolCall {
            call_id: "c_big".into(),
            status: ToolCallStatus::Completed,
            detail: ToolCallDetail::FileEdit {
                path: "huge.rs".into(),
                unified_diff: Some(giant),
            },
            background_task: None,
        };
        sanitize_timeline_payload(&mut payload);
        match payload {
            TimelinePayload::ToolCall {
                detail: ToolCallDetail::FileEdit { unified_diff, .. },
                ..
            } => {
                let d = unified_diff.expect("diff preserved");
                assert!(d.len() <= orbit_providers::MAX_UNIFIED_DIFF_BYTES);
                assert!(d.contains("diff truncated at 24 KB"));
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }
}
