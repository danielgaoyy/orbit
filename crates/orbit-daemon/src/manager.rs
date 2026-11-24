//! Core Agent, Workspace/Worktree, Script, and Detached PTY lifecycle manager inside `orbit-daemon`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Utc;
use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageDeliveryStatus, AgentMessageKind, AgentStatus, AgentSummary,
    AttentionState, ForkTarget, HostProjectEntry, HostSyncState, KeepAwakeStatus,
    OrchestrationConfig, PendingPermissionSummary, PromptBadge, ProviderKind, ProviderModelCatalog,
    ProviderSubagentDescriptor, ProviderSubagentEvent, QuestionItem, ReasoningEffort, ScheduleSpec,
    SessionMode, TerminalSummary, TimelineEvent, TimelinePayload, ToolCallDetail, ToolCallStatus,
    WorkspaceBoardSnapshot, WorkspaceEventKind, WorkspaceEventRecord, WorkspaceIsolation,
    WorkspaceOrganizeEvent, WorkspaceScriptInfo, WorkspaceScriptKind, WorkspaceScriptStatus,
    WorkspaceSetupProgress, WorkspaceSetupStatus, WorkspaceSummary, WorktreeCreateMode,
    WorktreeHandoffDirection, WorktreeHandoffMode, WorktreeHandoffOutcome, WorktreeHandoffPreview,
};
use orbit_providers::{
    DriverCommand, DriverEvent, OrbitMcpLaunchSpec, PersistedBackgroundTask, PromptOrigin,
    SessionLaunchConfig, find_provider_executable, provider_model_supports_fast_mode,
    spawn_provider_session,
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
use crate::messaging::{
    AgentMessageDelivery, AgentPromptSource, CoalescedDispatchBatch, QueuedAgentMessage,
    sanitize_human_prompt_for_wire,
};
use crate::workspace_board::{
    WorkspaceRosterState, board_file_path, build_workspace_roster_context_incremental,
    default_board_template, upsert_markdown_section, write_board_file,
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
    pub source_agent_id: Option<String>,
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
    WorkspaceFilesChanged {
        workspace_id: String,
        cwd: String,
        changed_paths: Vec<String>,
        git_changed: bool,
        skills_or_rules_changed: bool,
    },
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
    ConditionalTimerEvent(orbit_protocol::ConditionalTimerRecord),
    GoalRunEvent(orbit_protocol::GoalRunRecord),
    MonitorWatchEvent(orbit_protocol::MonitorWatchRecord),
    WorkflowRunEvent(orbit_protocol::WorkflowRunRecord),
    FleetVaultChanged {
        origin_session_id: Uuid,
        vault_fingerprint: String,
        revision: u64,
    },
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

/// RAII guard tracking an in-flight `DriverEvent::PermissionRequest` while `PreToolUse` hooks
/// and/or Guardian classification are evaluating. Guarantees cleanup on every early return or drop.
struct EvaluatingPermissionGuard {
    set: Arc<std::sync::Mutex<std::collections::HashSet<(String, String)>>>,
    key: Option<(String, String)>,
}

impl EvaluatingPermissionGuard {
    fn disarm(&mut self) {
        if let Some(key) = self.key.take() {
            self.set
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&key);
        }
    }
}

impl Drop for EvaluatingPermissionGuard {
    fn drop(&mut self) {
        self.disarm();
    }
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
    pub guardian: Arc<crate::guardian::GuardianManager>,
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
    /// In-flight `(agent_id, request_id)` permissions currently evaluating `PreToolUse` hooks or
    /// Guardian classification. Any external `resolve_permission` call is rejected until settled.
    evaluating_permissions: Arc<std::sync::Mutex<std::collections::HashSet<(String, String)>>>,
    /// Recently Guardian-auto-resolved `(agent_id, request_id)` pairs so trailing
    /// `StatusChanged::AwaitingPermission` events do not flash stale `AttentionState::Permission`.
    guardian_auto_resolved_requests: RwLock<std::collections::HashSet<(String, String)>>,
    /// Recently dispatched `PostToolUse` `(agent_id, call_id)` pairs so repeated terminal
    /// tool-call snapshots do not double-fire hooks.
    post_tool_hook_dispatched: RwLock<std::collections::HashSet<(String, String)>>,
    /// Incremental `<orbit-workspace>` roster tracking state per `agent_id`.
    roster_states: RwLock<HashMap<String, WorkspaceRosterState>>,
    /// Per-agent injected `<WORKSPACE_RULES>` hashes `(agent_id -> (rule_id -> content_hash))`.
    injected_rule_hashes: RwLock<HashMap<String, HashMap<String, String>>>,
    /// Per-repository mutex serializing concurrent `handoff_worktree` mutations into shared checkouts.
    repo_handoff_locks: RwLock<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
    /// Conflict radar cooldown keyed by `(workspace_id, canonical_file_path)`.
    conflict_cooldowns: RwLock<HashMap<(String, String), std::time::Instant>>,
    /// Agents whose next idle transition was caused by explicit interrupt or flush-now,
    /// suppressing automatic queue drain for that transition.
    suppress_auto_drain: RwLock<std::collections::HashSet<String>>,
    /// Agents that already persisted provider-reported `DriverEvent::TurnUsage` for the active turn,
    /// avoiding double-counting when the turn settles to `Idle`.
    turn_usage_recorded: RwLock<std::collections::HashSet<String>>,
    /// Most recent provider-reported instantaneous single-call context telemetry per `agent_id`
    /// for accurate context occupancy without multi-step turn-sum inflation.
    last_call_prompt_occupancy: RwLock<HashMap<String, orbit_store::AgentContextTelemetry>>,
    /// Enqueue timestamps (`message_id -> queued_at_ms`) for wait/settlement telemetry (capped at 4096 entries).
    notification_queued_at: RwLock<HashMap<String, i64>>,
    /// In-flight AI organizer / summarizer locks keyed by `sum:<agent_id>` or `org:<project_root>`.
    /// Uses a synchronous `std::sync::Mutex` (never held across `.await`) so RAII `OrganizerLockGuard`
    /// always releases the lock and marks `keep_awake` idle on cancellation or panic.
    organizing_locks: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Bounded concurrency limiter for background Turn-1 AI auto-naming & summary tasks
    /// so bursts of completing sessions never saturate local CPU or provider API rate limits.
    auto_name_semaphore: Arc<tokio::sync::Semaphore>,
    conditional_timers: RwLock<Option<Arc<crate::timers::ConditionalTimerEngine>>>,
    message_delivery: RwLock<Option<Arc<AgentMessageDelivery>>>,
    personal_router: RwLock<Option<Arc<crate::personal_router::PersonalRouterManager>>>,
    workspace_watcher: RwLock<Option<Arc<crate::workspace_watcher::WorkspaceWatcherManager>>>,
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

/// Offload a synchronous `OrbitStore` operation onto Tokio's blocking pool so SQLite WAL
/// locking and fsync never starve async WebSocket, PTY, or streaming provider tasks.
pub(crate) async fn with_store<T, F>(store: &Arc<OrbitStore>, f: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce(&OrbitStore) -> T + Send + 'static,
{
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || f(&store))
        .await
        .map_err(|e| anyhow::anyhow!("OrbitStore blocking task failed: {e}"))
}

/// Offload a fallible synchronous `OrbitStore` operation onto Tokio's blocking pool,
/// flattening both `JoinError` and inner error types into `anyhow::Result<T>`.
pub(crate) async fn try_store<T, E, F>(store: &Arc<OrbitStore>, f: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    E: Into<anyhow::Error> + Send + 'static,
    F: FnOnce(&OrbitStore) -> Result<T, E> + Send + 'static,
{
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || f(&store))
        .await
        .map_err(|e| anyhow::anyhow!("OrbitStore blocking task failed: {e}"))?
        .map_err(Into::into)
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
        let guardian = crate::guardian::GuardianManager::with_tier2(
            home_dir.clone(),
            Some(Arc::clone(&store)),
            Arc::new(crate::guardian_llm::LlmGuardianClassifier::new(Some(
                Arc::clone(&provider_catalogs),
            ))),
        );
        Arc::new(Self {
            home_dir,
            orbit_exe,
            host_id,
            mcp_token,
            listen_port,
            orch_config,
            store,
            scripts,
            guardian,
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
            evaluating_permissions: Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )),
            guardian_auto_resolved_requests: RwLock::new(std::collections::HashSet::new()),
            post_tool_hook_dispatched: RwLock::new(std::collections::HashSet::new()),
            roster_states: RwLock::new(HashMap::new()),
            injected_rule_hashes: RwLock::new(HashMap::new()),
            repo_handoff_locks: RwLock::new(HashMap::new()),
            conflict_cooldowns: RwLock::new(HashMap::new()),
            suppress_auto_drain: RwLock::new(std::collections::HashSet::new()),
            turn_usage_recorded: RwLock::new(std::collections::HashSet::new()),
            last_call_prompt_occupancy: RwLock::new(HashMap::new()),
            notification_queued_at: RwLock::new(HashMap::new()),
            organizing_locks: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
            auto_name_semaphore: Arc::new(tokio::sync::Semaphore::new(2)),
            conditional_timers: RwLock::new(None),
            message_delivery: RwLock::new(None),
            personal_router: RwLock::new(None),
            workspace_watcher: RwLock::new(None),
            broadcast_tx,
        })
    }

    pub async fn set_conditional_timers(&self, timers: Arc<crate::timers::ConditionalTimerEngine>) {
        *self.conditional_timers.write().await = Some(timers);
    }

    async fn conditional_timers(&self) -> Option<Arc<crate::timers::ConditionalTimerEngine>> {
        self.conditional_timers.read().await.clone()
    }

    pub async fn set_message_delivery(&self, delivery: Arc<AgentMessageDelivery>) {
        *self.message_delivery.write().await = Some(delivery);
    }

    async fn message_delivery(&self) -> Option<Arc<AgentMessageDelivery>> {
        self.message_delivery.read().await.clone()
    }

    pub async fn set_personal_router(
        &self,
        router: Arc<crate::personal_router::PersonalRouterManager>,
    ) {
        *self.personal_router.write().await = Some(router);
    }

    async fn personal_router(&self) -> Option<Arc<crate::personal_router::PersonalRouterManager>> {
        self.personal_router.read().await.clone()
    }

    pub async fn set_workspace_watcher(
        &self,
        watcher: Arc<crate::workspace_watcher::WorkspaceWatcherManager>,
    ) {
        *self.workspace_watcher.write().await = Some(watcher);
    }

    /// Invalidate per-agent cached `<WORKSPACE_RULES>` injection hashes for every active agent
    /// inside `workspace_id` so edited `.orbit/rules/*`, `.claude/rules/*`, or `AGENTS.md` files
    /// take effect immediately on the next prompt turn.
    pub async fn invalidate_workspace_rules_cache(&self, workspace_id: &str) {
        let ws_id = workspace_id.to_string();
        let Ok(agents) = self
            .try_store(move |s| s.list_agents_filtered(Some(&ws_id), false, true, None))
            .await
        else {
            return;
        };
        if agents.is_empty() {
            return;
        }
        let mut guard = self.injected_rule_hashes.write().await;
        for ag in agents {
            guard.remove(&ag.agent_id);
        }
    }

    pub fn are_all_workspace_peers_settled(&self, agent_id: &str) -> bool {
        let Some(agent) = self.store.get_agent(agent_id).ok().flatten() else {
            return false;
        };
        let peers = self
            .store
            .list_agents_filtered(Some(&agent.workspace_id), false, false, None)
            .unwrap_or_default();
        !peers.into_iter().any(|p| {
            p.agent_id != agent_id
                && p.parent_agent_id.is_none()
                && (p.status.is_foreground_busy()
                    || matches!(p.status, AgentStatus::AwaitingPermission { .. }))
        })
    }

    async fn acquire_repo_handoff_lock(&self, checkout_dir: &Path) -> Arc<tokio::sync::Mutex<()>> {
        let key = match resolve_git_checkout_info(checkout_dir).await {
            Ok(info) => info.common_dir,
            Err(_) => PathBuf::from(self.resolve_canonical_repo_key(checkout_dir).await),
        };
        if let Some(lock) = self.repo_handoff_locks.read().await.get(&key) {
            return Arc::clone(lock);
        }
        let mut guard = self.repo_handoff_locks.write().await;
        Arc::clone(
            guard
                .entry(key)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    fn are_all_subagents_settled_from_store(store: &OrbitStore, parent_agent_id: &str) -> bool {
        let managed_busy = store
            .list_agents_filtered(None, false, true, Some(parent_agent_id))
            .map(|children| {
                children.into_iter().any(|child| {
                    child.status.is_foreground_busy()
                        || matches!(child.status, AgentStatus::AwaitingPermission { .. })
                })
            })
            .unwrap_or(false);
        if managed_busy {
            return false;
        }
        let native_running = store
            .list_provider_subagents(parent_agent_id)
            .map(|subs| {
                subs.into_iter()
                    .any(|s| s.status == orbit_protocol::ProviderSubagentStatus::Running)
            })
            .unwrap_or(false);
        !native_running
    }

    async fn are_all_subagents_settled_async(&self, parent_agent_id: &str) -> bool {
        let pid = parent_agent_id.to_string();
        self.with_store(move |s| Self::are_all_subagents_settled_from_store(s, &pid))
            .await
            .unwrap_or(false)
    }

    /// Offload a synchronous `OrbitStore` closure onto Tokio's blocking pool so heavy SQLite
    /// WAL reads/writes never stall async WebSocket, PTY, or streaming provider tasks.
    pub(crate) async fn with_store<T, F>(&self, f: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&OrbitStore) -> T + Send + 'static,
    {
        with_store(&self.store, f).await
    }

    /// Offload a fallible synchronous `OrbitStore` closure onto Tokio's blocking pool,
    /// flattening both `JoinError` and the inner error into `anyhow::Result<T>`.
    pub(crate) async fn try_store<T, E, F>(&self, f: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        E: Into<anyhow::Error> + Send + 'static,
        F: FnOnce(&OrbitStore) -> Result<T, E> + Send + 'static,
    {
        try_store(&self.store, f).await
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

    pub(crate) fn home_dir(&self) -> &Path {
        &self.home_dir
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
        self.roster_states.write().await.remove(agent_id);
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
        if let Ok(sock) = self.shim_socket_path("a", agent_id) {
            kill_existing_shim(&sock).await;
        }
        kill_existing_shim(&self.legacy_shim_socket_path("agent", agent_id)).await;
        self.clear_pending_requests(agent_id).await;
        self.keep_awake.mark_agent_idle(agent_id);
        next_gen
    }

    fn claude_runtime_dir(&self) -> PathBuf {
        let dir = self.home_dir.join("runtime");
        let abs = if dir.is_absolute() {
            dir
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&dir))
                .unwrap_or(dir)
        };
        std::fs::canonicalize(&abs).unwrap_or(abs)
    }

    /// Activate live shim reattachment and startup agent recovery once the Daemon HTTP/WS listener
    /// is bound (must NOT be called from short-lived CLI read commands like `orbit pair`).
    pub async fn activate_runtime(self: &Arc<Self>) {
        // Snapshot pre-existing Claude & Pi MCP config files BEFORE recovery can spawn new sessions
        // so the startup reaper can never delete fresh files written by newly launching agents.
        let runtime_dir = self.claude_runtime_dir();
        let (claude_mcp_candidates, pi_mcp_candidates) = tokio::task::spawn_blocking(move || {
            (
                orbit_providers::snapshot_claude_mcp_config_files(&runtime_dir),
                orbit_providers::snapshot_pi_mcp_config_files(&runtime_dir),
            )
        })
        .await
        .unwrap_or_default();

        self.reattach_surviving_terminals().await;
        if let Some(protected_agent_ids) = self.reattach_or_recover_agents().await
            && (!claude_mcp_candidates.is_empty() || !pi_mcp_candidates.is_empty())
        {
            tokio::task::spawn_blocking(move || {
                let reaped_claude = orbit_providers::reap_snapshotted_claude_mcp_configs(
                    &claude_mcp_candidates,
                    &protected_agent_ids,
                );
                let reaped_pi = orbit_providers::reap_snapshotted_pi_mcp_configs(
                    &pi_mcp_candidates,
                    &protected_agent_ids,
                );
                let reaped = reaped_claude + reaped_pi;
                if reaped > 0 {
                    info!(
                        reaped,
                        reaped_claude, reaped_pi, "Reaped stale MCP config files on startup"
                    );
                }
            });
        }
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

    /// Resolve a Unix Domain Socket path guaranteed to fit within platform `sun_path` limits
    /// (104 bytes on Darwin, 108 bytes on Linux), falling back to a verified per-UID `0700` dir.
    fn shim_socket_path(&self, kind_prefix: &str, id: &str) -> std::io::Result<PathBuf> {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(id.as_bytes());
        let dig = h.finalize();
        let short_id: String = dig[..8].iter().map(|b| format!("{b:02x}")).collect();
        let filename = format!("{kind_prefix}_{short_id}.sock");

        let mut hh = Sha256::new();
        hh.update(self.home_dir.to_string_lossy().as_bytes());
        let hdig = hh.finalize();
        let home_tag: String = hdig[..4].iter().map(|b| format!("{b:02x}")).collect();

        orbit_shim::resolve_private_shim_socket_path(&self.home_dir, &filename, &home_tag)
    }

    fn legacy_shim_socket_path(&self, legacy_prefix: &str, id: &str) -> PathBuf {
        self.home_dir
            .join("shims")
            .join(format!("{legacy_prefix}_{id}.sock"))
    }

    /// Re-attach to any `orbit shim` PTY sockets that survived a Daemon restart.
    async fn reattach_surviving_terminals(self: &Arc<Self>) {
        let Ok(saved) = self.try_store(|s| s.list_terminals()).await else {
            return;
        };
        for term in saved {
            let primary = self.shim_socket_path("p", &term.terminal_id).ok();
            let legacy = self.legacy_shim_socket_path("pty", &term.terminal_id);
            let connected = match primary {
                Some(ref p) if p.exists() => connect_existing_shim(p).await,
                _ => None,
            };
            let connected = match connected {
                Some(c) => Some(c),
                None if legacy.exists() => connect_existing_shim(&legacy).await,
                None => None,
            };
            if let Some(shim) = connected {
                self.bind_terminal_shim(term, shim).await;
            } else {
                let tid = term.terminal_id.clone();
                let _ = self.try_store(move |s| s.delete_terminal(&tid)).await;
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
            .filter(|(_, seq)| *seq > 0)
            .collect();
        if entries.is_empty() {
            return;
        }
        let _ = self
            .with_store(move |s| {
                for (aid, seq) in entries {
                    let _ = s.set_agent_shim_cursor(&aid, seq);
                }
            })
            .await;
    }

    /// Re-attach to surviving agent shims across daemon hot-upgrades, and optionally
    /// resume previously `Running` agents if `orchestration.resume_on_startup` is enabled.
    /// Returns `Some(protected_agent_ids)` whose pre-snapshotted MCP configs must NOT be swept
    /// (live shims, transient reattach failures, archived agents with live/uncertain sockets,
    /// agents already registered in `live_agents`, or agents with uncertain socket/pid liveness),
    /// or `None` if reading the agent store failed (in which case startup reaping is skipped).
    async fn reattach_or_recover_agents(
        self: &Arc<Self>,
    ) -> Option<std::collections::HashSet<String>> {
        let mut protected_agents = std::collections::HashSet::new();
        let Ok(agents) = self
            .try_store(|s| s.list_agents_filtered(None, true, true, None))
            .await
        else {
            return None;
        };
        let cfg = self.orch_config.read().await.clone();

        for ag in agents {
            let primary = self.shim_socket_path("a", &ag.agent_id).ok();
            let legacy = self.legacy_shim_socket_path("agent", &ag.agent_id);
            let surviving_sock = if let Some(ref p) = primary
                && p.exists()
                && probe_existing_shim(p).await
            {
                Some(p.clone())
            } else if legacy.exists() && probe_existing_shim(&legacy).await {
                Some(legacy.clone())
            } else {
                None
            };
            // Protect any agent whose shim is live OR whose socket/pid liveness could not be
            // disproved (path error or non-refused timeout leaving socket or `.pid` file on disk).
            if primary.is_none()
                || surviving_sock.is_some()
                || primary
                    .as_ref()
                    .is_some_and(|p| p.exists() || orbit_shim::pid_file_path(p).exists())
                || legacy.exists()
                || orbit_shim::pid_file_path(&legacy).exists()
            {
                protected_agents.insert(ag.agent_id.clone());
            }
            if ag.archived_at.is_some() {
                continue;
            }
            let was_running = ag.status.is_foreground_busy();

            if surviving_sock.is_some()
                && let Ok((executable, _)) = self
                    .resolve_launch_executable(ag.provider, ag.acp_agent_id.as_deref())
                    .await
            {
                let aid = ag.agent_id.clone();
                let resume_ref = self
                    .try_store(move |s| s.get_persistence_ref(&aid))
                    .await
                    .ok()
                    .flatten();
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
            let should_close_status = matches!(
                ag.status,
                AgentStatus::Running { .. }
                    | AgentStatus::Initializing
                    | AgentStatus::AwaitingPermission { .. }
                    | AgentStatus::AutonomousWakeup { .. }
            );
            let aid_cleanup = ag.agent_id.clone();
            let _ = self
                .with_store(move |s| {
                    if let Ok(stale_bg) = s.list_running_background_tool_calls(&aid_cleanup)
                        && !stale_bg.is_empty()
                    {
                        let settlements: Vec<(String, ToolCallStatus)> = stale_bg
                            .into_iter()
                            .map(|(cid, _, _)| (cid, ToolCallStatus::Failed))
                            .collect();
                        let _ = s.settle_timeline_tool_calls_in_place(&aid_cleanup, &settlements);
                    }
                    if should_close_status {
                        let _ = s.update_agent_status(&aid_cleanup, &AgentStatus::Closed);
                    }
                })
                .await;
            if should_close_status {
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
        for live_id in self.live_agents.read().await.keys() {
            protected_agents.insert(live_id.clone());
        }
        Some(protected_agents)
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
        if !is_worktree && !spec.background && spec.allow_reuse_existing {
            let cwd_lookup = cwd_str.clone();
            if let Ok(Some(existing)) = self
                .try_store(move |s| s.find_workspace_by_cwd(&cwd_lookup))
                .await
            {
                return Ok((existing, None));
            }
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
        let ws_for_store = ws.clone();
        self.try_store(move |s| s.upsert_workspace(&ws_for_store))
            .await?;
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
        if let Some(watcher) = self.workspace_watcher.read().await.clone() {
            watcher.watch_workspace(&ws.workspace_id, Path::new(&ws.cwd));
        }

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
            let ws_id_bg = hook.workspace_id.clone();
            if let Ok(Some(updated_ws)) = with_store(&store_bg, move |s| {
                let _ = s.set_workspace_setup_status(&ws_id_bg, final_status);
                s.get_workspace(&ws_id_bg).ok().flatten()
            })
            .await
            {
                let _ = bcast_bg.send(DaemonBroadcast::WorkspaceUpdated(updated_ws));
            }
        });
    }

    pub async fn rename_workspace(
        &self,
        workspace_id: &str,
        title: &str,
    ) -> anyhow::Result<WorkspaceSummary> {
        let ws_id = workspace_id.to_string();
        let title_owned = title.to_string();
        let ws = self
            .try_store(move |s| -> anyhow::Result<WorkspaceSummary> {
                s.rename_workspace(&ws_id, &title_owned)?;
                s.get_workspace(&ws_id)?
                    .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id}` not found"))
            })
            .await?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceUpdated(ws.clone()));
        Ok(ws)
    }

    async fn resolve_handoff_source_and_target(
        &self,
        workspace_id: &str,
        source_agent_id: Option<&str>,
        direction: WorktreeHandoffDirection,
        target_workspace_id: Option<&str>,
    ) -> anyhow::Result<(
        WorkspaceSummary,
        Option<AgentSummary>,
        PathBuf,
        Option<PathBuf>,
    )> {
        let ws_id_owned = workspace_id.to_string();
        let src_aid_owned = source_agent_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let tgt_ws_id_owned = target_workspace_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let (source_ws, source_agent, explicit_target_ws) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let source_ws = s
                    .get_workspace(&ws_id_owned)?
                    .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id_owned}` not found"))?;
                let source_agent = match src_aid_owned {
                    Some(aid) => {
                        let ag = s
                            .get_agent(&aid)?
                            .ok_or_else(|| anyhow::anyhow!("agent `{aid}` not found"))?;
                        if ag.workspace_id != source_ws.workspace_id {
                            anyhow::bail!(
                                "agent `{}` belongs to workspace `{}`, not `{}`",
                                ag.agent_id,
                                ag.workspace_id,
                                source_ws.workspace_id
                            );
                        }
                        Some(ag)
                    }
                    None => None,
                };
                let target_ws = match tgt_ws_id_owned {
                    Some(tid) => Some(
                        s.get_workspace(&tid)?
                            .ok_or_else(|| anyhow::anyhow!("target workspace `{tid}` not found"))?,
                    ),
                    None => None,
                };
                Ok((source_ws, source_agent, target_ws))
            })
            .await?;

        let source_cwd = match source_agent.as_ref() {
            Some(ag) => crate::expand_host_path(&ag.cwd),
            None => crate::expand_host_path(&source_ws.cwd),
        };

        let target_cwd = if let Some(target_ws) = explicit_target_ws {
            Some(crate::expand_host_path(&target_ws.cwd))
        } else if direction == WorktreeHandoffDirection::WorktreeToLocal {
            let agent_wt_src = source_agent
                .as_ref()
                .and_then(|ag| ag.labels.get("orbit.worktree_source_cwd"))
                .map(String::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(crate::expand_host_path)
                .filter(|p| *p != source_cwd);
            if let Some(candidate) = agent_wt_src
                && tokio::fs::try_exists(&candidate).await.unwrap_or(false)
            {
                Some(candidate)
            } else if let Some(candidate) = source_ws
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

        Ok((source_ws, source_agent, source_cwd, target_cwd))
    }

    /// Preview a bidirectional Git Worktree <-> Local checkout handoff without mutating disk.
    pub async fn preview_worktree_handoff(
        &self,
        workspace_id: &str,
        direction: WorktreeHandoffDirection,
        target_workspace_id: Option<&str>,
        include_committed: bool,
    ) -> anyhow::Result<WorktreeHandoffPreview> {
        self.preview_worktree_handoff_with_agent(
            workspace_id,
            None,
            direction,
            target_workspace_id,
            include_committed,
        )
        .await
    }

    /// Preview a bidirectional Git Worktree <-> Local checkout handoff, optionally scoped to a
    /// specific sibling peer's isolated worktree inside a Mission Workspace.
    pub async fn preview_worktree_handoff_with_agent(
        &self,
        workspace_id: &str,
        source_agent_id: Option<&str>,
        direction: WorktreeHandoffDirection,
        target_workspace_id: Option<&str>,
        include_committed: bool,
    ) -> anyhow::Result<WorktreeHandoffPreview> {
        let (source_ws, _source_agent, source_cwd, target_cwd) = self
            .resolve_handoff_source_and_target(
                workspace_id,
                source_agent_id,
                direction,
                target_workspace_id,
            )
            .await?;

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
        let (source_ws, _source_agent, source_cwd, resolved_target_cwd) = self
            .resolve_handoff_source_and_target(
                &spec.workspace_id,
                spec.source_agent_id.as_deref(),
                spec.direction,
                spec.target_workspace_id.as_deref(),
            )
            .await?;

        // Serialize concurrent handoffs touching the same underlying Git repository so parallel
        // worktree peers merging back into local checkout never race on `.git` or partial patches.
        let repo_lock = self.acquire_repo_handoff_lock(&source_cwd).await;
        let _repo_guard = repo_lock.lock().await;

        match spec.direction {
            WorktreeHandoffDirection::WorktreeToLocal => {
                let target_cwd = match resolved_target_cwd {
                    Some(dir) => dir,
                    None => {
                        crate::worktree_handoff::resolve_primary_local_checkout(&source_cwd).await?
                    }
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
                    let tid_owned = tid.to_string();
                    let target_ws = self
                        .try_store(move |s| -> anyhow::Result<WorkspaceSummary> {
                            s.get_workspace(&tid_owned)?.ok_or_else(|| {
                                anyhow::anyhow!("target workspace `{tid_owned}` not found")
                            })
                        })
                        .await?;
                    let target_cwd = resolved_target_cwd
                        .unwrap_or_else(|| crate::expand_host_path(&target_ws.cwd));
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
        let ws_id_owned = workspace_id.to_string();
        let ws = self
            .try_store(move |s| -> anyhow::Result<WorkspaceSummary> {
                s.get_workspace(&ws_id_owned)?
                    .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id_owned}` not found"))
            })
            .await?;

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

        // 2. Either disband (move active conversations to unfiled inbox) or soft-archive all active agents
        let ws_id_agents = workspace_id.to_string();
        let agents = self
            .try_store(move |s| s.list_agents_filtered(Some(&ws_id_agents), false, true, None))
            .await?;
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

        // 3. Mark workspace archived & check if worktree cwd still has active workspace refs
        let ws_id_arch = workspace_id.to_string();
        let ws_cwd_check = ws.cwd.clone();
        let is_wt = ws.is_worktree;
        let (updated_ws_opt, active_cwd_refs) = self
            .try_store(
                move |s| -> anyhow::Result<(Option<WorkspaceSummary>, usize)> {
                    s.archive_workspace_soft(&ws_id_arch)?;
                    let updated = s.get_workspace(&ws_id_arch).ok().flatten();
                    let refs = if is_wt {
                        s.count_active_workspaces_for_cwd(&ws_cwd_check)
                            .unwrap_or(1)
                    } else {
                        1
                    };
                    Ok((updated, refs))
                },
            )
            .await?;
        if let Some(updated_ws) = updated_ws_opt {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceUpdated(updated_ws));
        }
        if let Some(watcher) = self.workspace_watcher.read().await.clone() {
            watcher.unwatch_workspace(workspace_id);
        }

        // 4. If managed worktree and NO other active workspace references its directory -> teardown & remove
        if ws.is_worktree && active_cwd_refs == 0 {
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

        let (ws_candidates, agent_candidates) = self
            .with_store(|s| {
                let mut ws_cands = std::collections::HashSet::new();
                for w in s.list_workspaces_filtered(false, true).unwrap_or_default() {
                    ws_cands.insert(w.cwd);
                    if let Some(src) = w.source_cwd {
                        ws_cands.insert(src);
                    }
                }
                let mut ag_cands = std::collections::HashSet::new();
                for a in s
                    .list_agents_filtered(None, false, true, None)
                    .unwrap_or_default()
                {
                    if let Some(src) = a.labels.get("orbit.worktree_source_cwd") {
                        ag_cands.insert(src.clone());
                    }
                }
                (ws_cands, ag_cands)
            })
            .await
            .unwrap_or_default();

        let mut has_active_ref = false;
        for candidate in ws_candidates {
            if self.resolve_canonical_repo_key(Path::new(&candidate)).await == target_key {
                has_active_ref = true;
                break;
            }
        }

        if !has_active_ref {
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
        let ws_id = workspace_id.to_string();
        let ws = self
            .try_store(move |s| -> anyhow::Result<WorkspaceSummary> {
                s.unarchive_workspace(&ws_id)?;
                s.get_workspace(&ws_id)?
                    .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id}` not found"))
            })
            .await?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceUpdated(ws.clone()));
        if let Some(watcher) = self.workspace_watcher.read().await.clone() {
            watcher.watch_workspace(&ws.workspace_id, Path::new(&ws.cwd));
        }
        Ok(ws)
    }

    // --- Workspace Scripts & Services ---

    pub async fn list_workspace_scripts(
        &self,
        workspace_id: &str,
    ) -> anyhow::Result<Vec<WorkspaceScriptInfo>> {
        let ws_id = workspace_id.to_string();
        let ws = self
            .try_store(move |s| -> anyhow::Result<WorkspaceSummary> {
                s.get_workspace(&ws_id)?
                    .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id}` not found"))
            })
            .await?;
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
        let ws_id = workspace_id.to_string();
        let ws = self
            .try_store(move |s| -> anyhow::Result<WorkspaceSummary> {
                s.get_workspace(&ws_id)?
                    .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id}` not found"))
            })
            .await?;
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

        let terminal_id = term.terminal_id;
        let info = WorkspaceScriptInfo {
            workspace_id: workspace_id.to_string(),
            name: script_name.to_string(),
            command: entry.command,
            kind,
            status: WorkspaceScriptStatus::Running,
            terminal_id: Some(terminal_id.clone()),
            port: assigned_port,
            proxy_url,
            last_error: None,
        };
        self.scripts
            .register_running(workspace_id, script_name, info.clone(), proxy_host)
            .await;
        if !self.live_terminals.read().await.contains_key(&terminal_id)
            && let Some(mut exited) = self.scripts.unregister(workspace_id, script_name).await
        {
            exited.info.status = WorkspaceScriptStatus::Stopped;
            exited.info.terminal_id = None;
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceScriptUpdated(exited.info.clone()));
            return Ok(exited.info);
        }
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
        let profile = match spec.profile_id.clone() {
            Some(pid) => {
                self.try_store(move |s| -> anyhow::Result<_> {
                    Ok(s.get_profile(&pid)?
                        .or_else(|| s.get_profile_by_name(&pid).ok().flatten()))
                })
                .await?
            }
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
        let parent_agent = match spec.parent_agent_id.clone() {
            Some(pid) if spec.parent_host_id.is_none() => self
                .try_store(move |s| s.get_agent(&pid))
                .await
                .ok()
                .flatten(),
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
            let ws_id_owned = ws_id.clone();
            let existing_ws = self
                .try_store(move |s| -> anyhow::Result<WorkspaceSummary> {
                    s.get_workspace(&ws_id_owned)?
                        .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id_owned}` not found"))
                })
                .await?;
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
                let ws_id_promote = existing_ws.workspace_id.clone();
                let promoted = self
                    .try_store(move |s| s.set_workspace_curated(&ws_id_promote, true))
                    .await?
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
        } else {
            let parent_ws_opt = if let Some(ref parent) = parent_agent
                && (spec.cwd.trim().is_empty() || spec.cwd == parent.cwd)
            {
                let pws_id = parent.workspace_id.clone();
                self.try_store(move |s| s.get_workspace(&pws_id))
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };
            if let Some(parent_ws) = parent_ws_opt {
                parent_ws
            } else {
                // Top-level local conversation: run two-stage Auto-Clustering when enabled
                let cfg_auto_cluster = self.orch_config.read().await.auto_cluster_enabled;
                let should_eval_cluster = !effective_background
                    && parent_agent.is_none()
                    && spec.auto_cluster.unwrap_or(cfg_auto_cluster)
                    && !human_seed.trim().is_empty();

                let git_branch_now = inspect_git_status(&expanded_cwd).await.branch;
                let (evaluated, auto_applied_ws) = if should_eval_cluster {
                    let project_root_cl = inferred_project_root.clone();
                    let cwd_cl = expanded_cwd_str.clone();
                    let seed_cl = human_seed.clone();
                    let badges_cl = spec.initial_badges.clone();
                    self.try_store(move |s| -> anyhow::Result<_> {
                        let mut evaluated = evaluate_auto_cluster(
                            s,
                            &project_root_cl,
                            &cwd_cl,
                            git_branch_now.as_deref(),
                            &seed_cl,
                            &badges_cl,
                            None,
                            true,
                        );
                        let applied_ws = if let Some(ref mut sug) = evaluated
                            && sug.auto_applied
                            && let Ok(Some(target_ws)) = s.get_workspace(&sug.target_workspace_id)
                        {
                            let _ = s.update_workspace_goal(
                                &target_ws.workspace_id,
                                None,
                                None,
                                None,
                                Some(true),
                            );
                            sug.previous_workspace_id = None;
                            Some(
                                s.get_workspace(&target_ws.workspace_id)?
                                    .unwrap_or(target_ws),
                            )
                        } else {
                            None
                        };
                        Ok((evaluated, applied_ws))
                    })
                    .await?
                } else {
                    (None, None)
                };

                if let Some(applied_ws) = auto_applied_ws {
                    cluster_suggestion = evaluated;
                    applied_ws
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
            let hook_outcome = Box::pin(self.hooks.dispatch_event(
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
            ))
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

        let ws_id_peers = ws.workspace_id.clone();
        let existing_peers = self
            .try_store(move |s| s.list_agents_filtered(Some(&ws_id_peers), false, false, None))
            .await
            .unwrap_or_default();
        if spec.handle.as_deref().is_some_and(|h| !h.trim().is_empty()) {
            agent_labels.insert("orbit.handle_locked".into(), "1".into());
        }
        if spec.title.as_deref().is_some_and(|t| !t.trim().is_empty()) {
            agent_labels.insert("orbit.title_locked".into(), "1".into());
        }
        let is_subagent = spec.parent_agent_id.is_some() || effective_background;
        let has_browser_binary = if crate::tools::prompt_mentions_browser(&human_seed) {
            crate::browser::discover_chrome_binary().is_some()
        } else {
            false
        };
        let detected_packs =
            crate::tools::detect_startup_mcp_packs(&human_seed, is_subagent, has_browser_binary);
        agent_labels.insert(
            crate::tools::LABEL_MCP_PACKS.into(),
            crate::tools::format_mcp_packs_label(&detected_packs),
        );
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
        let summary_insert = summary.clone();
        let extra_dirs_insert = extra_strs;
        let emit_peer_joined = !existing_peers.is_empty() && !summary.background;
        let ws_id_insert = ws.workspace_id.clone();
        let aid_insert = agent_id.clone();
        let handle_insert = allocated_handle.clone();
        let (peer_joined_ev, refreshed_ws_opt) = self
            .try_store(move |s| -> anyhow::Result<_> {
                s.upsert_agent(&summary_insert, None)?;
                if !extra_dirs_insert.is_empty() {
                    let _ = s.set_agent_extra_dirs(&aid_insert, &extra_dirs_insert);
                }
                let (ev, refreshed_ws) = if emit_peer_joined {
                    let ev = s
                        .append_workspace_event(
                            &ws_id_insert,
                            Some(&aid_insert),
                            Some(&handle_insert),
                            WorkspaceEventKind::PeerJoined,
                            &format!(
                                "@{} ({}) joined workspace",
                                handle_insert,
                                provider.display_name()
                            ),
                            None,
                        )
                        .ok();
                    let refreshed_ws = s.get_workspace(&ws_id_insert).ok().flatten();
                    (ev, refreshed_ws)
                } else {
                    (None, None)
                };
                Ok((ev, refreshed_ws))
            })
            .await?;
        self.keep_awake
            .notify_agent_status(&agent_id, &summary.status);
        if let Some(ws_ev) = peer_joined_ev {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceEventRecorded(ws_ev));
        }
        if let Some(refreshed_ws) = refreshed_ws_opt {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceUpdated(refreshed_ws));
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
        let (initial_turn, final_agent_opt) = if !initial_prompt.trim().is_empty() {
            let tid = format!("turn_{}", Uuid::new_v4().simple());
            let visible_text = spec
                .initial_display_prompt
                .unwrap_or_else(|| initial_prompt.clone());
            let aid_p = agent_id.clone();
            let tid_p = tid.clone();
            let badges_p = spec.initial_badges;
            let mark_read = summary.parent_agent_id.is_none();
            let (ev, final_ag) = self
                .try_store(move |s| -> anyhow::Result<_> {
                    let ev = s.append_timeline_event(
                        &aid_p,
                        1,
                        Some(&tid_p),
                        TimelinePayload::UserPrompt {
                            message_id: format!("u_{}", Uuid::new_v4().simple()),
                            text: visible_text,
                            badges: badges_p,
                        },
                    )?;
                    let prompt_seq = ev.seq;
                    // Authoritative multi-device sync: human's own prompt is already read by the creator.
                    if mark_read {
                        let _ = s.mark_agent_read_seq(&aid_p, Some(prompt_seq));
                    }
                    let final_ag = s.get_agent(&aid_p)?;
                    Ok((ev, final_ag))
                })
                .await?;
            let prompt_seq = ev.seq;
            let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            (Some((tid, prompt_seq)), final_ag)
        } else {
            let aid_p = agent_id.clone();
            let final_ag = self.try_store(move |s| s.get_agent(&aid_p)).await?;
            (None, final_ag)
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
                let aid_cp = ag_id_bg.clone();
                let tid_cp = turn_id.clone();
                let _ = mgr
                    .with_store(move |s| {
                        let _ =
                            s.save_turn_checkpoint(&aid_cp, prompt_seq, Some(&tid_cp), &tree_oid);
                    })
                    .await;
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
                                origin: PromptOrigin::Human,
                                message_ids: Vec::new(),
                            })
                            .await;
                    }
                }
                Err(e) => {
                    if mgr.current_generation(&ag_id_bg).await != launch_gen {
                        return;
                    }
                    mgr.keep_awake.mark_agent_idle(&ag_id_bg);
                    let aid_err = ag_id_bg.clone();
                    let err_msg = e.to_string();
                    if let Ok((ev_opt, ag_opt)) = mgr
                        .with_store(move |s| {
                            let ev = s
                                .append_timeline_event(
                                    &aid_err,
                                    1,
                                    None,
                                    TimelinePayload::StatusChanged {
                                        status: AgentStatus::Error {
                                            message: err_msg,
                                            recoverable: true,
                                        },
                                    },
                                )
                                .ok();
                            let _ = s.set_agent_attention(&aid_err, AttentionState::Error);
                            let ag = s.get_agent(&aid_err).ok().flatten();
                            (ev, ag)
                        })
                        .await
                    {
                        if let Some(ev) = ev_opt {
                            let _ = mgr.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
                        }
                        if let Some(ag) = ag_opt {
                            let _ = mgr.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));
                        }
                    }
                }
            }
        });

        Ok(final_agent_opt.unwrap_or(summary))
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

        let src_aid = spec.source_agent_id.clone();
        let (source, source_ws) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let source = s
                    .get_agent(&src_aid)?
                    .ok_or_else(|| anyhow::anyhow!("source agent `{src_aid}` not found"))?;
                let source_ws = s.get_workspace(&source.workspace_id)?;
                Ok((source, source_ws))
            })
            .await?;

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

        let (
            target_workspace_id,
            target_cwd,
            created_workspace,
            shared_curated_workspace,
            peer_wt_meta,
        ) = match spec.target {
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
                        None,
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
                        None,
                    )
                }
            }
            ForkTarget::NewWorktree { branch, base_ref } => {
                let src_for_wt = source
                    .labels
                    .get("orbit.worktree_source_cwd")
                    .map(String::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        source_ws
                            .as_ref()
                            .and_then(|w| w.source_cwd.as_deref())
                            .filter(|s| !s.trim().is_empty())
                    })
                    .unwrap_or(&source.cwd);
                let expanded_src = crate::expand_host_path(src_for_wt);
                if resolve_git_scope(&expanded_src).await.is_none() {
                    anyhow::bail!(
                        "not_a_git_repo: `{}` is not a Git repository. Fork in the same directory instead.",
                        expanded_src.display()
                    );
                }
                let branch_name = match branch.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
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
                            let peer_wt_branch = source
                                .labels
                                .get("orbit.worktree_branch")
                                .map(String::as_str)
                                .filter(|s| !s.trim().is_empty());
                            let ws_wt_branch = source_ws
                                .as_ref()
                                .filter(|w| w.is_worktree)
                                .and_then(|w| w.git_branch.as_deref())
                                .filter(|s| !s.trim().is_empty());
                            if let Some(wt_branch) = peer_wt_branch.or(ws_wt_branch) {
                                validate_git_base_ref(&expanded_src, wt_branch).await.ok()
                            } else {
                                None
                            }
                        }
                    };
                if source_ws.as_ref().is_some_and(|w| w.is_curated) {
                    // Inside a curated Mission Workspace, keep the forked peer in the SAME
                    // workspace while isolating its checkout onto an owned git worktree so
                    // sibling peers remain visible and can coordinate/merge via `handoff_worktree`.
                    let (wt_path, created_br) = create_worktree_with_mode_pooled(
                        &self.home_dir,
                        &expanded_src,
                        WorktreeCreateMode::BranchOff,
                        Some(&branch_name),
                        effective_base_ref.as_deref(),
                        None,
                        Some(&self.worktree_pool),
                    )
                    .await?;
                    let resolved_branch = created_br.or(Some(branch_name));
                    (
                        source.workspace_id.clone(),
                        wt_path.to_string_lossy().to_string(),
                        None,
                        true,
                        Some((expanded_src, resolved_branch)),
                    )
                } else {
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
                        None,
                    )
                }
            }
        };

        let now = Utc::now();
        let new_agent_id = format!("ag_{}", Uuid::new_v4().simple());
        let tgt_ws_peers = target_workspace_id.clone();
        let existing_peers = self
            .try_store(move |s| s.list_agents_filtered(Some(&tgt_ws_peers), false, false, None))
            .await
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
        if let Some(packs) = source.labels.get(crate::tools::LABEL_MCP_PACKS) {
            labels.insert(crate::tools::LABEL_MCP_PACKS.into(), packs.clone());
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
        } else if let Some((ref src, ref br)) = peer_wt_meta {
            labels.insert(
                "orbit.worktree_source_cwd".into(),
                src.to_string_lossy().to_string(),
            );
            if let Some(branch) = br {
                labels.insert("orbit.worktree_branch".into(), branch.clone());
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

        // Run all fallible DB writes inside a single `spawn_blocking` closure so any failure
        // cleanly rolls back both the inserted agent row AND any newly provisioned workspace/worktree!
        let setup_res: anyhow::Result<ForkDbSetup> = {
            let initial_summary_db = initial_summary.clone();
            let source_id_db = source.agent_id.clone();
            let new_agent_id_db = new_agent_id.clone();
            let up_to_seq_db = spec.up_to_seq;
            let initial_prompt_db = initial_prompt_text.clone();
            self.try_store(move |s| -> anyhow::Result<ForkDbSetup> {
                s.upsert_agent(&initial_summary_db, None)?;

                let source_extra_dirs = s.get_agent_extra_dirs(&source_id_db).unwrap_or_default();
                if !source_extra_dirs.is_empty() {
                    let _ = s.set_agent_extra_dirs(&new_agent_id_db, &source_extra_dirs);
                }

                let cloned =
                    s.clone_timeline_events_up_to(&source_id_db, &new_agent_id_db, up_to_seq_db)?;

                // Build handoff context STRICTLY from the newly cloned timeline (never the source's tail!).
                let handoff_context = Self::build_handoff_context_from_store(s, &new_agent_id_db);

                // Eliminate TOCTOU leak: native upstream session fork is ONLY eligible when the
                // atomic transaction cloned the true full tail AND the re-fetched source agent is
                // not currently foreground-busy.
                let source_now = s.get_agent(&source_id_db)?;
                let can_native_fork = cloned.cloned_full_tail
                    && source_now
                        .as_ref()
                        .is_some_and(|ag| !ag.status.is_foreground_busy());
                let fork_from_session_id = if can_native_fork {
                    s.get_persistence_ref(&source_id_db)?
                        .filter(|p| !p.trim().is_empty())
                } else {
                    None
                };

                let prompt_event = if let Some(ref prompt_text) = initial_prompt_db {
                    let turn_id = format!("turn_{}", Uuid::new_v4().simple());
                    let msg_id = format!("u_{}", Uuid::new_v4().simple());
                    let ev = s.append_timeline_event(
                        &new_agent_id_db,
                        1,
                        Some(&turn_id),
                        TimelinePayload::UserPrompt {
                            message_id: msg_id.clone(),
                            text: prompt_text.clone(),
                            badges: Vec::new(),
                        },
                    )?;
                    let prompt_seq = ev.seq;
                    let _ = s.mark_agent_read_seq(&new_agent_id_db, Some(prompt_seq));
                    Some(ForkInitialPromptEvent {
                        turn_id,
                        msg_id,
                        prompt_seq,
                        event: ev,
                    })
                } else {
                    None
                };

                let final_summary = s
                    .get_agent(&new_agent_id_db)?
                    .unwrap_or_else(|| initial_summary_db.clone());

                Ok(ForkDbSetup {
                    final_summary,
                    handoff_context,
                    fork_from_session_id,
                    prompt_event,
                })
            })
            .await
        };

        let ForkDbSetup {
            final_summary,
            handoff_context,
            fork_from_session_id,
            prompt_event,
        } = match setup_res {
            Ok(v) => v,
            Err(err) => {
                let del_id = new_agent_id.clone();
                let _ = self.try_store(move |s| s.delete_agent(&del_id)).await;
                if let Some(ref ws) = created_workspace {
                    let _ = self.archive_workspace(&ws.workspace_id).await;
                } else if let Some((ref src, ref br)) = peer_wt_meta {
                    teardown_and_remove_worktree(Some(src), Path::new(&target_cwd), br.as_deref())
                        .await;
                }
                return Err(err);
            }
        };

        // Emit PeerJoined when joining a curated multi-agent Mission Workspace
        if shared_curated_workspace && !final_summary.background {
            let joined_msg = if let Some((_, Some(ref br))) = peer_wt_meta {
                format!(
                    "@{} ({}) forked into isolated worktree (`{br}`) inside workspace",
                    allocated_handle,
                    final_summary.provider.display_name()
                )
            } else {
                format!(
                    "@{} ({}) forked into workspace",
                    allocated_handle,
                    final_summary.provider.display_name()
                )
            };
            let tgt_ws_ev = target_workspace_id.clone();
            let new_ag_ev = new_agent_id.clone();
            let handle_ev = allocated_handle.clone();
            if let Ok((ws_ev_opt, refreshed_ws_opt)) = self
                .with_store(move |s| {
                    let ev = s
                        .append_workspace_event(
                            &tgt_ws_ev,
                            Some(&new_ag_ev),
                            Some(&handle_ev),
                            WorkspaceEventKind::PeerJoined,
                            &joined_msg,
                            None,
                        )
                        .ok();
                    let refreshed = s.get_workspace(&tgt_ws_ev).ok().flatten();
                    (ev, refreshed)
                })
                .await
            {
                if let Some(ws_ev) = ws_ev_opt {
                    let _ = self
                        .broadcast_tx
                        .send(DaemonBroadcast::WorkspaceEventRecorded(ws_ev));
                }
                if let Some(refreshed_ws) = refreshed_ws_opt {
                    let _ = self
                        .broadcast_tx
                        .send(DaemonBroadcast::WorkspaceUpdated(refreshed_ws));
                }
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
                    let aid_cp = ag_id_bg.clone();
                    let tid_cp = turn_id.clone();
                    let _ = mgr
                        .with_store(move |s| {
                            let _ = s.save_turn_checkpoint(
                                &aid_cp,
                                prompt_seq,
                                Some(&tid_cp),
                                &tree_oid,
                            );
                        })
                        .await;
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
                                origin: PromptOrigin::Human,
                                message_ids: Vec::new(),
                            })
                            .await;
                    }
                    Err(e) => {
                        if mgr.current_generation(&ag_id_bg).await != launch_gen {
                            return;
                        }
                        mgr.keep_awake.mark_agent_idle(&ag_id_bg);
                        let aid_err = ag_id_bg.clone();
                        let err_msg = e.to_string();
                        if let Ok((ev_opt, ag_opt)) = mgr
                            .with_store(move |s| {
                                let ev = s
                                    .append_timeline_event(
                                        &aid_err,
                                        1,
                                        None,
                                        TimelinePayload::StatusChanged {
                                            status: AgentStatus::Error {
                                                message: err_msg,
                                                recoverable: true,
                                            },
                                        },
                                    )
                                    .ok();
                                let _ = s.set_agent_attention(&aid_err, AttentionState::Error);
                                let ag = s.get_agent(&aid_err).ok().flatten();
                                (ev, ag)
                            })
                            .await
                        {
                            if let Some(ev) = ev_opt {
                                let _ = mgr.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
                            }
                            if let Some(ag) = ag_opt {
                                let _ = mgr.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));
                            }
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

    async fn run_pre_prompt_submit_hook(
        &self,
        agent_id: &str,
        existing_agent: Option<&AgentSummary>,
        mut prompt: String,
    ) -> anyhow::Result<String> {
        if let Some(ag) = existing_agent {
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
        Ok(prompt)
    }

    pub async fn send_prompt_with_options(
        self: &Arc<Self>,
        agent_id: &str,
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
        behavior: ActiveTurnBehavior,
        message_id: Option<String>,
    ) -> anyhow::Result<()> {
        let aid = agent_id.to_string();
        let existing_agent = self
            .try_store(move |s| s.get_agent(&aid))
            .await
            .ok()
            .flatten();
        let checked_prompt = self
            .run_pre_prompt_submit_hook(agent_id, existing_agent.as_ref(), prompt)
            .await?;
        self.send_prompt_with_options_after_hook(
            agent_id,
            checked_prompt,
            display_prompt,
            badges,
            behavior,
            message_id,
            existing_agent,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_prompt_with_options_after_hook(
        self: &Arc<Self>,
        agent_id: &str,
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
        behavior: ActiveTurnBehavior,
        message_id: Option<String>,
        existing_agent: Option<AgentSummary>,
    ) -> anyhow::Result<()> {
        // Sending a human prompt acknowledges unread attention and unarchives if needed
        if let Some(ref ag) = existing_agent
            && ag.archived_at.is_some()
        {
            let _ = self.unarchive_agent(agent_id).await;
        }
        let is_live = self.live_agents.read().await.contains_key(agent_id);
        let aid_db = agent_id.to_string();
        let handoff_context = self
            .try_store(move |s| -> anyhow::Result<Option<String>> {
                let _ = s.set_agent_attention(&aid_db, AttentionState::None);
                let _ = s.touch_agent_activity(&aid_db, Utc::now());
                let needs_handoff = !is_live && s.get_persistence_ref(&aid_db)?.is_none();
                Ok(if needs_handoff {
                    Self::build_handoff_context_from_store(s, &aid_db)
                } else {
                    None
                })
            })
            .await?;
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
        let aid = agent_id.to_string();
        let ag = self
            .try_store(move |s| -> anyhow::Result<AgentSummary> {
                s.get_agent(&aid)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid}` not found"))
            })
            .await?;
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
        let aid_enq = agent_id.to_string();
        let (saved, store_still_busy) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let saved = s.enqueue_agent_prompt(&aid_enq, &qid, &prompt, &visible, &badges)?;
                let busy = s.get_agent(&aid_enq)?.is_some_and(|cur| {
                    cur.status.is_foreground_busy()
                        || matches!(cur.status, AgentStatus::AwaitingPermission { .. })
                });
                Ok((saved, busy))
            })
            .await?;

        // Re-check status after persisting to close the lost-wakeup race where the agent
        // became foreground-idle between our initial check and SQLite insert.
        let still_busy = store_still_busy || self.has_pending_permissions(agent_id).await;
        if !still_busy {
            let aid_take = agent_id.to_string();
            let qid_take = saved.queue_id.clone();
            let (taken, updated_opt) = self
                .try_store(move |s| -> anyhow::Result<_> {
                    let item = s.take_queued_prompt(&aid_take, &qid_take)?;
                    let updated = s.get_agent(&aid_take).ok().flatten();
                    Ok((item, updated))
                })
                .await?;
            if let Some(updated) = updated_opt {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::AgentUpdated(updated));
            }
            if let Some(item) = taken {
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
            return Ok(());
        }

        let aid_upd = agent_id.to_string();
        if let Ok(Some(updated)) = self.try_store(move |s| s.get_agent(&aid_upd)).await {
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
        let aid = agent_id.to_string();
        let qid = queue_id.to_string();
        let updated_opt = self
            .try_store(move |s| -> anyhow::Result<Option<AgentSummary>> {
                let removed = s.remove_queued_prompt(&aid, &qid)?;
                Ok(if removed {
                    s.get_agent(&aid).ok().flatten()
                } else {
                    None
                })
            })
            .await?;
        if let Some(updated) = updated_opt {
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
        // Peek first and preflight `PrePromptSubmit` hooks BEFORE consuming the queue entry or
        // cancelling pending permissions/questions so a blocked hook leaves state intact.
        let aid_peek = agent_id.to_string();
        let qid_peek = queue_id.to_string();
        let (peeked_opt, existing_agent) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let peeked = s.peek_queued_prompt(&aid_peek, &qid_peek)?;
                let ag = s.get_agent(&aid_peek).ok().flatten();
                Ok((peeked, ag))
            })
            .await?;
        let Some(peeked) = peeked_opt else {
            return Ok(());
        };
        let checked_prompt = self
            .run_pre_prompt_submit_hook(agent_id, existing_agent.as_ref(), peeked.wire_prompt)
            .await?;

        let aid_take = agent_id.to_string();
        let qid_take = queue_id.to_string();
        let (item_opt, updated_opt) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let item = s.take_queued_prompt(&aid_take, &qid_take)?;
                let updated = if item.is_some() {
                    s.get_agent(&aid_take).ok().flatten()
                } else {
                    None
                };
                Ok((item, updated))
            })
            .await?;
        let Some(item) = item_opt else {
            return Ok(());
        };
        let store_busy = updated_opt.as_ref().is_some_and(|ag| {
            ag.status.is_foreground_busy()
                || matches!(ag.status, AgentStatus::AwaitingPermission { .. })
        });
        if let Some(updated) = updated_opt {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        let should_interrupt = store_busy || self.has_pending_permissions(agent_id).await;
        // Never send a separate `DriverCommand::Interrupt` before `send_prompt_with_options`:
        // doing so lets `session/cancel` settle while prompt pre-processing is still awaiting git
        // snapshots, letting stale queued prompts or trailing turn finalization race ahead of the
        // new `UserPrompt`. Instead, pass `ActiveTurnBehavior::Interrupt` directly on `SendPrompt`
        // so the driver atomically cancels the active turn and dispatches this replacement prompt.
        if should_interrupt {
            self.suppress_auto_drain
                .write()
                .await
                .insert(agent_id.to_string());
            self.cancel_pending_permissions_and_questions(agent_id)
                .await;
        }
        let behavior = if should_interrupt {
            ActiveTurnBehavior::Interrupt
        } else {
            ActiveTurnBehavior::Steer
        };
        let res = self
            .send_prompt_with_options_after_hook(
                agent_id,
                checked_prompt,
                Some(item.display_prompt),
                item.badges,
                behavior,
                None,
                existing_agent,
            )
            .await;
        if res.is_err() && should_interrupt {
            self.suppress_auto_drain.write().await.remove(agent_id);
        }
        res
    }

    pub async fn reorder_queued_user_prompts(
        &self,
        agent_id: &str,
        queue_ids: &[String],
    ) -> anyhow::Result<()> {
        let aid = agent_id.to_string();
        let qids = queue_ids.to_vec();
        let updated_opt = self
            .try_store(move |s| -> anyhow::Result<Option<AgentSummary>> {
                let changed = s.reorder_queued_prompts(&aid, &qids)?;
                Ok(if changed {
                    s.get_agent(&aid).ok().flatten()
                } else {
                    None
                })
            })
            .await?;
        if let Some(updated) = updated_opt {
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
        let recipient_lookup = batch.recipient_id.clone();
        let ag = self
            .try_store(move |s| -> anyhow::Result<AgentSummary> {
                s.get_agent(&recipient_lookup)?.ok_or_else(|| {
                    anyhow::anyhow!("recipient agent `{recipient_lookup}` not found")
                })
            })
            .await?;
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

        let recipient_touch = batch.recipient_id.clone();
        let _ = self
            .with_store(move |s| {
                let _ = s.touch_agent_activity(&recipient_touch, Utc::now());
            })
            .await;
        let cmd_tx = self.get_or_resume_driver(&batch.recipient_id).await?;
        let turn_id = format!("turn_{}", Uuid::new_v4().simple());

        // Pre-compute workspace context augmentation BEFORE persisting `AgentMessage` events so
        // there is no async gap between timeline persistence and driver dispatch.
        let wire_prompt = self
            .augment_prompt_with_workspace_context(&batch.recipient_id, batch.wire_prompt)
            .await;

        // Persist structured `TimelinePayload::AgentMessage` cards for each entry in the coalesced batch
        if let Some(timers) = self.conditional_timers().await {
            for entry in &batch.entries {
                if matches!(
                    entry.source.kind,
                    orbit_protocol::AgentMessageKind::DirectMessage
                        | orbit_protocol::AgentMessageKind::FinishNotification
                ) {
                    timers
                        .notify_agent_message(
                            &batch.recipient_id,
                            entry.source.source_agent_id.as_deref(),
                        )
                        .await;
                }
            }
        }
        let blocking_turn_id = match ag.status {
            AgentStatus::Running { turn_id: ref tid }
            | AgentStatus::AwaitingPermission {
                turn_id: ref tid, ..
            } => Some(tid.clone()),
            _ => None,
        };
        let queued_at_ms = Utc::now().timestamp_millis();
        let message_ids: Vec<String> = batch
            .entries
            .iter()
            .map(|entry| entry.message_id.clone())
            .collect();
        {
            let mut qmap = self.notification_queued_at.write().await;
            for msg_id in &message_ids {
                if qmap.len() >= 4096
                    && !qmap.contains_key(msg_id)
                    && let Some(oldest_k) = qmap
                        .iter()
                        .min_by_key(|(_, ts)| *ts)
                        .map(|(k, _)| k.clone())
                {
                    qmap.remove(&oldest_k);
                }
                qmap.insert(msg_id.clone(), queued_at_ms);
            }
        }
        let payloads: Vec<TimelinePayload> = batch
            .entries
            .iter()
            .map(|entry| TimelinePayload::AgentMessage {
                message_id: entry.message_id.clone(),
                kind: entry.source.kind,
                source_agent_id: entry.source.source_agent_id.clone(),
                source_host_id: entry.source.source_host_id.clone(),
                source_title: entry.source.source_title.clone(),
                event: entry.source.event.clone(),
                text: entry.body.clone(),
                permission_context: entry.source.permission_context.clone(),
                delivery_status: Some(AgentMessageDeliveryStatus::Queued),
                delivery_turn_id: Some(turn_id.clone()),
                blocking_turn_id: blocking_turn_id.clone(),
                queued_at_ms: Some(queued_at_ms),
                dispatched_at_ms: None,
                settled_at_ms: None,
            })
            .collect();
        let rec_append = batch.recipient_id.clone();
        let tid_append = turn_id.clone();
        let saved_evs = self
            .try_store(move |s| -> anyhow::Result<Vec<TimelineEvent>> {
                let mut out = Vec::with_capacity(payloads.len());
                for payload in payloads {
                    out.push(s.append_timeline_event(
                        &rec_append,
                        1,
                        Some(&tid_append),
                        payload,
                    )?);
                }
                Ok(out)
            })
            .await?;
        for ev in saved_evs {
            let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
        }

        if cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: turn_id.clone(),
                prompt: wire_prompt,
                behavior: batch.active_turn_behavior,
                origin: PromptOrigin::Agent,
                message_ids: message_ids.clone(),
            })
            .await
            .is_err()
        {
            let drop_ts = Utc::now().timestamp_millis();
            {
                let mut qmap = self.notification_queued_at.write().await;
                for msg_id in &message_ids {
                    qmap.remove(msg_id);
                }
            }
            let rec_drop = batch.recipient_id.clone();
            let tid_drop = turn_id.clone();
            if let Ok(dropped_evs) = self
                .with_store(move |s| {
                    let mut out = Vec::new();
                    for msg_id in message_ids {
                        if let Ok((saved_ev, mutated)) = s.append_timeline_event_deduped(
                            &rec_drop,
                            1,
                            Some(&tid_drop),
                            TimelinePayload::AgentMessageDeliveryUpdated {
                                message_id: msg_id,
                                status: AgentMessageDeliveryStatus::Dropped,
                                delivery_turn_id: Some(tid_drop.clone()),
                                blocking_turn_id: None,
                                timestamp_ms: drop_ts,
                            },
                        ) && mutated
                        {
                            out.push(saved_ev);
                        }
                    }
                    out
                })
                .await
            {
                for saved_ev in dropped_evs {
                    let _ = self
                        .broadcast_tx
                        .send(DaemonBroadcast::TimelineLive(saved_ev));
                }
            }
            anyhow::bail!("agent session channel closed");
        }
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
        if let Some(timers) = self.conditional_timers().await {
            timers.notify_agent_message(agent_id, Some("user")).await;
        }
        let turn_id = format!("turn_{}", Uuid::new_v4().simple());
        let msg_id = message_id.unwrap_or_else(|| format!("u_{}", Uuid::new_v4().simple()));
        let visible_text = display_prompt.unwrap_or_else(|| prompt.clone());
        let sanitized = sanitize_human_prompt_for_wire(&prompt, &msg_id);

        // Compute git checkpoint snapshot and prompt augmentations BEFORE appending `UserPrompt`
        // so there is ZERO async `.await` gap between persisting `UserPrompt` and sending
        // `DriverCommand::SendPrompt` to the driver.
        let aid_cwd = agent_id.to_string();
        let ag_cwd = self
            .try_store(move |s| s.get_agent(&aid_cwd))
            .await
            .ok()
            .flatten()
            .map(|ag| ag.cwd);
        let tree_oid = if let Some(cwd_str) = ag_cwd {
            capture_git_tree_snapshot(Path::new(&cwd_str)).await
        } else {
            None
        };
        let with_handoff = match handoff_context {
            Some(ctx) => format!("{ctx}\n\n[Latest User Request]\n{sanitized}"),
            None => sanitized.clone(),
        };
        let with_rules = self
            .augment_prompt_with_path_rules(agent_id, &badges, &sanitized, with_handoff)
            .await;
        let wire_prompt = self
            .augment_prompt_with_workspace_context(agent_id, with_rules)
            .await;

        let aid_persist = agent_id.to_string();
        let tid_persist = turn_id.clone();
        let (ev, read_ag_opt) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let ev = s.append_timeline_event(
                    &aid_persist,
                    1,
                    Some(&tid_persist),
                    TimelinePayload::UserPrompt {
                        message_id: msg_id,
                        text: visible_text,
                        badges,
                    },
                )?;
                let prompt_seq = ev.seq;
                if let Some(ref oid) = tree_oid {
                    let _ =
                        s.save_turn_checkpoint(&aid_persist, prompt_seq, Some(&tid_persist), oid);
                }
                // Advance authoritative read_seq on human prompt so other devices never flash unread for user input
                let read_ag = s
                    .mark_agent_read_seq(&aid_persist, Some(prompt_seq))
                    .ok()
                    .flatten();
                Ok((ev, read_ag))
            })
            .await?;
        if let Some(read_ag) = read_ag_opt {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(read_ag));
        }
        let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id,
                prompt: wire_prompt,
                behavior,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .map_err(|_| anyhow::anyhow!("agent session channel closed"))?;
        Ok(())
    }

    fn build_handoff_context_from_store(store: &OrbitStore, agent_id: &str) -> Option<String> {
        let events = store.fetch_recent_timeline(agent_id, 200).ok()?;
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

    #[cfg(test)]
    fn build_handoff_context(&self, agent_id: &str) -> Option<String> {
        Self::build_handoff_context_from_store(&self.store, agent_id)
    }

    async fn cancel_pending_permissions_and_questions(&self, agent_id: &str) {
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
        let cancelled_questions = self
            .pending_questions
            .write()
            .await
            .remove(agent_id)
            .unwrap_or_default();
        if cancelled_perms.is_empty() && cancelled_questions.is_empty() {
            return;
        }
        let aid = agent_id.to_string();
        if let Ok(evs) = self
            .with_store(move |s| {
                let mut out = Vec::new();
                for request_id in cancelled_perms {
                    if let Ok(ev) = s.append_timeline_event(
                        &aid,
                        1,
                        None,
                        TimelinePayload::PermissionResolved {
                            request_id,
                            decision: "cancelled".into(),
                        },
                    ) {
                        out.push(ev);
                    }
                }
                for request_id in cancelled_questions {
                    if let Ok(ev) = s.append_timeline_event(
                        &aid,
                        1,
                        None,
                        TimelinePayload::QuestionResolved {
                            request_id,
                            answers: Vec::new(),
                        },
                    ) {
                        out.push(ev);
                    }
                }
                out
            })
            .await
        {
            for ev in evs {
                let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            }
        }
    }

    pub async fn interrupt_agent(&self, agent_id: &str) -> anyhow::Result<()> {
        self.interrupt_agent_with_options(agent_id, false).await
    }

    /// Interrupt the active turn on `agent_id`.
    /// When `preserve_queue` is `true`, accepted FIFO notifications and queued follow-ups stay
    /// intact and drain immediately after settlement (`Interrupt & continue`).
    pub async fn interrupt_agent_with_options(
        &self,
        agent_id: &str,
        preserve_queue: bool,
    ) -> anyhow::Result<()> {
        if preserve_queue {
            self.suppress_auto_drain.write().await.remove(agent_id);
        } else {
            self.suppress_auto_drain
                .write()
                .await
                .insert(agent_id.to_string());
        }
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let cmd = if preserve_queue {
                DriverCommand::InterruptAndDrainQueue
            } else {
                DriverCommand::Interrupt
            };
            let _ = tx.send(cmd).await;
        }
        self.cancel_pending_permissions_and_questions(agent_id)
            .await;
        Ok(())
    }

    pub async fn set_session_mode(&self, agent_id: &str, mode: SessionMode) -> anyhow::Result<()> {
        let aid = agent_id.to_string();
        let mode_db = mode.clone();
        self.try_store(move |s| s.update_agent_mode(&aid, &mode_db))
            .await?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx.send(DriverCommand::SetMode { mode }).await;
        }
        let aid_get = agent_id.to_string();
        if let Ok(Some(updated)) = self.try_store(move |s| s.get_agent(&aid_get)).await {
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
        let aid = agent_id.to_string();
        let existing_opt = self.try_store(move |s| s.get_agent(&aid)).await?;
        let clear_fast = if let Some(existing) = existing_opt {
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
        let aid_upd = agent_id.to_string();
        let model_db = model.clone();
        self.try_store(move |s| {
            s.update_agent_config(&aid_upd, model_db.as_deref(), effort, clear_fast)
        })
        .await?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx
                .send(DriverCommand::SetConfig {
                    model,
                    effort,
                    fast_mode: clear_fast,
                })
                .await;
        }
        let aid_get = agent_id.to_string();
        if let Ok(Some(updated)) = self.try_store(move |s| s.get_agent(&aid_get)).await {
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
        let aid = agent_id.to_string();
        let existing_opt = self.try_store(move |s| s.get_agent(&aid)).await?;
        let clear_fast = if let Some(existing) = existing_opt {
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
        let aid_upd = agent_id.to_string();
        let model_db = model.clone();
        self.try_store(move |s| s.update_agent_model(&aid_upd, model_db.as_deref(), clear_fast))
            .await?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx
                .send(DriverCommand::SetConfig {
                    model,
                    effort: None,
                    fast_mode: clear_fast,
                })
                .await;
        }
        let aid_get = agent_id.to_string();
        if let Ok(Some(updated)) = self.try_store(move |s| s.get_agent(&aid_get)).await {
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
        let aid = agent_id.to_string();
        let existing = self
            .try_store(move |s| -> anyhow::Result<AgentSummary> {
                s.get_agent(&aid)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid}` not found"))
            })
            .await?;
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
        let aid_upd = agent_id.to_string();
        let model_db = model.clone();
        let mode_db = mode.clone();
        self.try_store(move |s| {
            s.update_agent_fields(
                &aid_upd,
                title.as_deref(),
                model_db.as_deref().map(Some),
                effort.map(Some),
                mode_db.as_ref(),
                effective_labels.as_ref(),
                clean_handle.as_deref(),
                task_summary.as_deref(),
                effective_fast_mode,
            )
        })
        .await?;
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
        let aid_get = agent_id.to_string();
        let updated = self
            .try_store(move |s| -> anyhow::Result<AgentSummary> {
                s.get_agent(&aid_get)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid_get}` not found"))
            })
            .await?;
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

        let aid_prev = agent_id.to_string();
        let prev_fast_mode = self
            .try_store(move |s| s.get_agent(&aid_prev))
            .await?
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
        let switched_label = acp_spec
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| provider.display_name().to_string());
        let aid_sw = agent_id.to_string();
        let (ev_opt, updated_opt) = self
            .try_store(move |s| -> anyhow::Result<_> {
                s.switch_agent_provider(
                    &aid_sw,
                    provider,
                    model.as_deref(),
                    effort,
                    next_fast_mode,
                    acp_agent_id.as_deref(),
                )?;
                let ev = s
                    .append_timeline_event(
                        &aid_sw,
                        1,
                        None,
                        TimelinePayload::Notice {
                            level: "info".into(),
                            message: format!(
                                "Switched to {switched_label} to continue this conversation"
                            ),
                        },
                    )
                    .ok();
                let updated = s.get_agent(&aid_sw).ok().flatten();
                Ok((ev, updated))
            })
            .await?;
        if let Some(ev) = ev_opt {
            let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
        }
        if let Some(updated) = updated_opt {
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
        self.guardian_auto_resolved_requests
            .write()
            .await
            .retain(|(aid, _)| aid != agent_id);
        self.pending_questions.write().await.remove(agent_id);
    }

    pub async fn resolve_permission(
        self: &Arc<Self>,
        agent_id: &str,
        request_id: String,
        decision: String,
    ) -> anyhow::Result<()> {
        self.resolve_permission_inner(agent_id, request_id, decision, true)
            .await
    }

    /// Agent/Router-originated permission resolution path: strictly rejects human-only
    /// plan approval gates (`plan_approval.is_some()`) and fail-closed/human-only permissions
    /// (`human_only == true`).
    pub async fn resolve_permission_by_agent(
        self: &Arc<Self>,
        agent_id: &str,
        request_id: String,
        decision: String,
    ) -> anyhow::Result<()> {
        self.resolve_permission_inner(agent_id, request_id, decision, false)
            .await
    }

    async fn resolve_permission_inner(
        self: &Arc<Self>,
        agent_id: &str,
        request_id: String,
        decision: String,
        allow_human_only_resolution: bool,
    ) -> anyhow::Result<()> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum CanonicalDecision {
            Allow,
            AllowAlways,
            Deny,
        }

        let key = (agent_id.to_string(), request_id.clone());
        let peeked = {
            let guard = self.pending_permissions.read().await;
            {
                let eval_guard = self
                    .evaluating_permissions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if eval_guard.contains(&key) {
                    anyhow::bail!(
                        "permission `{request_id}` is currently being evaluated by PreToolUse / Guardian and cannot be resolved yet"
                    );
                }
                if !guard.contains_key(&key)
                    && (!allow_human_only_resolution
                        || eval_guard.iter().any(|(aid, _)| aid == agent_id))
                {
                    anyhow::bail!("no pending permission `{request_id}` for agent `{agent_id}`");
                }
            }
            if let Some(existing) = guard.get(&key)
                && !allow_human_only_resolution
                && (existing.human_only
                    || existing
                        .guardian_assessment
                        .as_ref()
                        .is_some_and(crate::guardian::assessment_requires_human_only))
            {
                anyhow::bail!(
                    "permission `{request_id}` is marked human-only and cannot be resolved by an agent"
                );
            }
            guard.get(&key).cloned()
        };

        let allowed_decisions: &[String] = peeked
            .as_ref()
            .map(|p| p.allowed_decisions.as_slice())
            .unwrap_or(&[]);

        // Validate & canonicalize the decision ONCE before any persistence or provider dispatch.
        let trimmed = decision.trim();
        let canonical = match trimmed.to_ascii_lowercase().as_str() {
            "allow" | "allow_once" | "accept" => {
                if !allowed_decisions.is_empty()
                    && !crate::permission_rules::permits_allow_decision(allowed_decisions)
                {
                    anyhow::bail!(
                        "permission request `{request_id}` does not permit single-shot `allow`"
                    );
                }
                CanonicalDecision::Allow
            }
            "allow_always" => {
                if !allowed_decisions.is_empty()
                    && !crate::permission_rules::permits_allow_always_decision(allowed_decisions)
                {
                    anyhow::bail!(
                        "permission request `{request_id}` does not permit `allow_always`"
                    );
                }
                CanonicalDecision::AllowAlways
            }
            "deny" | "reject" | "reject_once" | "decline" => CanonicalDecision::Deny,
            _ => anyhow::bail!(
                "invalid permission decision `{decision}` (expected `allow`, `allow_always`, or `deny`)"
            ),
        };

        // Security invariant: agents/routers NEVER mint persistent `allow_always` grants.
        if !allow_human_only_resolution && canonical == CanonicalDecision::AllowAlways {
            anyhow::bail!(
                "persistent `allow_always` grants require explicit human approval and cannot be issued by an agent"
            );
        }

        // Security invariant: handle human-only plan approval gates via dedicated workflow.
        if let Some(ref existing) = peeked
            && existing.plan_approval.is_some()
        {
            if !allow_human_only_resolution {
                anyhow::bail!(
                    "plan approval gate `{request_id}` requires explicit human review and cannot be resolved by an agent"
                );
            }
            if canonical == CanonicalDecision::AllowAlways {
                anyhow::bail!("plan approval gate `{request_id}` does not support `allow_always`");
            }
            let (action, target_mode) = match canonical {
                CanonicalDecision::Allow => (
                    orbit_protocol::PlanApprovalAction::ApproveAndBuild,
                    Some(SessionMode::Default),
                ),
                CanonicalDecision::Deny => (orbit_protocol::PlanApprovalAction::Dismiss, None),
                CanonicalDecision::AllowAlways => unreachable!(),
            };
            return self
                .resolve_plan_approval(agent_id, request_id, action, None, None, target_mode)
                .await;
        }

        // Handle `allow_always` with Orbit SQLite as the single authoritative persistence layer:
        // derive & persist the Orbit rule FIRST, then dispatch a one-shot `allow` to the provider
        // whenever `allow` is supported so revoking a rule or adding a `Deny` rule takes effect
        // immediately across all providers (Claude, Codex, Antigravity, ACP).
        let (wire_decision, timeline_decision) = if canonical == CanonicalDecision::AllowAlways {
            let Some(ref pending) = peeked else {
                anyhow::bail!(
                    "no pending permission `{request_id}` for agent `{agent_id}` to save as Always-Allow"
                );
            };
            let provider_supports_one_shot_allow =
                crate::permission_rules::permits_allow_decision(&pending.allowed_decisions);
            let Some(tx) = self.live_agent_tx(agent_id).await else {
                anyhow::bail!("agent session not active");
            };
            let permit = tx
                .reserve()
                .await
                .map_err(|_| anyhow::anyhow!("agent session not active"))?;

            let ag = self
                .store
                .get_agent(agent_id)
                .map_err(|e| anyhow::anyhow!("failed to look up agent scope: {e}"))?
                .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;

            let derived_rule = crate::permission_rules::derive_rule_from_always_allow(
                &ag.cwd,
                ag.provider,
                &pending.title,
                &pending.detail,
                false,
            );

            if provider_supports_one_shot_allow && derived_rule.is_none() {
                anyhow::bail!(
                    "cannot create a safe persistent Always-Allow rule for this request (compound/expanding command, multi-file/placeholder path, or display-title confirmation); choose one-time Allow instead"
                );
            }

            // Atomically claim the exact pending entry FIRST so a concurrent caller that already
            // denied/resolved this request never causes a stale Always-Allow rule to be persisted.
            let claimed_pending = {
                let mut guard = self.pending_permissions.write().await;
                let still_matches = guard.get(&key).is_some_and(|cur| {
                    cur.created_at == pending.created_at && cur.plan_approval.is_none()
                });
                if !still_matches {
                    anyhow::bail!("permission request `{request_id}` was already resolved");
                }
                guard.remove(&key)
            };

            // Persist the Orbit SQLite rule BEFORE dispatching to the driver; on persistence
            // failure restore the claimed pending entry so the human can retry cleanly.
            if let Some(ref rule_input) = derived_rule
                && let Err(e) = self.store.upsert_permission_rule(rule_input)
            {
                if let Some(restored) = claimed_pending {
                    self.pending_permissions.write().await.insert(key, restored);
                }
                anyhow::bail!("failed to persist Always-Allow rule: {e}");
            }

            let dispatch_dec = if provider_supports_one_shot_allow {
                crate::guardian::pick_auto_allow_decision(&pending.allowed_decisions)
            } else {
                "allow_always".to_string()
            };
            permit.send(DriverCommand::ResolvePermission {
                request_id: request_id.clone(),
                decision: dispatch_dec,
            });

            let clear_attn = !self.has_pending_permissions(agent_id).await;
            let aid = agent_id.to_string();
            if let Ok(Some(ev)) = self
                .with_store(move |s| {
                    let ev = s
                        .append_timeline_event(
                            &aid,
                            1,
                            None,
                            TimelinePayload::PermissionResolved {
                                request_id,
                                decision: "allow_always".into(),
                            },
                        )
                        .ok();
                    if clear_attn {
                        let _ = s.set_agent_attention(&aid, AttentionState::None);
                    }
                    ev
                })
                .await
            {
                let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            }
            return Ok(());
        } else {
            let wire_dec = match canonical {
                CanonicalDecision::Allow => {
                    if allowed_decisions.is_empty() {
                        "allow".to_string()
                    } else {
                        crate::guardian::pick_auto_allow_decision(allowed_decisions)
                    }
                }
                CanonicalDecision::Deny => {
                    if allowed_decisions.is_empty() {
                        "deny".to_string()
                    } else {
                        crate::guardian::pick_auto_deny_decision(allowed_decisions)
                    }
                }
                CanonicalDecision::AllowAlways => unreachable!(),
            };
            let timeline_dec = match canonical {
                CanonicalDecision::Allow => "allow".to_string(),
                CanonicalDecision::Deny => "deny".to_string(),
                CanonicalDecision::AllowAlways => unreachable!(),
            };
            (wire_dec, timeline_dec)
        };

        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let permit = tx
                .reserve()
                .await
                .map_err(|_| anyhow::anyhow!("agent session not active"))?;
            {
                let mut guard = self.pending_permissions.write().await;
                {
                    let eval_guard = self
                        .evaluating_permissions
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if eval_guard.contains(&key) {
                        anyhow::bail!(
                            "permission `{request_id}` is currently being evaluated by PreToolUse / Guardian and cannot be resolved yet"
                        );
                    }
                }
                if let Some(ref expected) = peeked {
                    match guard.get(&key) {
                        Some(existing) if existing.plan_approval.is_some() => {
                            anyhow::bail!(
                                "plan approval gate `{request_id}` requires explicit human review"
                            );
                        }
                        Some(existing)
                            if !allow_human_only_resolution
                                && (existing.human_only
                                    || existing.guardian_assessment.as_ref().is_some_and(
                                        crate::guardian::assessment_requires_human_only,
                                    )) =>
                        {
                            anyhow::bail!(
                                "permission `{request_id}` is marked human-only and cannot be resolved by an agent"
                            );
                        }
                        Some(existing) if existing.created_at == expected.created_at => {
                            guard.remove(&key);
                        }
                        _ => {
                            anyhow::bail!("permission request `{request_id}` was already resolved");
                        }
                    }
                } else {
                    if let Some(existing) = guard.get(&key) {
                        if existing.plan_approval.is_some() {
                            anyhow::bail!(
                                "plan approval gate `{request_id}` requires explicit human review"
                            );
                        }
                        if !allow_human_only_resolution
                            && (existing.human_only
                                || existing
                                    .guardian_assessment
                                    .as_ref()
                                    .is_some_and(crate::guardian::assessment_requires_human_only))
                        {
                            anyhow::bail!(
                                "permission `{request_id}` is marked human-only and cannot be resolved by an agent"
                            );
                        }
                    }
                    guard.remove(&key);
                }
            }
            permit.send(DriverCommand::ResolvePermission {
                request_id: request_id.clone(),
                decision: wire_decision,
            });

            let clear_attn = !self.has_pending_permissions(agent_id).await;
            let aid = agent_id.to_string();
            if let Ok(Some(ev)) = self
                .with_store(move |s| {
                    let ev = s
                        .append_timeline_event(
                            &aid,
                            1,
                            None,
                            TimelinePayload::PermissionResolved {
                                request_id,
                                decision: timeline_decision,
                            },
                        )
                        .ok();
                    if clear_attn {
                        let _ = s.set_agent_attention(&aid, AttentionState::None);
                    }
                    ev
                })
                .await
            {
                let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            }
        } else {
            self.pending_permissions.write().await.remove(&key);
        }
        Ok(())
    }

    pub fn list_permission_rules(
        &self,
        project_cwd: Option<&str>,
    ) -> anyhow::Result<Vec<orbit_protocol::PermissionRuleRecord>> {
        self.store
            .list_permission_rules(project_cwd)
            .map_err(|e| anyhow::anyhow!("list permission rules failed: {e}"))
    }

    pub fn upsert_permission_rule(
        &self,
        input: orbit_protocol::UpsertPermissionRuleInput,
    ) -> anyhow::Result<orbit_protocol::PermissionRuleRecord> {
        crate::permission_rules::validate_permission_rule_input(&input)
            .map_err(|e| anyhow::anyhow!(e))?;
        self.store
            .upsert_permission_rule(&input)
            .map_err(|e| anyhow::anyhow!("upsert permission rule failed: {e}"))
    }

    pub fn delete_permission_rule(&self, rule_id: &str) -> anyhow::Result<bool> {
        self.store
            .delete_permission_rule(rule_id)
            .map_err(|e| anyhow::anyhow!("delete permission rule failed: {e}"))
    }

    /// Server-verified safe batch permission resolution (`ResolvePermissionsBatch`).
    ///
    /// Security invariants (enforced independently on the daemon regardless of client filtering):
    /// - Strictly rejects `"allow_always"` or unknown batch decisions (only `"allow"` or `"deny"`).
    /// - Strictly skips any item with `plan_approval.is_some()`.
    /// - For `"allow"`: requires Guardian to be enabled on this host, requires agent scope and
    ///   permission-rule store lookups to succeed (fail-closed), strictly skips items matching a
    ///   `Deny` rule, strictly skips items that do not permit `"allow"` in `allowed_decisions`,
    ///   strictly skips `High`/`Critical`/`fail_closed`/`guardian_disabled` assessments, and
    ///   evaluates uncached items under the **current host Guardian policy** (including host
    ///   `blocked_command_patterns`).
    /// - Atomically reserves the session channel permit and claims the exact pending entry under
    ///   lock before dispatching so concurrent callers can never double-dispatch or drop items.
    pub async fn resolve_permissions_batch(
        self: &Arc<Self>,
        items: Vec<(String, String)>,
        decision: String,
    ) -> anyhow::Result<orbit_protocol::ResolvePermissionsBatchResult> {
        let normalized_decision = decision.trim().to_ascii_lowercase();
        let is_allow_batch = match normalized_decision.as_str() {
            "allow" | "allow_once" | "accept" => true,
            "deny" | "reject" | "reject_once" | "decline" => false,
            _ => anyhow::bail!(
                "unsupported batch permission decision `{decision}` (only `allow` or `deny` permitted)"
            ),
        };

        let host_guardian_cfg = self.guardian.get_config().await;
        let mut seen = std::collections::HashSet::new();
        let mut resolved_count = 0usize;
        let mut skipped_reasons = Vec::new();

        for (agent_id, perm_id) in items {
            let key = (agent_id.clone(), perm_id.clone());
            if !seen.insert(key.clone()) {
                continue;
            }

            let (pending, initial_live_gen) = {
                let guard = self.pending_permissions.read().await;
                let live_guard = self.live_agents.read().await;
                (
                    guard.get(&key).cloned(),
                    live_guard.get(&agent_id).map(|e| e.generation),
                )
            };
            let Some(pending) = pending else {
                skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                    agent_id,
                    permission_request_id: perm_id,
                    reason: "not_pending".into(),
                });
                continue;
            };

            // Security invariant 1: plan approval gates NEVER batch-resolve.
            if pending.plan_approval.is_some() {
                skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                    agent_id,
                    permission_request_id: perm_id,
                    reason: "plan_approval_requires_individual_review".into(),
                });
                continue;
            }

            let (resolved_wire_decision, timeline_decision) = if is_allow_batch {
                // Security invariant 1b: human-only verdicts (PreToolUse fail-closed, LLM fail-closed)
                // NEVER batch-approve.
                if pending.human_only {
                    skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                        agent_id,
                        permission_request_id: perm_id,
                        reason: "human_only_requires_individual_review".into(),
                    });
                    continue;
                }

                // Security invariant 2: `allowed_decisions` must explicitly permit `"allow"`.
                if !crate::permission_rules::permits_allow_decision(&pending.allowed_decisions) {
                    skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                        agent_id,
                        permission_request_id: perm_id,
                        reason: "allow_decision_not_permitted".into(),
                    });
                    continue;
                }

                // Security invariant 3: Guardian MUST be enabled on this host to certify safe batch.
                if !host_guardian_cfg.enabled {
                    skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                        agent_id,
                        permission_request_id: perm_id,
                        reason: "guardian_disabled".into(),
                    });
                    continue;
                }

                // Security invariant 4: fail-closed on agent-scope or permission-rules store failure.
                let ag = match self.store.get_agent(&agent_id) {
                    Ok(Some(ag)) => ag,
                    Ok(None) | Err(_) => {
                        skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                            agent_id,
                            permission_request_id: perm_id,
                            reason: "agent_scope_unavailable".into(),
                        });
                        continue;
                    }
                };

                let rules = match self.store.list_permission_rules(Some(&ag.cwd)) {
                    Ok(rules) => rules,
                    Err(_) => {
                        skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                            agent_id,
                            permission_request_id: perm_id,
                            reason: "permission_rules_store_unavailable".into(),
                        });
                        continue;
                    }
                };

                // Respect any active Deny permission rule even if added after the prompt arrived.
                if let Some(matched) = crate::permission_rules::evaluate_permission_rules(
                    &rules,
                    &crate::permission_rules::PermissionRuleEvalContext {
                        project_cwd: &ag.cwd,
                        provider: ag.provider,
                        title: &pending.title,
                        detail: &pending.detail,
                        has_plan_approval: false,
                    },
                ) && matches!(
                    matched.rule.decision,
                    orbit_protocol::PermissionRuleDecision::Deny
                ) {
                    skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                        agent_id,
                        permission_request_id: perm_id,
                        reason: "denied_by_permission_rule".into(),
                    });
                    continue;
                }

                // Security invariant 5: validate cached assessment (rejecting disabled/fail-closed/
                // High/Critical/AutoDeny) AND evaluate uncached or policy-constrained items against
                // the CURRENT host Guardian policy (preserving `blocked_command_patterns`).
                let is_unassessed_or_disabled = |a: &orbit_protocol::GuardianAssessment| -> bool {
                    a.classifier_tier == "fail_closed"
                        || a.signals
                            .iter()
                            .any(|s| s == "guardian_disabled" || s == "fail_closed")
                };

                if let Some(ref cached) = pending.guardian_assessment {
                    if is_unassessed_or_disabled(cached) {
                        skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                            agent_id,
                            permission_request_id: perm_id,
                            reason: "guardian_disabled".into(),
                        });
                        continue;
                    }
                    if cached.risk_level >= orbit_protocol::GuardianRiskLevel::High
                        || cached.decision == orbit_protocol::GuardianDecision::AutoDeny
                    {
                        skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                            agent_id,
                            permission_request_id: perm_id,
                            reason: "high_or_critical_risk".into(),
                        });
                        continue;
                    }
                }

                if pending.guardian_assessment.is_none()
                    || !host_guardian_cfg.blocked_command_patterns.is_empty()
                {
                    let eval_req = crate::guardian::GuardianEvalRequest {
                        agent_id: &agent_id,
                        request_id: &perm_id,
                        title: &pending.title,
                        detail: &pending.detail,
                        workspace_cwd: &ag.cwd,
                        agent_provider: Some(ag.provider),
                    };
                    let assessment = self
                        .guardian
                        .assess_with_config(&eval_req, &host_guardian_cfg)
                        .await;
                    if is_unassessed_or_disabled(&assessment) {
                        skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                            agent_id,
                            permission_request_id: perm_id,
                            reason: "guardian_disabled".into(),
                        });
                        continue;
                    }
                    if assessment.risk_level >= orbit_protocol::GuardianRiskLevel::High
                        || assessment.decision == orbit_protocol::GuardianDecision::AutoDeny
                    {
                        skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                            agent_id,
                            permission_request_id: perm_id,
                            reason: "high_or_critical_risk".into(),
                        });
                        continue;
                    }
                }

                (
                    crate::guardian::pick_auto_allow_decision(&pending.allowed_decisions),
                    "allow".to_string(),
                )
            } else {
                (
                    crate::guardian::pick_auto_deny_decision(&pending.allowed_decisions),
                    "deny".to_string(),
                )
            };

            // Security invariant 6 (Atomic claim & session-bound dispatch):
            // Bind to the live session generation observed when reading `pending`, reserve a send
            // permit first, then atomically verify generation + exact pending entry under lock
            // before sending synchronously on the permit.
            let Some((expected_gen, tx)) = ({
                let live_guard = self.live_agents.read().await;
                live_guard.get(&agent_id).and_then(|e| {
                    (initial_live_gen == Some(e.generation))
                        .then(|| (e.generation, e.cmd_tx.clone()))
                })
            }) else {
                skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                    agent_id,
                    permission_request_id: perm_id,
                    reason: "agent_session_not_active".into(),
                });
                continue;
            };
            let Ok(permit) = tx.reserve().await else {
                skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                    agent_id,
                    permission_request_id: perm_id,
                    reason: "agent_session_not_active".into(),
                });
                continue;
            };

            let claimed = {
                let mut guard = self.pending_permissions.write().await;
                let live_guard = self.live_agents.read().await;
                let session_still_bound = live_guard
                    .get(&agent_id)
                    .is_some_and(|e| e.generation == expected_gen);
                let matches_snapshot = session_still_bound
                    && guard.get(&key).is_some_and(|cur| {
                        cur.created_at == pending.created_at
                            && cur.plan_approval.is_none()
                            && cur.allowed_decisions == pending.allowed_decisions
                    });
                if matches_snapshot {
                    guard.remove(&key).is_some()
                } else {
                    false
                }
            };

            if !claimed {
                // Drop `permit` without sending — another caller already resolved or replaced it.
                drop(permit);
                skipped_reasons.push(orbit_protocol::BatchPermissionSkipReason {
                    agent_id,
                    permission_request_id: perm_id,
                    reason: "already_resolved".into(),
                });
                continue;
            }

            permit.send(DriverCommand::ResolvePermission {
                request_id: perm_id.clone(),
                decision: resolved_wire_decision,
            });
            if let Ok(ev) = self.store.append_timeline_event(
                &agent_id,
                1,
                None,
                TimelinePayload::PermissionResolved {
                    request_id: perm_id,
                    decision: timeline_decision,
                },
            ) {
                let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            }
            if !self.has_pending_permissions(&agent_id).await {
                let _ = self
                    .store
                    .set_agent_attention(&agent_id, AttentionState::None);
            }
            resolved_count += 1;
        }

        let skipped_count = skipped_reasons.len();
        Ok(orbit_protocol::ResolvePermissionsBatchResult {
            resolved_count,
            skipped_count,
            skipped_reasons,
        })
    }

    pub async fn resolve_plan_approval(
        self: &Arc<Self>,
        agent_id: &str,
        request_id: String,
        action: orbit_protocol::PlanApprovalAction,
        edited_plan_markdown: Option<String>,
        feedback: Option<String>,
        target_mode: Option<SessionMode>,
    ) -> anyhow::Result<()> {
        let key = (agent_id.to_string(), request_id.clone());
        // Atomically verify & single-claim the pending plan approval gate so concurrent or stale
        // calls cannot execute a build/mode transition twice and non-plan permissions are untouched.
        let pending = {
            let mut guard = self.pending_permissions.write().await;
            if self
                .evaluating_permissions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains(&key)
            {
                anyhow::bail!(
                    "permission `{request_id}` is currently being evaluated by PreToolUse / Guardian and cannot be resolved yet"
                );
            }
            match guard.get(&key) {
                None => {
                    anyhow::bail!("no pending plan approval `{request_id}` for agent `{agent_id}`");
                }
                Some(p) if p.plan_approval.is_none() => {
                    anyhow::bail!(
                        "pending permission `{request_id}` for agent `{agent_id}` is not a plan approval gate"
                    );
                }
                Some(_) => match guard.remove(&key) {
                    Some(claimed) => claimed,
                    None => {
                        anyhow::bail!(
                            "no pending plan approval `{request_id}` for agent `{agent_id}`"
                        );
                    }
                },
            }
        };

        let Some(plan) = pending.plan_approval.clone() else {
            self.pending_permissions.write().await.insert(key, pending);
            anyhow::bail!(
                "pending permission `{request_id}` for agent `{agent_id}` is not a plan approval gate"
            );
        };

        let aid_plan = agent_id.to_string();
        let agent = match self.try_store(move |s| s.get_agent(&aid_plan)).await {
            Ok(Some(ag)) => ag,
            Ok(None) => {
                self.pending_permissions.write().await.insert(key, pending);
                anyhow::bail!("agent `{agent_id}` not found");
            }
            Err(e) => {
                self.pending_permissions.write().await.insert(key, pending);
                return Err(e);
            }
        };

        let effective_target_mode = target_mode.unwrap_or(SessionMode::Default);
        let effective_edited = edited_plan_markdown
            .filter(|edited| orbit_protocol::is_plan_edited(&plan.plan_markdown, Some(edited)));

        // Reserve a delivery permit on the live driver channel BEFORE mutating disk or mode so
        // driver send failure never leaves mode mutated or silently consumes the pending gate.
        let live_tx = self.live_agent_tx(agent_id).await;
        let driver_is_live = live_tx.is_some();
        let driver_permit = if let Some(ref tx) = live_tx {
            match tx.reserve().await {
                Ok(permit) => Some(permit),
                Err(_) => {
                    self.pending_permissions.write().await.insert(key, pending);
                    anyhow::bail!("agent session not active");
                }
            }
        } else {
            None
        };

        if matches!(action, orbit_protocol::PlanApprovalAction::ApproveAndBuild) {
            // Sole plan-file write owner: ONLY persist back to disk when `plan_file_backed`
            // verified that `plan_file_path` is the exact file loaded into the card, and surface
            // any persistence failure cleanly before mutating mode or releasing the gate.
            if let Some(ref edited) = effective_edited
                && plan.plan_file_backed
                && let Some(ref fp) = plan.plan_file_path
            {
                match orbit_providers::plan_approval::write_edited_plan_file_safe(
                    fp,
                    Path::new(&agent.cwd),
                    edited,
                ) {
                    Ok(true) => {}
                    Ok(false) => {
                        drop(driver_permit);
                        self.pending_permissions.write().await.insert(key, pending);
                        anyhow::bail!(
                            "cannot save edited plan: `{fp}` is outside verified plan roots"
                        );
                    }
                    Err(err_msg) => {
                        drop(driver_permit);
                        self.pending_permissions.write().await.insert(key, pending);
                        anyhow::bail!("{err_msg}");
                    }
                }
            }

            let aid_mode = agent_id.to_string();
            let mode_to_set = effective_target_mode.clone();
            if let Err(e) = self
                .try_store(move |s| s.update_agent_mode(&aid_mode, &mode_to_set))
                .await
            {
                drop(driver_permit);
                self.pending_permissions.write().await.insert(key, pending);
                return Err(e);
            }
        }

        // Infallible commit to live driver (permit already reserved).
        if let Some(permit) = driver_permit {
            permit.send(DriverCommand::ResolvePlanApproval {
                request_id: request_id.clone(),
                action,
                edited_plan_markdown: effective_edited.clone(),
                feedback: feedback.clone(),
                target_mode: effective_target_mode.clone(),
            });
        }

        let no_pending = !self.has_pending_permissions(agent_id).await;
        let is_approve_build =
            matches!(action, orbit_protocol::PlanApprovalAction::ApproveAndBuild);
        let aid_after = agent_id.to_string();
        let rid_after = request_id.clone();
        let decision_str = action.as_decision_str().to_string();
        if let Ok((pre_updated, ev_opt, post_updated)) = self
            .with_store(move |s| {
                let pre_updated = if is_approve_build {
                    s.get_agent(&aid_after).ok().flatten()
                } else {
                    None
                };
                let ev = s
                    .append_timeline_event(
                        &aid_after,
                        1,
                        None,
                        TimelinePayload::PermissionResolved {
                            request_id: rid_after,
                            decision: decision_str,
                        },
                    )
                    .ok();
                let post_updated = if no_pending {
                    let _ = s.set_agent_attention(&aid_after, AttentionState::None);
                    if !driver_is_live {
                        let _ = s.update_agent_status(&aid_after, &AgentStatus::Idle);
                    }
                    s.get_agent(&aid_after).ok().flatten()
                } else {
                    None
                };
                (pre_updated, ev, post_updated)
            })
            .await
        {
            if let Some(updated) = pre_updated {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::AgentUpdated(updated));
            }
            if let Some(ev) = ev_opt {
                let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            }
            if let Some(updated) = post_updated {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::AgentUpdated(updated));
            }
        }

        if let Some(followup) = crate::plan_approval::decide_plan_followup(
            agent.provider,
            Some(&pending),
            &request_id,
            action,
            effective_edited.as_deref(),
            feedback.as_deref(),
            driver_is_live,
        ) {
            self.send_prompt_with_options(
                agent_id,
                followup.wire_prompt,
                Some(followup.display_prompt),
                Vec::new(),
                followup.behavior,
                None,
            )
            .await?;
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

            let aid_q = agent_id.to_string();
            if let Ok(ev) = self
                .try_store(move |s| {
                    s.append_timeline_event(
                        &aid_q,
                        1,
                        None,
                        TimelinePayload::QuestionResolved {
                            request_id,
                            answers,
                        },
                    )
                })
                .await
            {
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
            let aid_get = agent_id.to_string();
            let existing = self.try_store(move |s| s.get_agent(&aid_get)).await?;
            let parent_ws_id = existing.as_ref().map(|a| a.workspace_id.clone());

            // 1. Stop live runtime & clear pending permissions
            let _ = self.stop_and_wait_driver(agent_id).await;

            // 2. Retire any heartbeats & armed conditional timers bound to this agent
            if let Some(timers) = self.conditional_timers().await {
                timers
                    .cancel_all_for_agent(agent_id, "Agent archived")
                    .await;
            }
            // 3. Soft-archive record in SQLite & delete heartbeats
            let aid_arch = agent_id.to_string();
            let removed_hbs = self
                .try_store(move |s| -> anyhow::Result<Vec<String>> {
                    let hbs = s.delete_heartbeats_for_agent(&aid_arch).unwrap_or_default();
                    s.archive_agent_soft(&aid_arch)?;
                    Ok(hbs)
                })
                .await?;
            for hb_id in removed_hbs {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::ScheduleDeleted(hb_id));
            }
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
            if let Some(ref parent_id) = existing.as_ref().and_then(|a| a.parent_agent_id.clone())
                && let Some(timers) = self.conditional_timers().await
            {
                let all_settled = self.are_all_subagents_settled_async(parent_id).await;
                timers.notify_subagent_settled(parent_id, all_settled).await;
            }
            if let Some(ref ws_id) = parent_ws_id
                && let Some(timers) = self.conditional_timers().await
            {
                timers.notify_workspace_peer_settled(ws_id, agent_id).await;
            }
            self.roster_states.write().await.remove(agent_id);
            self.injected_rule_hashes.write().await.remove(agent_id);

            let aid_post = agent_id.to_string();
            let (updated_opt, still_used_wt, children) = self
                .try_store(move |s| -> anyhow::Result<_> {
                    let updated = s.get_agent(&aid_post).ok().flatten();
                    let still_used = if let Some(ref ag) = updated
                        && ag.labels.contains_key("orbit.worktree_source_cwd")
                    {
                        s.list_agents_filtered(None, false, true, None)
                            .unwrap_or_default()
                            .iter()
                            .any(|a| a.cwd == ag.cwd)
                    } else {
                        true
                    };
                    let children = s.list_child_agents(&aid_post)?;
                    Ok((updated, still_used, children))
                })
                .await?;
            if let Some(updated) = updated_opt {
                let wt_src = updated.labels.get("orbit.worktree_source_cwd").cloned();
                let wt_branch = updated.labels.get("orbit.worktree_branch").cloned();
                let wt_cwd = updated.cwd.clone();
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::AgentUpdated(updated));
                // Clean up peer-owned isolated worktree if no other active agent uses it
                if let Some(src) = wt_src
                    && !still_used_wt
                {
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

            // 4. Resolve managed children: detach cross-workspace / open-tab children, cascade-archive the rest
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
        let aid = agent_id.to_string();
        let updated = self
            .try_store(move |s| -> anyhow::Result<AgentSummary> {
                s.unarchive_agent(&aid)?;
                s.get_agent(&aid)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid}` not found"))
            })
            .await?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(updated.clone()));
        Ok(updated)
    }

    pub async fn detach_agent(&self, agent_id: &str) -> anyhow::Result<AgentSummary> {
        let aid = agent_id.to_string();
        let (prev_parent_id, updated) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let prev_parent = s.get_agent(&aid)?.and_then(|a| a.parent_agent_id);
                s.detach_agent(&aid)?;
                let updated = s
                    .get_agent(&aid)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid}` not found"))?;
                Ok((prev_parent, updated))
            })
            .await?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(updated.clone()));
        if let Some(ref parent_id) = prev_parent_id
            && let Some(timers) = self.conditional_timers().await
        {
            let all_settled = self.are_all_subagents_settled_async(parent_id).await;
            timers.notify_subagent_settled(parent_id, all_settled).await;
        }
        Ok(updated)
    }

    pub async fn kill_agent(&self, agent_id: &str) -> anyhow::Result<()> {
        let _ = self.stop_and_wait_driver(agent_id).await;
        let aid = agent_id.to_string();
        let updated_opt = self
            .try_store(move |s| -> anyhow::Result<Option<AgentSummary>> {
                s.update_agent_status(&aid, &AgentStatus::Closed)?;
                Ok(s.get_agent(&aid).ok().flatten())
            })
            .await?;
        if let Some(updated) = updated_opt {
            let parent_id = updated.parent_agent_id.clone();
            let ws_id = updated.workspace_id.clone();
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
            if let Some(timers) = self.conditional_timers().await {
                if let Some(ref pid) = parent_id {
                    let all_settled = self.are_all_subagents_settled_async(pid).await;
                    timers.notify_subagent_settled(pid, all_settled).await;
                }
                timers.notify_workspace_peer_settled(&ws_id, agent_id).await;
            }
        }
        Ok(())
    }

    pub async fn delete_agent(self: &Arc<Self>, agent_id: &str) -> anyhow::Result<()> {
        let aid_get = agent_id.to_string();
        let existing = self
            .try_store(move |s| s.get_agent(&aid_get))
            .await
            .ok()
            .flatten();
        let prev_parent_id = existing.as_ref().and_then(|a| a.parent_agent_id.clone());
        let prev_ws_id = existing.map(|a| a.workspace_id);
        let _ = self.kill_agent(agent_id).await;
        if let Some(timers) = self.conditional_timers().await {
            timers.cancel_all_for_agent(agent_id, "Agent deleted").await;
        }
        let aid_del = agent_id.to_string();
        let removed_hbs = self
            .try_store(move |s| -> anyhow::Result<Vec<String>> {
                let _ = s.delete_conditional_timers_for_agent(&aid_del);
                let hbs = s.delete_heartbeats_for_agent(&aid_del).unwrap_or_default();
                s.delete_agent(&aid_del)?;
                Ok(hbs)
            })
            .await?;
        for hb_id in removed_hbs {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::ScheduleDeleted(hb_id));
        }
        if let Some(timers) = self.conditional_timers().await {
            if let Some(ref parent_id) = prev_parent_id {
                let all_settled = self.are_all_subagents_settled_async(parent_id).await;
                timers.notify_subagent_settled(parent_id, all_settled).await;
            }
            if let Some(ref ws_id) = prev_ws_id {
                timers.notify_workspace_peer_settled(ws_id, agent_id).await;
            }
        }
        self.driver_start_locks.write().await.remove(agent_id);
        self.agent_generations.write().await.remove(agent_id);
        self.roster_states.write().await.remove(agent_id);
        self.injected_rule_hashes.write().await.remove(agent_id);
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
        let aid = agent_id.to_string();
        if let Some((updated, changed)) = self
            .try_store(move |s| s.mark_agent_read_seq_with_changed(&aid, seen_seq))
            .await?
            && changed
        {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        Ok(())
    }

    pub async fn mark_agent_unread(&self, agent_id: &str) -> anyhow::Result<()> {
        let aid = agent_id.to_string();
        if let Some(updated) = self.try_store(move |s| s.mark_agent_unread(&aid)).await? {
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
        let aid = agent_id.to_string();
        if let Some(updated) = self
            .try_store(move |s| s.set_agent_pinned(&aid, pinned, pinned_order))
            .await?
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
        match self.update_workspace_board_cas(
            workspace_id,
            content,
            section,
            None,
            updated_by,
            editor_agent_id,
        )? {
            Ok(snap) | Err(snap) => Ok(snap),
        }
    }

    /// Atomically update `BOARD.md` in SQLite and on disk, optionally enforcing an optimistic lock
    /// via `expected_revision`. Returns `Ok(Err(current_snapshot))` when the revision is stale.
    pub fn update_workspace_board_cas(
        &self,
        workspace_id: &str,
        content: &str,
        section: Option<&str>,
        expected_revision: Option<u64>,
        updated_by: Option<&str>,
        editor_agent_id: Option<&str>,
    ) -> anyhow::Result<Result<WorkspaceBoardSnapshot, WorkspaceBoardSnapshot>> {
        let ws = self
            .store
            .get_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("workspace `{workspace_id}` not found"))?;
        let clean_section = section.map(str::trim).filter(|s| !s.is_empty());
        let target_disk_path = board_file_path(&self.home_dir, workspace_id)
            .to_string_lossy()
            .to_string();

        let cas_res = self.store.upsert_workspace_board_with_cas(
            workspace_id,
            expected_revision,
            updated_by,
            Some(&target_disk_path),
            |existing_content| match clean_section {
                Some(sec) => {
                    let base = existing_content.map(ToOwned::to_owned).unwrap_or_else(|| {
                        default_board_template(&ws.title, ws.mission_goal.as_deref())
                    });
                    upsert_markdown_section(&base, sec, content)
                }
                None => content.to_string(),
            },
        )?;

        let snap = match cas_res {
            Ok(s) => s,
            Err(current) => return Ok(Err(current)),
        };

        let _ = write_board_file(&self.home_dir, workspace_id, &snap.content);
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceBoardUpdated(snap.clone()));
        let event_summary = match clean_section {
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
        Ok(Ok(snap))
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
        let aid = agent_id.to_string();
        let (agent, old_ws) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let ag = s
                    .get_agent(&aid)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid}` not found"))?;
                let old_ws = s.get_workspace(&ag.workspace_id).ok().flatten();
                Ok((ag, old_ws))
            })
            .await?;
        let target_ws = if make_unclassified {
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
                    let ws_id_owned = ws_id.clone();
                    self.try_store(move |s| -> anyhow::Result<WorkspaceSummary> {
                        let ws = s.get_workspace(&ws_id_owned)?.ok_or_else(|| {
                            anyhow::anyhow!("workspace `{ws_id_owned}` not found")
                        })?;
                        if !ws.is_curated {
                            Ok(s.set_workspace_curated(&ws.workspace_id, true)?
                                .unwrap_or(ws))
                        } else {
                            Ok(ws)
                        }
                    })
                    .await?
                }
                _ => {
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

        // Ensure unique handle in the destination workspace and move atomically off the async thread
        let aid_move = agent_id.to_string();
        let target_ws_id = target_ws.workspace_id.clone();
        let target_ws_title = target_ws.title.clone();
        let target_ws_curated = target_ws.is_curated;
        let (updated, peer_ev_opt, refreshed_ws_opt) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let dest_peers: Vec<AgentSummary> = s
                    .list_agents_filtered(Some(&target_ws_id), false, false, None)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|p| p.agent_id != aid_move)
                    .collect();
                let next_handle = allocate_unique_handle(
                    agent.handle.as_deref(),
                    agent.provider,
                    None,
                    Some(&agent.title),
                    &dest_peers,
                );
                let _ = s.set_agent_handle(&aid_move, Some(&next_handle));
                let updated = s
                    .move_agent_to_workspace(&aid_move, &target_ws_id, true)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid_move}` not found"))?;
                let peer_ev = if target_ws_curated {
                    s.append_workspace_event(
                        &target_ws_id,
                        Some(&aid_move),
                        Some(&next_handle),
                        WorkspaceEventKind::PeerJoined,
                        &format!("@{next_handle} joined workspace `{target_ws_title}`"),
                        None,
                    )
                    .ok()
                } else {
                    None
                };
                let refreshed_ws = s.get_workspace(&target_ws_id).ok().flatten();
                Ok((updated, peer_ev, refreshed_ws))
            })
            .await?;
        // Reset cached roster state so next turn reflects the new workspace peers
        self.roster_states.write().await.remove(agent_id);
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::AgentUpdated(updated.clone()));
        if let Some(ev) = peer_ev_opt {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
        }
        if let Some(refreshed_ws) = refreshed_ws_opt {
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
        let src_id = source_workspace_id.to_string();
        let dst_id = target_workspace_id.to_string();
        let (src_ws, dst_ws, src_agents, src_board_opt) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let src_ws = s
                    .get_workspace(&src_id)?
                    .ok_or_else(|| anyhow::anyhow!("source workspace `{src_id}` not found"))?;
                let dst_ws = s
                    .get_workspace(&dst_id)?
                    .ok_or_else(|| anyhow::anyhow!("target workspace `{dst_id}` not found"))?;
                let src_agents = s.list_agents_filtered(Some(&src_id), false, true, None)?;
                let mut dst_peers = s.list_agents_filtered(Some(&dst_id), false, false, None)?;
                for ag in &src_agents {
                    let unique_handle = allocate_unique_handle(
                        ag.handle.as_deref(),
                        ag.provider,
                        None,
                        Some(&ag.title),
                        &dst_peers,
                    );
                    let _ = s.set_agent_handle(&ag.agent_id, Some(&unique_handle));
                    let mut moved_copy = ag.clone();
                    moved_copy.handle = Some(unique_handle);
                    dst_peers.push(moved_copy);
                }
                let src_board = s
                    .get_workspace_board(&src_id)
                    .ok()
                    .flatten()
                    .filter(|b| !b.content.trim().is_empty());
                Ok((src_ws, dst_ws, src_agents, src_board))
            })
            .await?;

        {
            let mut roster = self.roster_states.write().await;
            for ag in &src_agents {
                roster.remove(&ag.agent_id);
            }
        }
        // Merge non-empty source board into target board if present (via blocking pool)
        if let Some(src_board) = src_board_opt {
            let dst_id_b = target_workspace_id.to_string();
            let home_b = self.home_dir.clone();
            let bcast_b = self.broadcast_tx.clone();
            let store_b = Arc::clone(&self.store);
            let section_name = format!("Merged from {}", src_ws.title);
            let board_content = src_board.content;
            let _ = with_store(&store_b, move |s| {
                if let Ok(Some(ws)) = s.get_workspace(&dst_id_b) {
                    let target_disk_path = board_file_path(&home_b, &dst_id_b)
                        .to_string_lossy()
                        .to_string();
                    if let Ok(Ok(snap)) = s.upsert_workspace_board_with_cas(
                        &dst_id_b,
                        None,
                        Some("workspace"),
                        Some(&target_disk_path),
                        |existing_content| {
                            let base =
                                existing_content.map(ToOwned::to_owned).unwrap_or_else(|| {
                                    default_board_template(&ws.title, ws.mission_goal.as_deref())
                                });
                            upsert_markdown_section(&base, &section_name, &board_content)
                        },
                    ) {
                        let _ = write_board_file(&home_b, &dst_id_b, &snap.content);
                        let _ = bcast_b.send(DaemonBroadcast::WorkspaceBoardUpdated(snap.clone()));
                        if let Ok(ev) = s.append_workspace_event(
                            &dst_id_b,
                            None,
                            Some("workspace"),
                            WorkspaceEventKind::BoardUpdated,
                            &format!(
                                "Updated Shared Board section `{section_name}` (rev {})",
                                snap.revision
                            ),
                            None,
                        ) {
                            let _ = bcast_b.send(DaemonBroadcast::WorkspaceEventRecorded(ev));
                        }
                        if let Ok(Some(updated_ws)) = s.get_workspace(&dst_id_b) {
                            let _ = bcast_b.send(DaemonBroadcast::WorkspaceUpdated(updated_ws));
                        }
                    }
                }
            })
            .await;
        }

        let src_id_m = source_workspace_id.to_string();
        let dst_id_m = target_workspace_id.to_string();
        let src_title = src_ws.title;
        let dst_title = dst_ws.title;
        let moved_agent_ids: Vec<String> = src_agents.into_iter().map(|a| a.agent_id).collect();
        let (moved, merge_ev_opt, updated_agents, updated_src, updated_dst) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let moved = s.merge_workspaces(&src_id_m, &dst_id_m)?;
                let merge_ev = s
                    .append_workspace_event(
                        &dst_id_m,
                        None,
                        None,
                        WorkspaceEventKind::WorkspaceMerged,
                        &format!(
                            "Merged workspace `{src_title}` ({moved} peer(s)) into `{dst_title}`"
                        ),
                        None,
                    )
                    .ok();
                let mut updated_agents = Vec::new();
                for aid in moved_agent_ids {
                    if let Ok(Some(updated)) = s.get_agent(&aid) {
                        updated_agents.push(updated);
                    }
                }
                let updated_src = s.get_workspace(&src_id_m).ok().flatten();
                let updated_dst = s.get_workspace(&dst_id_m).ok().flatten();
                Ok((moved, merge_ev, updated_agents, updated_src, updated_dst))
            })
            .await?;
        if let Some(ev) = merge_ev_opt {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
        }
        for updated in updated_agents {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::AgentUpdated(updated));
        }
        if let Some(updated_src) = updated_src {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceUpdated(updated_src));
        }
        if let Some(updated_dst) = updated_dst {
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

    /// Resolve a sibling peer inside `workspace_id` by `@handle`, `agent_id`, historical handle
    /// aliases (`orbit.handle_aliases`), or case-insensitive title substring.
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
        let sanitized = crate::workspace_cluster::sanitize_handle(clean);
        if !sanitized.is_empty()
            && let Some(norm_handle) = peers.iter().find(|a| {
                a.handle
                    .as_deref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(&sanitized))
            })
        {
            return Ok(Some(norm_handle.clone()));
        }
        if let Some(alias_match) = peers.iter().find(|a| {
            a.labels
                .get(orbit_store::LABEL_HANDLE_ALIASES)
                .is_some_and(|raw| {
                    raw.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .any(|alias| {
                            alias.eq_ignore_ascii_case(clean)
                                || (!sanitized.is_empty()
                                    && (alias.eq_ignore_ascii_case(&sanitized)
                                        || crate::workspace_cluster::sanitize_handle(alias)
                                            .eq_ignore_ascii_case(&sanitized)))
                        })
                })
        }) {
            return Ok(Some(alias_match.clone()));
        }
        let lower = clean.to_ascii_lowercase();
        Ok(peers
            .into_iter()
            .find(|a| a.title.to_ascii_lowercase().contains(&lower)))
    }

    /// Match workspace/user path-scoped glob rules against mentioned/edited paths and `@rule:` tokens,
    /// deduplicating unchanged rules across turns within the same session so prompt prefixes stay clean.
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
        let aid = agent_id.to_string();
        let Ok(Some((agent, edited_paths))) = self
            .with_store(move |s| {
                let agent = s.get_agent(&aid).ok().flatten()?;
                if agent.background {
                    return None;
                }
                let edited_paths: Vec<String> = s
                    .list_workspace_events(&agent.workspace_id, 24)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|ev| ev.kind == WorkspaceEventKind::FileEdited)
                    .filter_map(|ev| ev.file_path)
                    .collect();
                Some((agent, edited_paths))
            })
            .await
        else {
            return prompt;
        };
        let (active_paths, manual_rules) = crate::rules::extract_prompt_rule_signals(
            signal_source,
            badges,
            &agent.cwd,
            &edited_paths,
        );

        let mut prior_hashes = self
            .injected_rule_hashes
            .read()
            .await
            .get(agent_id)
            .cloned()
            .unwrap_or_default();
        let home_dir = self.home_dir.clone();
        let cwd = PathBuf::from(&agent.cwd);
        let (matched, updated_hashes) = tokio::task::spawn_blocking(move || {
            let res = crate::rules::match_rules_for_workspace_dedup(
                &home_dir,
                Some(&cwd),
                &active_paths,
                &manual_rules,
                &mut prior_hashes,
            );
            (res, prior_hashes)
        })
        .await
        .unwrap_or_default();

        if !updated_hashes.is_empty() {
            self.injected_rule_hashes
                .write()
                .await
                .insert(agent_id.to_string(), updated_hashes);
        }

        if matched.injected_markdown.is_empty() {
            prompt
        } else {
            format!("{}\n\n{prompt}", matched.injected_markdown)
        }
    }

    /// Prepend either the full `<orbit-workspace>` block (Turn 1) or a compact `<orbit-workspace-delta>`
    /// block (Turn 2+) whenever structural peers, shared board, or high-signal events change.
    async fn augment_prompt_with_workspace_context(
        &self,
        agent_id: &str,
        prompt: String,
    ) -> String {
        let aid = agent_id.to_string();
        let Ok(Some(agent)) = self.try_store(move |s| s.get_agent(&aid)).await else {
            return prompt;
        };
        if agent.background {
            return prompt;
        }
        if agent.is_personal_router() {
            if let Some(router) = self.personal_router().await
                && let Some(fleet_xml) = router.build_fleet_context_incremental(agent_id).await
            {
                return format!("{fleet_xml}\n\n{prompt}");
            }
            return prompt;
        }

        let prev_state = self.roster_states.read().await.get(agent_id).cloned();
        let ws_id = agent.workspace_id.clone();
        let aid_ctx = agent_id.to_string();
        let home_dir = self.home_dir.clone();
        let Ok(Some((maybe_xml, next_state))) = self
            .with_store(move |s| {
                let ws = s.get_workspace(&ws_id).ok().flatten()?;
                let peers = s
                    .list_agents_filtered(Some(&ws.workspace_id), false, false, None)
                    .unwrap_or_default();
                let events = s
                    .list_workspace_events(&ws.workspace_id, 16)
                    .unwrap_or_default();
                let board = s.get_workspace_board(&ws.workspace_id).ok().flatten();
                build_workspace_roster_context_incremental(
                    s,
                    &home_dir,
                    &ws,
                    &aid_ctx,
                    &peers,
                    &events,
                    board.as_ref(),
                    prev_state.as_ref(),
                )
            })
            .await
        else {
            return prompt;
        };

        self.roster_states
            .write()
            .await
            .insert(agent_id.to_string(), next_state);

        match maybe_xml {
            Some(xml) => format!("{xml}\n\n{prompt}"),
            None => prompt,
        }
    }

    async fn send_conflict_steer_if_active(&self, target_agent_id: &str, body: String) {
        let tid = target_agent_id.to_string();
        let Ok(Some(ag)) = self.try_store(move |s| s.get_agent(&tid)).await else {
            return;
        };
        if ag.archived_at.is_some()
            || ag.background
            || !matches!(
                ag.status,
                AgentStatus::Running { .. } | AgentStatus::AutonomousWakeup { .. }
            )
        {
            return;
        }
        let Some(delivery) = self.message_delivery().await else {
            return;
        };
        let _ = delivery
            .enqueue(QueuedAgentMessage {
                recipient_id: target_agent_id.to_string(),
                message_id: format!("cr_{}", Uuid::new_v4().simple()),
                source: AgentPromptSource {
                    kind: AgentMessageKind::FinishNotification,
                    source_agent_id: None,
                    source_host_id: Some(self.host_id.clone()),
                    source_title: Some("Conflict Radar".to_string()),
                    event: Some("conflict_warning".to_string()),
                    permission_context: None,
                },
                body,
                active_turn_behavior: ActiveTurnBehavior::Steer,
                session_mode: None,
                unarchive: false,
            })
            .await;
    }

    async fn check_conflict_cooldown(
        &self,
        workspace_id: &str,
        key_suffix: &str,
        cooldown_secs: u64,
    ) -> bool {
        let key = (workspace_id.to_string(), key_suffix.to_string());
        let mut guard = self.conflict_cooldowns.write().await;
        let now = std::time::Instant::now();
        guard.retain(|_, prev| now.duration_since(*prev) < Duration::from_secs(120));
        match guard.get(&key) {
            Some(prev) if now.duration_since(*prev) < Duration::from_secs(cooldown_secs) => false,
            _ => {
                guard.insert(key, now);
                true
            }
        }
    }

    /// Track file edits inside a Mission Workspace and emit live Conflict Radar alerts:
    /// - Same-directory edits within 10m emit `ConflictWarning` + UI Notice + active `Steer`
    ///   (strictly to currently `Running` / `AutonomousWakeup` peers — never wakes `Idle` agents).
    /// - Cross-worktree edits on the same path emit an early-warning `ConflictWarning` + active `Steer`.
    async fn record_file_edit_and_check_conflict(&self, agent_id: &str, raw_path: &str) {
        let trimmed = raw_path.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let aid = agent_id.to_string();
        let Ok(Some((agent, norm_path, edit_ev_opt, same_dir_peers, cross_wt_peers))) = self
            .with_store(move |s| {
                let agent = s.get_agent(&aid).ok().flatten()?;
                if agent.background {
                    return None;
                }
                let norm_path = Path::new(&trimmed)
                    .strip_prefix(&agent.cwd)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| trimmed.trim_start_matches("./").replace('\\', "/"));

                let handle = agent.handle.as_deref().unwrap_or("peer");
                let edit_ev = s
                    .append_workspace_event(
                        &agent.workspace_id,
                        Some(&aid),
                        Some(handle),
                        WorkspaceEventKind::FileEdited,
                        &format!("@{handle} edited `{norm_path}`"),
                        Some(&norm_path),
                    )
                    .ok();

                let since = Utc::now() - chrono::Duration::minutes(10);
                let other_editors = s
                    .recent_workspace_file_editors(&agent.workspace_id, &norm_path, &aid, since)
                    .unwrap_or_default();
                let mut same_dir_peers: Vec<(String, String)> = Vec::new();
                let mut cross_wt_peers: Vec<(String, String)> = Vec::new();
                for (other_id, other_handle) in other_editors {
                    if let Ok(Some(other_ag)) = s.get_agent(&other_id)
                        && other_ag.archived_at.is_none()
                        && !other_ag.background
                    {
                        let label = other_handle
                            .or(other_ag.handle)
                            .unwrap_or_else(|| other_ag.title.clone());
                        if other_ag.cwd == agent.cwd {
                            same_dir_peers.push((other_id, label));
                        } else {
                            cross_wt_peers.push((other_id, label));
                        }
                    }
                }
                Some((agent, norm_path, edit_ev, same_dir_peers, cross_wt_peers))
            })
            .await
        else {
            return;
        };
        if let Some(ev) = edit_ev_opt {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
        }
        if same_dir_peers.is_empty() && cross_wt_peers.is_empty() {
            return;
        }
        let handle = agent.handle.as_deref().unwrap_or("peer");

        if !same_dir_peers.is_empty()
            && self
                .check_conflict_cooldown(&agent.workspace_id, &norm_path, 60)
                .await
        {
            let others_str = same_dir_peers
                .iter()
                .map(|(_, h)| format!("@{h}"))
                .collect::<Vec<_>>()
                .join(", ");
            let msg = format!(
                "Conflict Radar: @{handle} and {others_str} both edited `{norm_path}` in the same checkout (`{}`). Unless hard-blocked, isolate one track into its own git worktree via `orbit_agent(action=\"fork\", worktree=true, branch=..., initial_prompt=...)` or `spawn_workspace_peer(worktree=true, ...)` and continue in parallel; rebase/merge via `handoff_worktree` later.",
                agent.cwd
            );
            let ws_id_c = agent.workspace_id.clone();
            let aid_c = agent_id.to_string();
            let handle_c = handle.to_string();
            let msg_c = msg.clone();
            let norm_c = norm_path.clone();
            if let Ok((ws_ev_opt, notice_ev_opt)) = self
                .with_store(move |s| {
                    let ws_ev = s
                        .append_workspace_event(
                            &ws_id_c,
                            Some(&aid_c),
                            Some(&handle_c),
                            WorkspaceEventKind::ConflictWarning,
                            &msg_c,
                            Some(&norm_c),
                        )
                        .ok();
                    let notice_ev = s
                        .append_timeline_event(
                            &aid_c,
                            1,
                            None,
                            TimelinePayload::Notice {
                                level: "warning".into(),
                                message: msg_c,
                            },
                        )
                        .ok();
                    (ws_ev, notice_ev)
                })
                .await
            {
                if let Some(ev) = ws_ev_opt {
                    let _ = self
                        .broadcast_tx
                        .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
                }
                if let Some(notice_ev) = notice_ev_opt {
                    let _ = self
                        .broadcast_tx
                        .send(DaemonBroadcast::TimelineLive(notice_ev));
                }
            }
            self.send_conflict_steer_if_active(agent_id, msg.clone())
                .await;
            for (other_id, _) in &same_dir_peers {
                self.send_conflict_steer_if_active(other_id, msg.clone())
                    .await;
            }
        }

        if !cross_wt_peers.is_empty()
            && self
                .check_conflict_cooldown(&agent.workspace_id, &format!("xwt:{norm_path}"), 120)
                .await
        {
            let others_str = cross_wt_peers
                .iter()
                .map(|(_, h)| format!("@{h}"))
                .collect::<Vec<_>>()
                .join(", ");
            let msg = format!(
                "Cross-worktree conflict heads-up: @{handle} and {others_str} both edited `{norm_path}` in separate checkouts. Align contracts on `workspace_board` and merge with `handoff_worktree(allow_three_way_conflicts=true)`."
            );
            let ws_id_c = agent.workspace_id.clone();
            let aid_c = agent_id.to_string();
            let handle_c = handle.to_string();
            let msg_c = msg.clone();
            let norm_c = norm_path.clone();
            if let Ok(Some(ev)) = self
                .with_store(move |s| {
                    s.append_workspace_event(
                        &ws_id_c,
                        Some(&aid_c),
                        Some(&handle_c),
                        WorkspaceEventKind::ConflictWarning,
                        &msg_c,
                        Some(&norm_c),
                    )
                    .ok()
                })
                .await
            {
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::WorkspaceEventRecorded(ev));
            }
            self.send_conflict_steer_if_active(agent_id, msg.clone())
                .await;
            for (other_id, _) in &cross_wt_peers {
                self.send_conflict_steer_if_active(other_id, msg.clone())
                    .await;
            }
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
        let aid = agent_id.to_string();
        if let Ok(Some(updated)) = self
            .with_store(move |s| {
                let _ = s.update_agent_status(&aid, &AgentStatus::Closed);
                s.get_agent(&aid).ok().flatten()
            })
            .await
        {
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

    #[cfg(test)]
    pub(crate) async fn insert_pending_permission_for_test(
        &self,
        summary: PendingPermissionSummary,
    ) {
        self.pending_permissions.write().await.insert(
            (summary.agent_id.clone(), summary.request_id.clone()),
            summary,
        );
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
        let aid_get = agent_id.to_string();
        let existing = self
            .try_store(move |s| -> anyhow::Result<AgentSummary> {
                s.get_agent(&aid_get)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid_get}` not found"))
            })
            .await?;

        let _ = self.stop_and_wait_driver(agent_id).await;

        let mut restored_files: u32 = 0;
        let mut notice: Option<String> = None;

        if restore_files {
            let cwd_path = PathBuf::from(&existing.cwd);
            let aid_cp = agent_id.to_string();
            let (checkpoint_oid, from_events) = self
                .try_store(move |s| -> anyhow::Result<_> {
                    let cp = s.get_turn_checkpoint(&aid_cp, to_before_seq)?;
                    let evs = s
                        .fetch_timeline_from(&aid_cp, to_before_seq)
                        .unwrap_or_default();
                    Ok((cp, evs))
                })
                .await?;
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

        let aid_rw = agent_id.to_string();
        let rewound = self
            .try_store(move |s| -> anyhow::Result<_> {
                s.rewind_agent_timeline(&aid_rw, to_before_seq)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid_rw}` not found"))
            })
            .await?;

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

        let aid = agent_id.to_string();
        let (summary, resume_ref) = self
            .try_store(move |s| -> anyhow::Result<_> {
                let summary = s
                    .get_agent(&aid)?
                    .ok_or_else(|| anyhow::anyhow!("agent `{aid}` not found"))?;
                let resume_ref = s.get_persistence_ref(&aid)?;
                Ok((summary, resume_ref))
            })
            .await?;
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
        let primary_sock = self.shim_socket_path("a", &summary.agent_id)?;
        let legacy_sock = self.legacy_shim_socket_path("agent", &summary.agent_id);
        let shim_socket =
            if reattach_existing_shim && !primary_sock.exists() && legacy_sock.exists() {
                legacy_sock
            } else {
                primary_sock
            };

        let persisted_active_turn_id = if reattach_existing_shim {
            match &summary.status {
                AgentStatus::Running { turn_id } => Some(turn_id.clone()),
                _ => None,
            }
        } else {
            None
        };
        if !reattach_existing_shim && resume_session_id.is_none() {
            self.roster_states.write().await.remove(&summary.agent_id);
            self.injected_rule_hashes
                .write()
                .await
                .remove(&summary.agent_id);
        }

        let aid_boot = summary.agent_id.clone();
        let profile_id_boot = summary.profile_id.clone();
        let latest_seq_boot = summary.latest_seq;
        let persisted_tid_boot = persisted_active_turn_id.clone();
        let need_fallback_handoff = explicit_handoff_context.is_none()
            && !reattach_existing_shim
            && (resume_session_id.is_some()
                || fork_from_session_id.is_some()
                || latest_seq_boot > 0);
        let (
            extra_dir_strs,
            profile,
            persisted_running_bg_tasks,
            shim_after_seq,
            persisted_turn_step_seq,
            store_fallback_handoff,
        ) = self
            .with_store(move |s| {
                let extra_dir_strs = s.get_agent_extra_dirs(&aid_boot).unwrap_or_default();
                let profile = profile_id_boot
                    .as_deref()
                    .and_then(|pid| s.get_profile(pid).ok().flatten());
                let persisted_running_bg_tasks: Vec<PersistedBackgroundTask> = s
                    .list_running_background_tool_calls(&aid_boot)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(call_id, detail, info)| PersistedBackgroundTask {
                        call_id,
                        detail,
                        info,
                    })
                    .collect();
                let shim_after_seq = if reattach_existing_shim {
                    match s.get_agent_shim_cursor(&aid_boot) {
                        Some(seq) => Some(seq),
                        // If we are reattaching to an existing shim whose cursor was never persisted
                        // (e.g. daemon crashed/SIGKILLed before flushing or agent predated cursor tracking),
                        // and this agent already has persisted timeline history, tail from the shim's
                        // latest seq (`u64::MAX`) instead of replaying the entire 512-frame ring buffer.
                        None if latest_seq_boot > 0 => Some(u64::MAX),
                        None => None,
                    }
                } else {
                    let _ = s.clear_agent_shim_cursor(&aid_boot);
                    None
                };
                let persisted_turn_step_seq = persisted_tid_boot
                    .as_deref()
                    .map(|tid| s.max_turn_segment_step(&aid_boot, tid))
                    .unwrap_or(0);
                let fallback_handoff = if need_fallback_handoff {
                    Self::build_handoff_context_from_store(s, &aid_boot)
                } else {
                    None
                };
                (
                    extra_dir_strs,
                    profile,
                    persisted_running_bg_tasks,
                    shim_after_seq,
                    persisted_turn_step_seq,
                    fallback_handoff,
                )
            })
            .await?;

        let mut extra_dirs: Vec<PathBuf> = extra_dir_strs
            .into_iter()
            .map(|s| crate::expand_host_path(&s))
            .collect();

        let attachments_dir = self.home_dir.join("attachments");
        let _ = tokio::fs::create_dir_all(&attachments_dir).await;
        if !extra_dirs.contains(&attachments_dir) {
            extra_dirs.push(attachments_dir.clone());
        }

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

        let persisted_bg_wait = matches!(
            &summary.status,
            AgentStatus::AutonomousWakeup { source, .. }
                if source.trim().starts_with("waiting:")
        );

        let shim_seq_tracker = Arc::new(AtomicU64::new(
            shim_after_seq.filter(|&seq| seq < u64::MAX).unwrap_or(0),
        ));
        let fallback_handoff_context = explicit_handoff_context.or(store_fallback_handoff);

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
            runtime_dir: self.claude_runtime_dir(),
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
                                            Box::pin(mgr.ingest_driver_event(
                                                &ag_id,
                                                expected_gen,
                                                *prev,
                                            ))
                                            .await;
                                        }
                                        Box::pin(mgr.ingest_driver_event(
                                            &ag_id,
                                            expected_gen,
                                            *current,
                                        ))
                                        .await;
                                        let cur_seq = shim_seq_bg.load(Ordering::Relaxed);
                                        if cur_seq > last_persisted_shim_seq {
                                            let aid_cur = ag_id.clone();
                                            let _ = mgr
                                                .with_store(move |s| {
                                                    let _ = s.set_agent_shim_cursor(&aid_cur, cur_seq);
                                                })
                                                .await;
                                            last_persisted_shim_seq = cur_seq;
                                        }
                                    }
                                }
                            }
                            None => {
                                if let Some(pending) = stream_buf.take()
                                    && mgr.current_generation(&ag_id).await == expected_gen
                                {
                                    Box::pin(mgr.ingest_driver_event(
                                        &ag_id,
                                        expected_gen,
                                        pending,
                                    ))
                                    .await;
                                }
                                let cur_seq = shim_seq_bg.load(Ordering::Relaxed);
                                if cur_seq > last_persisted_shim_seq {
                                    let aid_cur = ag_id.clone();
                                    let _ = mgr
                                        .with_store(move |s| {
                                            let _ = s.set_agent_shim_cursor(&aid_cur, cur_seq);
                                        })
                                        .await;
                                }
                                break;
                            }
                        }
                    }
                    _ = &mut sleep, if !stream_buf.is_empty() => {
                        if let Some(pending) = stream_buf.take()
                            && mgr.current_generation(&ag_id).await == expected_gen
                        {
                            Box::pin(mgr.ingest_driver_event(&ag_id, expected_gen, pending))
                                .await;
                            let cur_seq = shim_seq_bg.load(Ordering::Relaxed);
                            if cur_seq > last_persisted_shim_seq {
                                let aid_cur = ag_id.clone();
                                let _ = mgr
                                    .with_store(move |s| {
                                        let _ = s.set_agent_shim_cursor(&aid_cur, cur_seq);
                                    })
                                    .await;
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
                let aid = agent_id.to_string();
                let _ = self
                    .with_store(move |s| {
                        let _ = s.set_persistence_ref(&aid, &persistence_ref);
                        if let Some(m) = model
                            && let Ok(Some(mut ag)) = s.get_agent(&aid)
                        {
                            ag.model = Some(m);
                            let _ = s.upsert_agent(&ag, Some(&persistence_ref));
                        }
                    })
                    .await;
                return;
            }
            DriverEvent::SessionTitleUpdated { title } => {
                let clean_title: String = title
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(120)
                    .collect();
                if clean_title.is_empty() {
                    return;
                }
                if let Ok(Some(ag)) = self.store.get_agent(agent_id)
                    && ag.labels.get("orbit.title_locked").map(String::as_str) != Some("1")
                    && ag.title != clean_title
                {
                    let mut labels_patch = HashMap::new();
                    labels_patch.insert("orbit.auto_named".to_string(), "1".to_string());
                    if self
                        .store
                        .update_agent_fields(
                            agent_id,
                            Some(&clean_title),
                            None,
                            None,
                            None,
                            Some(&labels_patch),
                            None,
                            None,
                            None,
                        )
                        .is_ok()
                        && let Ok(Some(updated_ag)) = self.store.get_agent(agent_id)
                    {
                        let _ = self
                            .broadcast_tx
                            .send(DaemonBroadcast::AgentUpdated(updated_ag));
                    }
                }
                return;
            }
            DriverEvent::ProviderSubagentUpsert(desc) => {
                let parent_id = desc.parent_agent_id.clone();
                let is_settled = desc.status != orbit_protocol::ProviderSubagentStatus::Running;
                let desc_db = desc.clone();
                let _ = self
                    .with_store(move |s| {
                        let _ = s.upsert_provider_subagent(&desc_db);
                    })
                    .await;
                let _ = self
                    .broadcast_tx
                    .send(DaemonBroadcast::ProviderSubagentUpdated(desc));
                if is_settled && let Some(timers) = self.conditional_timers().await {
                    let all_settled = self.are_all_subagents_settled_async(&parent_id).await;
                    timers
                        .notify_subagent_settled(&parent_id, all_settled)
                        .await;
                }
                return;
            }
            DriverEvent::ProviderSubagentTimeline {
                subagent_id,
                mut payload,
            } => {
                sanitize_timeline_payload(&mut payload);
                let aid = agent_id.to_string();
                if let Ok((saved, mutated)) = self
                    .try_store(move |s| {
                        s.append_provider_subagent_event_upsert(&aid, &subagent_id, payload)
                    })
                    .await
                    && mutated
                {
                    let _ = self
                        .broadcast_tx
                        .send(DaemonBroadcast::ProviderSubagentTimelineLive(saved));
                }
                return;
            }
            DriverEvent::ReconcileBackgroundTasks(settlements) => {
                let aid = agent_id.to_string();
                if let Ok(Some(ag)) = self
                    .with_store(move |s| {
                        if let Ok(updated) =
                            s.settle_timeline_tool_calls_in_place(&aid, &settlements)
                            && updated > 0
                        {
                            s.get_agent(&aid).ok().flatten()
                        } else {
                            None
                        }
                    })
                    .await
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
                context_window_tokens,
                model,
            } => {
                let recorded_at_ms = Utc::now().timestamp_millis();
                self.turn_usage_recorded
                    .write()
                    .await
                    .insert(agent_id.to_string());
                let telemetry_opt =
                    last_call_prompt_tokens
                        .filter(|&toks| toks > 0)
                        .map(|prompt_tokens| orbit_store::AgentContextTelemetry {
                            prompt_tokens,
                            context_window_tokens: context_window_tokens.filter(|&w| w > 0),
                            recorded_at_ms,
                        });
                if let Some(telemetry) = telemetry_opt {
                    self.last_call_prompt_occupancy
                        .write()
                        .await
                        .insert(agent_id.to_string(), telemetry);
                }
                let store_bg = Arc::clone(&self.store);
                let aid_bg = agent_id.to_string();
                tokio::task::spawn_blocking(move || {
                    if let Some(telemetry) = telemetry_opt {
                        let _ = store_bg.upsert_agent_context_telemetry(&aid_bg, telemetry);
                    }
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
            DriverEvent::PromptDeliveryUpdated {
                turn_id,
                message_ids,
                status,
                blocking_turn_id,
            } => {
                let timestamp_ms = Utc::now().timestamp_millis();
                let mut payloads = Vec::with_capacity(message_ids.len());
                for message_id in message_ids {
                    let wait_ms = {
                        let mut qmap = self.notification_queued_at.write().await;
                        match status {
                            AgentMessageDeliveryStatus::Queued => {
                                if qmap.len() >= 4096
                                    && !qmap.contains_key(&message_id)
                                    && let Some(oldest_k) = qmap
                                        .iter()
                                        .min_by_key(|(_, ts)| *ts)
                                        .map(|(k, _)| k.clone())
                                {
                                    qmap.remove(&oldest_k);
                                }
                                let q = *qmap.entry(message_id.clone()).or_insert(timestamp_ms);
                                Some(timestamp_ms.saturating_sub(q))
                            }
                            AgentMessageDeliveryStatus::Dispatched => qmap
                                .get(&message_id)
                                .map(|q| timestamp_ms.saturating_sub(*q)),
                            AgentMessageDeliveryStatus::Settled
                            | AgentMessageDeliveryStatus::Dropped => qmap
                                .remove(&message_id)
                                .map(|q| timestamp_ms.saturating_sub(q)),
                        }
                    };
                    info!(
                        agent_id = %agent_id,
                        notification_id = %message_id,
                        status = ?status,
                        turn_id = %turn_id,
                        blocking_turn_id = ?blocking_turn_id,
                        wait_ms = ?wait_ms,
                        "Agent notification delivery status updated"
                    );
                    payloads.push(TimelinePayload::AgentMessageDeliveryUpdated {
                        message_id,
                        status,
                        delivery_turn_id: Some(turn_id.clone()),
                        blocking_turn_id: blocking_turn_id.clone(),
                        timestamp_ms,
                    });
                }
                let aid = agent_id.to_string();
                let tid = turn_id;
                if let Ok(saved_evs) = self
                    .with_store(move |s| {
                        let mut out = Vec::new();
                        for payload in payloads {
                            if let Ok((saved_ev, mutated)) =
                                s.append_timeline_event_deduped(&aid, 1, Some(&tid), payload)
                                && mutated
                            {
                                out.push(saved_ev);
                            }
                        }
                        out
                    })
                    .await
                {
                    for saved_ev in saved_evs {
                        let _ = self
                            .broadcast_tx
                            .send(DaemonBroadcast::TimelineLive(saved_ev));
                    }
                }
                return;
            }
            _ => {}
        }

        let mut should_check_auto_archive = false;
        let mut should_drain_queued_prompt = false;
        let mut should_mark_finished = false;
        let mut completed_running_turn = false;
        let mut preempted_running_turn = false;
        let payload = match evt {
            DriverEvent::StatusChanged(status) => {
                self.keep_awake.notify_agent_status(agent_id, &status);
                let aid_prev = agent_id.to_string();
                let prev = self
                    .try_store(move |s| s.get_agent(&aid_prev))
                    .await
                    .ok()
                    .flatten();
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
                let mut should_touch_activity = false;
                let mut next_attention: Option<AttentionState> = None;
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
                            should_touch_activity = true;
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
                        if was_running_turn {
                            completed_running_turn = true;
                        }
                        if was_foreground_busy {
                            should_touch_activity = true;
                            if prev.as_ref().is_some_and(|a| !a.background) {
                                should_mark_finished = true;
                            }
                        }
                    }
                    AgentStatus::AutonomousWakeup { .. } => {
                        self.suppress_auto_drain.write().await.remove(agent_id);
                        should_touch_activity = true;
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
                        should_touch_activity = true;
                        if let Some(ref ag) = prev {
                            if !ag.background {
                                next_attention = Some(AttentionState::Error);
                            }
                            let same_error = matches!(
                                &ag.status,
                                AgentStatus::Error { message: prev_msg, .. } if prev_msg == message
                            );
                            if !same_error {
                                let prov_slug = ag.provider.slug().to_string();
                                let clean_msg = crate::hooks::truncate_hook_preview(message, 500);
                                let notif_payload = serde_json::json!({
                                    "event": "notification",
                                    "kind": "agent_error",
                                    "agent_id": agent_id,
                                    "agent_title": &ag.title,
                                    "workspace_id": &ag.workspace_id,
                                    "cwd": &ag.cwd,
                                    "provider": &prov_slug,
                                    "recoverable": recoverable,
                                    "message": clean_msg,
                                });
                                let _ = self.hooks.dispatch_event_async_with_targets(
                                    orbit_protocol::HookEventKind::Notification,
                                    Some(PathBuf::from(&ag.cwd)),
                                    vec!["agent_error".to_string(), "error".to_string(), prov_slug],
                                    notif_payload,
                                );
                            }
                        }
                        should_check_auto_archive = true;
                    }
                    AgentStatus::AwaitingPermission { request_id, .. } => {
                        let key = (agent_id.to_string(), request_id.clone());
                        let was_auto_resolved = self
                            .guardian_auto_resolved_requests
                            .write()
                            .await
                            .remove(&key);
                        if was_auto_resolved
                            && !self.pending_permissions.read().await.contains_key(&key)
                        {
                            return;
                        }
                        if let Some(ref ag) = prev {
                            if !ag.background {
                                next_attention = Some(AttentionState::Permission);
                            }
                            let same_request = matches!(
                                &ag.status,
                                AgentStatus::AwaitingPermission { request_id: prev_req, .. }
                                    if prev_req == request_id
                            );
                            if !same_request {
                                let perm_title = self
                                    .pending_permissions
                                    .read()
                                    .await
                                    .get(&key)
                                    .map(|p| p.title.clone());
                                let notif_message = match perm_title {
                                    Some(ref t) if !t.trim().is_empty() => format!(
                                        "Awaiting permission: {}",
                                        crate::hooks::truncate_hook_preview(t, 300)
                                    ),
                                    _ => format!("Awaiting permission ({request_id})"),
                                };
                                let prov_slug = ag.provider.slug().to_string();
                                let notif_payload = serde_json::json!({
                                    "event": "notification",
                                    "kind": "awaiting_permission",
                                    "agent_id": agent_id,
                                    "agent_title": &ag.title,
                                    "workspace_id": &ag.workspace_id,
                                    "cwd": &ag.cwd,
                                    "provider": &prov_slug,
                                    "request_id": request_id,
                                    "message": notif_message,
                                });
                                let _ = self.hooks.dispatch_event_async_with_targets(
                                    orbit_protocol::HookEventKind::Notification,
                                    Some(PathBuf::from(&ag.cwd)),
                                    vec!["awaiting_permission".to_string(), prov_slug],
                                    notif_payload,
                                );
                            }
                        }
                    }
                    AgentStatus::Running { turn_id } => {
                        self.suppress_auto_drain.write().await.remove(agent_id);
                        let was_preempted = matches!(
                            &prev,
                            Some(p) if matches!(&p.status, AgentStatus::Running { turn_id: old_tid } if old_tid != turn_id)
                        );
                        if was_preempted {
                            preempted_running_turn = true;
                        } else {
                            self.turn_usage_recorded.write().await.remove(agent_id);
                        }
                        should_touch_activity = true;
                    }
                    _ => {}
                }
                if should_touch_activity || next_attention.is_some() {
                    let aid_st = agent_id.to_string();
                    let _ = self
                        .with_store(move |s| {
                            if should_touch_activity {
                                let _ = s.touch_agent_activity(&aid_st, Utc::now());
                            }
                            if let Some(attn) = next_attention {
                                let _ = s.set_agent_attention(&aid_st, attn);
                            }
                        })
                        .await;
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
                if matches!(status, ToolCallStatus::Completed | ToolCallStatus::Failed) {
                    let post_key = (agent_id.to_string(), call_id.clone());
                    let already_seen = self
                        .post_tool_hook_dispatched
                        .read()
                        .await
                        .contains(&post_key);
                    if !already_seen && let Ok(Some(ag)) = self.store.get_agent(agent_id) {
                        let (primary_tool, hook_targets) =
                            crate::hooks::tool_detail_hook_targets(&detail, "");
                        let (exit_code, output_preview, modified_files) = match &detail {
                            ToolCallDetail::Shell {
                                exit_code,
                                output_preview,
                                ..
                            } => (
                                *exit_code,
                                output_preview
                                    .as_deref()
                                    .map(|s| crate::hooks::truncate_hook_preview(s, 2048)),
                                Vec::new(),
                            ),
                            ToolCallDetail::FileEdit { path, unified_diff } => (
                                None,
                                unified_diff
                                    .as_deref()
                                    .map(|s| crate::hooks::truncate_hook_preview(s, 2048)),
                                crate::hooks::extract_paths_from_unified_diff(
                                    path,
                                    unified_diff.as_deref(),
                                ),
                            ),
                            ToolCallDetail::Generic { arguments_json, .. } => (
                                None,
                                Some(crate::hooks::truncate_hook_preview(arguments_json, 2048)),
                                Vec::new(),
                            ),
                            _ => (None, None, Vec::new()),
                        };
                        let status_str = match status {
                            ToolCallStatus::Completed => "completed",
                            ToolCallStatus::Failed => "failed",
                            ToolCallStatus::Denied => "denied",
                            ToolCallStatus::Running => "running",
                        };
                        let post_payload = serde_json::json!({
                            "event": "post_tool_use",
                            "agent_id": agent_id,
                            "workspace_id": &ag.workspace_id,
                            "cwd": &ag.cwd,
                            "provider": ag.provider.slug(),
                            "call_id": &call_id,
                            "tool_name": &primary_tool,
                            "status": status_str,
                            "exit_code": exit_code,
                            "output_preview": output_preview,
                            "modified_files": modified_files,
                        });
                        // Atomically dedup + enqueue into the bounded mpsc dispatch queue so
                        // a call ID is never falsely marked dispatched if enqueue fails.
                        let mut seen = self.post_tool_hook_dispatched.write().await;
                        if seen.len() >= 2048 {
                            seen.clear();
                        }
                        if !seen.contains(&post_key)
                            && self.hooks.dispatch_event_async_with_targets(
                                orbit_protocol::HookEventKind::PostToolUse,
                                Some(PathBuf::from(&ag.cwd)),
                                hook_targets,
                                post_payload,
                            )
                        {
                            seen.insert(post_key);
                        }
                    }
                }
                if status == ToolCallStatus::Completed {
                    match &detail {
                        ToolCallDetail::FileEdit { path, unified_diff } => {
                            let edited_paths = crate::hooks::extract_paths_from_unified_diff(
                                path,
                                unified_diff.as_deref(),
                            );
                            for edited_path in &edited_paths {
                                self.record_file_edit_and_check_conflict(agent_id, edited_path)
                                    .await;
                            }
                            let aid_edit = agent_id.to_string();
                            if let Ok(Some(ag)) =
                                self.try_store(move |s| s.get_agent(&aid_edit)).await
                            {
                                let target_path = path.clone();
                                let payload = serde_json::json!({
                                    "event": "post_file_edit",
                                    "agent_id": agent_id,
                                    "workspace_id": &ag.workspace_id,
                                    "cwd": &ag.cwd,
                                    "path": &target_path,
                                    "paths": &edited_paths,
                                    "source": "agent_tool",
                                });
                                let _ = self.hooks.dispatch_event_async_with_targets(
                                    orbit_protocol::HookEventKind::PostFileEdit,
                                    Some(PathBuf::from(&ag.cwd)),
                                    edited_paths,
                                    payload,
                                );
                            }
                        }
                        ToolCallDetail::Generic {
                            tool_name,
                            arguments_json,
                        } if crate::artifacts::extract_trusted_mcp_server_from_tool_name(
                            tool_name,
                        )
                        .is_some()
                            && arguments_json.contains("ui://") =>
                        {
                            let mgr = Arc::clone(self);
                            let aid = agent_id.to_string();
                            let tname = tool_name.clone();
                            let payload_json = arguments_json.clone();
                            tokio::spawn(async move {
                                let _ = mgr
                                    .auto_extract_artifacts_from_tool_call_async(
                                        &aid,
                                        &tname,
                                        &payload_json,
                                    )
                                    .await;
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
                mut plan_approval,
            } => {
                // Mark this `(agent_id, request_id)` as actively evaluating BEFORE dispatching
                // PreToolUse, permission rules, or Guardian so no premature external
                // `resolve_permission` call can race ahead and forward an `allow` to the driver.
                let eval_key = (agent_id.to_string(), request_id.clone());
                {
                    self.evaluating_permissions
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .insert(eval_key.clone());
                }
                let mut eval_guard = EvaluatingPermissionGuard {
                    set: Arc::clone(&self.evaluating_permissions),
                    key: Some(eval_key),
                };

                let aid_perm = agent_id.to_string();
                let agent_lookup = self.try_store(move |s| s.get_agent(&aid_perm)).await;
                let agent_opt = match agent_lookup {
                    Ok(ag) => ag,
                    Err(err) => {
                        tracing::warn!(
                            agent_id = %agent_id,
                            error = %err,
                            "Failed to read agent scope during permission ingress; failing closed"
                        );
                        None
                    }
                };
                let mut guardian_assessment = None;

                if plan_approval.is_none() {
                    let in_plan_mode = agent_opt
                        .as_ref()
                        .is_some_and(|ag| matches!(ag.mode, SessionMode::Plan));
                    plan_approval = orbit_protocol::extract_plan_approval_from_detail(
                        &detail,
                        Some(&title),
                        in_plan_mode,
                    );
                }
                if let (Some(mut plan), Some(ag)) = (plan_approval.take(), agent_opt.as_ref()) {
                    let aid_enr = agent_id.to_string();
                    let cwd_enr = PathBuf::from(&ag.cwd);
                    if let Ok(enriched) = self
                        .with_store(move |s| {
                            crate::plan_approval::enrich_plan_approval_from_store(
                                s, &aid_enr, &cwd_enr, &mut plan,
                            );
                            plan
                        })
                        .await
                    {
                        plan_approval = Some(enriched);
                    }
                }

                // Synchronous PreToolUse lifecycle hook gate BEFORE permission rules, Guardian
                // auto-approval, or surfacing the permission request to the operator.
                // CRITICAL SAFETY INVARIANT: a PreToolUse hook returning `Allow` NEVER auto-approves
                // a tool call, NEVER bypasses a human-only `plan_approval.is_some()` gate, and NEVER
                // overrides an explicit user `Deny`. A `Block` decision or unsupported `updated_input`
                // fails closed immediately.
                let mut pre_hook_blocked = false;
                {
                    let (primary_tool, hook_targets) =
                        crate::hooks::tool_detail_hook_targets(&detail, &title);
                    let (_, target_summary) =
                        crate::guardian::summarize_tool_request(&detail, &title);
                    let hook_cwd = agent_opt.as_ref().map(|ag| PathBuf::from(&ag.cwd));
                    let pre_payload = serde_json::json!({
                        "event": "pre_tool_use",
                        "agent_id": agent_id,
                        "workspace_id": agent_opt.as_ref().map(|ag| ag.workspace_id.as_str()),
                        "cwd": agent_opt.as_ref().map(|ag| ag.cwd.as_str()),
                        "provider": agent_opt.as_ref().map(|ag| ag.provider.slug()),
                        "request_id": &request_id,
                        "title": &title,
                        "tool_name": &primary_tool,
                        "tool_detail": &detail,
                        "tool_input": crate::hooks::tool_detail_input_value(&detail),
                        "target_summary": &target_summary,
                        "is_plan_approval": plan_approval.is_some(),
                    });
                    let pre_outcome = self
                        .hooks
                        .dispatch_event_with_targets(
                            orbit_protocol::HookEventKind::PreToolUse,
                            hook_cwd.as_deref(),
                            &hook_targets,
                            pre_payload,
                        )
                        .await;
                    if self.current_generation(agent_id).await != expected_gen {
                        return;
                    }
                    let unsupported_input_rewrite = pre_outcome.decision
                        != orbit_protocol::HookDecision::Block
                        && pre_outcome.updated_input.is_some();
                    if pre_outcome.decision == orbit_protocol::HookDecision::Block
                        || unsupported_input_rewrite
                    {
                        pre_hook_blocked = true;
                        let hook_name = pre_outcome
                            .blocking_hook_name
                            .as_deref()
                            .or_else(|| pre_outcome.records.last().map(|r| r.hook_name.as_str()))
                            .unwrap_or("lifecycle hook");
                        let reason = if unsupported_input_rewrite {
                            "PreToolUse updated_input is not supported for this provider tool (fail-closed)"
                        } else {
                            pre_outcome
                                .reason
                                .as_deref()
                                .unwrap_or("blocked by PreToolUse hook")
                        };
                        let deny_decision =
                            crate::guardian::pick_auto_deny_decision(&allowed_decisions);
                        let resolved_key = (agent_id.to_string(), request_id.clone());
                        {
                            let mut set = self.guardian_auto_resolved_requests.write().await;
                            if set.len() > 512 {
                                set.clear();
                            }
                            set.insert(resolved_key.clone());
                        }
                        if let Some(tx) = self.live_agent_tx(agent_id).await
                            && tx
                                .send(DriverCommand::ResolvePermission {
                                    request_id: request_id.clone(),
                                    decision: deny_decision,
                                })
                                .await
                                .is_ok()
                        {
                            if !self.has_pending_permissions(agent_id).await {
                                let _ = self
                                    .store
                                    .set_agent_attention(agent_id, AttentionState::None);
                            }
                            let notice = TimelinePayload::Notice {
                                level: "warning".into(),
                                message: format!(
                                    "PreToolUse hook `{hook_name}` blocked `{primary_tool}` ({target_summary}): {reason}"
                                ),
                            };
                            if let Ok((saved_ev, mutated)) = self
                                .store
                                .append_timeline_event_deduped(agent_id, 1, None, notice)
                                && mutated
                            {
                                let _ = self
                                    .broadcast_tx
                                    .send(DaemonBroadcast::TimelineLive(saved_ev));
                            }
                            return;
                        }
                        self.guardian_auto_resolved_requests
                            .write()
                            .await
                            .remove(&resolved_key);
                        // Fail-closed: live driver deny dispatch unavailable -> never auto-allow;
                        // surface warning & attach fail-closed assessment.
                        let notice = TimelinePayload::Notice {
                            level: "warning".into(),
                            message: format!(
                                "PreToolUse hook `{hook_name}` blocked `{primary_tool}` ({target_summary}): {reason} (deny dispatch unavailable)"
                            ),
                        };
                        if let Ok((saved_ev, mutated)) = self
                            .store
                            .append_timeline_event_deduped(agent_id, 1, None, notice)
                            && mutated
                        {
                            let _ = self
                                .broadcast_tx
                                .send(DaemonBroadcast::TimelineLive(saved_ev));
                        }
                        guardian_assessment = Some(orbit_protocol::GuardianAssessment {
                            risk_level: orbit_protocol::GuardianRiskLevel::High,
                            decision: orbit_protocol::GuardianDecision::RequireHuman,
                            confidence: 1.0,
                            classifier_tier: "fail_closed".into(),
                            reason: format!(
                                "PreToolUse hook `{hook_name}` blocked `{primary_tool}`: {reason}"
                            ),
                            signals: vec!["pre_tool_use_hook_blocked".into(), "fail_closed".into()],
                            latency_ms: 0,
                        });
                    }
                }

                // Evaluate persisted fine-grained permission rules BEFORE Guardian / human prompting
                // (strictly excluding PreToolUse-blocked and human-only `plan_approval.is_some()` gates).
                // Fail-closed invariant: if the rules store cannot be read, do NOT fall through
                // to Guardian auto-approval (a Deny rule may exist in SQLite).
                let mut rules_store_unavailable = false;
                if !pre_hook_blocked
                    && plan_approval.is_none()
                    && let Some(ref ag) = agent_opt
                {
                    match self.store.list_permission_rules(Some(&ag.cwd)) {
                        Err(err) => {
                            tracing::warn!(
                                agent_id = %agent_id,
                                error = %err,
                                "Permission rules store unavailable on ingress; failing closed to human review"
                            );
                            rules_store_unavailable = true;
                        }
                        Ok(rules) => {
                            if let Some(matched) =
                                crate::permission_rules::evaluate_permission_rules(
                                    &rules,
                                    &crate::permission_rules::PermissionRuleEvalContext {
                                        project_cwd: &ag.cwd,
                                        provider: ag.provider,
                                        title: &title,
                                        detail: &detail,
                                        has_plan_approval: false,
                                    },
                                )
                            {
                                let is_allow = matches!(
                                    matched.rule.decision,
                                    orbit_protocol::PermissionRuleDecision::Allow
                                );
                                let can_dispatch_match = !is_allow
                                    || allowed_decisions.is_empty()
                                    || crate::permission_rules::permits_allow_decision(
                                        &allowed_decisions,
                                    );
                                let wire_decision = if is_allow {
                                    crate::guardian::pick_auto_allow_decision(&allowed_decisions)
                                } else {
                                    crate::guardian::pick_auto_deny_decision(&allowed_decisions)
                                };
                                let resolved_key = (agent_id.to_string(), request_id.clone());
                                if can_dispatch_match {
                                    let mut set =
                                        self.guardian_auto_resolved_requests.write().await;
                                    if set.len() > 512 {
                                        set.clear();
                                    }
                                    set.insert(resolved_key.clone());
                                }
                                if can_dispatch_match
                                    && let Some(tx) = self.live_agent_tx(agent_id).await
                                    && tx
                                        .send(DriverCommand::ResolvePermission {
                                            request_id: request_id.clone(),
                                            decision: wire_decision,
                                        })
                                        .await
                                        .is_ok()
                                {
                                    let matched_at = Utc::now();
                                    let _ = self
                                        .store
                                        .record_permission_rule_match(&matched.rule.id, matched_at);
                                    if !self.has_pending_permissions(agent_id).await {
                                        let _ = self
                                            .store
                                            .set_agent_attention(agent_id, AttentionState::None);
                                    }
                                    let (tool_name, target_summary) =
                                        crate::guardian::summarize_tool_request(&detail, &title);
                                    let decided_at_ms =
                                        u64::try_from(matched_at.timestamp_millis()).unwrap_or(0);
                                    self.guardian
                                        .record_audit_entry(orbit_protocol::GuardianAuditEntry {
                                            id: format!(
                                                "pr_{}",
                                                &Uuid::new_v4().simple().to_string()[..12]
                                            ),
                                            agent_id: agent_id.to_string(),
                                            request_id: request_id.clone(),
                                            tool_name,
                                            target_summary: target_summary.clone(),
                                            assessment: orbit_protocol::GuardianAssessment {
                                                risk_level: if is_allow {
                                                    orbit_protocol::GuardianRiskLevel::Low
                                                } else {
                                                    orbit_protocol::GuardianRiskLevel::High
                                                },
                                                decision: if is_allow {
                                                    orbit_protocol::GuardianDecision::AutoAllow
                                                } else {
                                                    orbit_protocol::GuardianDecision::AutoDeny
                                                },
                                                confidence: 1.0,
                                                classifier_tier: "deterministic".into(),
                                                reason: matched.reason.clone(),
                                                signals: vec![
                                                    "permission_rule".into(),
                                                    matched.rule.scope_kind.as_str().into(),
                                                    matched.rule.source.as_str().into(),
                                                ],
                                                latency_ms: 0,
                                            },
                                            decided_at_ms,
                                        })
                                        .await;
                                    let notice = TimelinePayload::Notice {
                                        level: if is_allow {
                                            "info".into()
                                        } else {
                                            "warning".into()
                                        },
                                        message: if is_allow {
                                            format!(
                                                "Permission rule auto-approved (`{}`): {target_summary}",
                                                matched.rule.pattern
                                            )
                                        } else {
                                            format!(
                                                "Permission rule denied (`{}`): {target_summary}",
                                                matched.rule.pattern
                                            )
                                        },
                                    };
                                    if let Ok((saved_ev, mutated)) = self
                                        .store
                                        .append_timeline_event_deduped(agent_id, 1, None, notice)
                                        && mutated
                                    {
                                        let _ = self
                                            .broadcast_tx
                                            .send(DaemonBroadcast::TimelineLive(saved_ev));
                                    }
                                    return;
                                }
                                self.guardian_auto_resolved_requests
                                    .write()
                                    .await
                                    .remove(&resolved_key);
                            }
                        }
                    }
                }

                // Plan approval gates always require explicit human review and never auto-resolve via Guardian.
                // Fail-closed: if the permission rules store was unreadable, attach a fail-closed
                // assessment and skip Guardian auto-approval regardless of session mode.
                if !pre_hook_blocked && plan_approval.is_none() && rules_store_unavailable {
                    guardian_assessment = Some(orbit_protocol::GuardianAssessment {
                        risk_level: orbit_protocol::GuardianRiskLevel::High,
                        decision: orbit_protocol::GuardianDecision::RequireHuman,
                        confidence: 0.0,
                        classifier_tier: "fail_closed".into(),
                        reason: "Permission rules store unavailable; requires human approval"
                            .into(),
                        signals: vec!["permission_rules_store_error".into(), "fail_closed".into()],
                        latency_ms: 0,
                    });
                } else if !pre_hook_blocked
                    && plan_approval.is_none()
                    && let Some(ref ag) = agent_opt
                    && matches!(ag.mode, SessionMode::Auto)
                {
                    let eval_req = crate::guardian::GuardianEvalRequest {
                        agent_id,
                        request_id: &request_id,
                        title: &title,
                        detail: &detail,
                        workspace_cwd: &ag.cwd,
                        agent_provider: Some(ag.provider),
                    };
                    let assessment = self.guardian.assess_and_record(&eval_req).await;
                    // Re-verify generation and current mode after async classification (TOCTOU guard).
                    if self.current_generation(agent_id).await != expected_gen {
                        return;
                    }
                    let aid_auto = agent_id.to_string();
                    let still_auto = self
                        .try_store(move |s| s.get_agent(&aid_auto))
                        .await
                        .ok()
                        .flatten()
                        .is_some_and(|fresh| matches!(fresh.mode, SessionMode::Auto));
                    let (_, target_summary) =
                        crate::guardian::summarize_tool_request(&detail, &title);

                    if !still_auto {
                        let mut escalated = assessment;
                        escalated.decision = orbit_protocol::GuardianDecision::RequireHuman;
                        escalated.classifier_tier = "fail_closed".into();
                        escalated.reason =
                            "Session mode changed during Guardian evaluation; requires approval"
                                .into();
                        guardian_assessment = Some(escalated);
                    } else {
                        match assessment.decision {
                            orbit_protocol::GuardianDecision::AutoAllow => {
                                let allow_decision =
                                    crate::guardian::pick_auto_allow_decision(&allowed_decisions);
                                let resolved_key = (agent_id.to_string(), request_id.clone());
                                {
                                    let mut set =
                                        self.guardian_auto_resolved_requests.write().await;
                                    if set.len() > 512 {
                                        set.clear();
                                    }
                                    set.insert(resolved_key.clone());
                                }
                                if let Some(tx) = self.live_agent_tx(agent_id).await
                                    && tx
                                        .send(DriverCommand::ResolvePermission {
                                            request_id: request_id.clone(),
                                            decision: allow_decision,
                                        })
                                        .await
                                        .is_ok()
                                {
                                    let clear_attn = !self.has_pending_permissions(agent_id).await;
                                    let notice = TimelinePayload::Notice {
                                        level: "info".into(),
                                        message: format!(
                                            "Guardian auto-approved ({}): {target_summary}",
                                            assessment.risk_level.as_str()
                                        ),
                                    };
                                    let aid_g = agent_id.to_string();
                                    if let Ok(Some(saved_ev)) = self
                                        .with_store(move |s| {
                                            if clear_attn {
                                                let _ = s.set_agent_attention(
                                                    &aid_g,
                                                    AttentionState::None,
                                                );
                                            }
                                            match s.append_timeline_event_deduped(
                                                &aid_g, 1, None, notice,
                                            ) {
                                                Ok((ev, true)) => Some(ev),
                                                _ => None,
                                            }
                                        })
                                        .await
                                    {
                                        let _ = self
                                            .broadcast_tx
                                            .send(DaemonBroadcast::TimelineLive(saved_ev));
                                    }
                                    return;
                                }
                                self.guardian_auto_resolved_requests
                                    .write()
                                    .await
                                    .remove(&resolved_key);
                                // Fail-closed: if auto-resolve dispatch failed, escalate to human.
                                let mut escalated = assessment;
                                escalated.decision = orbit_protocol::GuardianDecision::RequireHuman;
                                escalated.classifier_tier = "fail_closed".into();
                                escalated.reason =
                                    "Auto-approval dispatch unavailable; requires human approval"
                                        .into();
                                guardian_assessment = Some(escalated);
                            }
                            orbit_protocol::GuardianDecision::AutoDeny => {
                                let deny_decision =
                                    crate::guardian::pick_auto_deny_decision(&allowed_decisions);
                                let resolved_key = (agent_id.to_string(), request_id.clone());
                                {
                                    let mut set =
                                        self.guardian_auto_resolved_requests.write().await;
                                    if set.len() > 512 {
                                        set.clear();
                                    }
                                    set.insert(resolved_key.clone());
                                }
                                if let Some(tx) = self.live_agent_tx(agent_id).await
                                    && tx
                                        .send(DriverCommand::ResolvePermission {
                                            request_id: request_id.clone(),
                                            decision: deny_decision,
                                        })
                                        .await
                                        .is_ok()
                                {
                                    let clear_attn = !self.has_pending_permissions(agent_id).await;
                                    let notice = TimelinePayload::Notice {
                                        level: "warning".into(),
                                        message: format!(
                                            "Guardian denied ({}): {}",
                                            assessment.risk_level.as_str(),
                                            assessment.reason
                                        ),
                                    };
                                    let aid_g = agent_id.to_string();
                                    if let Ok(Some(saved_ev)) = self
                                        .with_store(move |s| {
                                            if clear_attn {
                                                let _ = s.set_agent_attention(
                                                    &aid_g,
                                                    AttentionState::None,
                                                );
                                            }
                                            match s.append_timeline_event_deduped(
                                                &aid_g, 1, None, notice,
                                            ) {
                                                Ok((ev, true)) => Some(ev),
                                                _ => None,
                                            }
                                        })
                                        .await
                                    {
                                        let _ = self
                                            .broadcast_tx
                                            .send(DaemonBroadcast::TimelineLive(saved_ev));
                                    }
                                    return;
                                }
                                self.guardian_auto_resolved_requests
                                    .write()
                                    .await
                                    .remove(&resolved_key);
                                // Fail-closed: mirror AutoAllow dispatch fallback for AutoDeny.
                                let mut escalated = assessment;
                                escalated.decision = orbit_protocol::GuardianDecision::RequireHuman;
                                escalated.classifier_tier = "fail_closed".into();
                                escalated.reason =
                                    "Auto-deny dispatch unavailable; requires human approval"
                                        .into();
                                guardian_assessment = Some(escalated);
                            }
                            orbit_protocol::GuardianDecision::RequireHuman => {
                                guardian_assessment = Some(assessment);
                            }
                        }
                    }
                }

                let human_only = plan_approval.is_some()
                    || pre_hook_blocked
                    || guardian_assessment
                        .as_ref()
                        .is_some_and(crate::guardian::assessment_requires_human_only);

                if let Some(ref ag) = agent_opt {
                    self.pending_permissions.write().await.insert(
                        (agent_id.to_string(), request_id.clone()),
                        PendingPermissionSummary {
                            agent_id: agent_id.to_string(),
                            agent_title: ag.title.clone(),
                            workspace_id: ag.workspace_id.clone(),
                            request_id: request_id.clone(),
                            title: title.clone(),
                            detail: detail.clone(),
                            allowed_decisions: allowed_decisions.clone(),
                            guardian_assessment: guardian_assessment.clone(),
                            plan_approval: plan_approval.clone(),
                            human_only,
                            created_at: Utc::now(),
                        },
                    );
                    // Transition atomically: remove from `evaluating_permissions` STRICTLY AFTER
                    // inserting the settled entry into `pending_permissions`.
                    eval_guard.disarm();

                    // Notify active Personal Router for Tier-2 Medium pre-review (Tier-3 High/Critical,
                    // plan approvals, and fail-closed/human-only verdicts stay strictly human-only!)
                    // and mirror event to optional external Hermes webhook.
                    if !ag.is_personal_router()
                        && let Some(router) = self.personal_router().await
                    {
                        let risk_level = guardian_assessment
                            .as_ref()
                            .map(|a| a.risk_level)
                            .unwrap_or(orbit_protocol::GuardianRiskLevel::High);
                        router.emit_external_webhook_event(
                            "worker.permission_checkpoint",
                            serde_json::json!({
                                "agent_id": agent_id,
                                "agent_title": ag.title,
                                "workspace_id": ag.workspace_id,
                                "request_id": request_id,
                                "title": title,
                                "risk_level": risk_level.as_str(),
                                "human_only": human_only,
                            }),
                        );
                        let cfg = router.config().await;
                        if !human_only
                            && matches!(
                                cfg.pre_review_mode,
                                orbit_protocol::RouterPreReviewMode::MediumRiskOnly
                                    | orbit_protocol::RouterPreReviewMode::AssistOnly
                            )
                            && matches!(
                                risk_level,
                                orbit_protocol::GuardianRiskLevel::Low
                                    | orbit_protocol::GuardianRiskLevel::Medium
                            )
                            && let Some(router_id) = router.active_router_agent_id().await
                            && router_id != agent_id
                            && let Some(delivery) = self.message_delivery().await
                        {
                            let body = format!(
                                "Worker `{}` ({}) requests Tier-2 ({}) permission `{}`: {}.\n\
                                 Evaluate whether this aligns with the user's stated intent. \
                                 If aligned and mode allows, call `orbit_control(action=\"respond_permission\", agent_id=\"{}\", request_id=\"{}\", decision=\"allow\")`; \
                                 otherwise deny or summarize for the human.",
                                ag.title,
                                agent_id,
                                risk_level.as_str(),
                                request_id,
                                title,
                                agent_id,
                                request_id
                            );
                            let _ = delivery
                                .enqueue(crate::messaging::QueuedAgentMessage {
                                    recipient_id: router_id,
                                    message_id: format!("pr_{}", Uuid::new_v4().simple()),
                                    source: crate::messaging::AgentPromptSource {
                                        kind: AgentMessageKind::FinishNotification,
                                        source_agent_id: Some(agent_id.to_string()),
                                        source_host_id: Some(self.host_id.clone()),
                                        source_title: Some(ag.title.clone()),
                                        event: Some("permission_pre_review".to_string()),
                                        permission_context: Some(
                                            orbit_protocol::PermissionCheckpointRef {
                                                agent_id: agent_id.to_string(),
                                                request_id: request_id.clone(),
                                                host_id: Some(self.host_id.clone()),
                                                title: title.clone(),
                                                detail: detail.clone(),
                                                allowed_decisions: allowed_decisions.clone(),
                                            },
                                        ),
                                    },
                                    body,
                                    active_turn_behavior: ActiveTurnBehavior::Queue,
                                    session_mode: None,
                                    unarchive: false,
                                })
                                .await;
                        }
                    }
                }
                TimelinePayload::PermissionRequest {
                    request_id,
                    title,
                    detail,
                    allowed_decisions,
                    guardian_assessment,
                    plan_approval,
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
                } else {
                    tracing::info!(
                        agent_id = %agent_id,
                        exit_code = ?code,
                        "Agent driver process exited cleanly"
                    );
                }
                self.clear_pending_requests(agent_id).await;
                self.post_tool_hook_dispatched
                    .write()
                    .await
                    .retain(|(aid, _)| aid != agent_id);
                self.turn_usage_recorded.write().await.remove(agent_id);
                self.keep_awake
                    .notify_agent_status(agent_id, &AgentStatus::Closed);
                let aid_ex = agent_id.to_string();
                if let Ok(Some(ag)) = self.try_store(move |s| s.get_agent(&aid_ex)).await {
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
                    let _ = self.hooks.dispatch_event_async_with_targets(
                        orbit_protocol::HookEventKind::SessionEnd,
                        Some(PathBuf::from(&ag.cwd)),
                        vec![prov_slug],
                        payload,
                    );
                }
                should_check_auto_archive = true;
                TimelinePayload::StatusChanged {
                    status: AgentStatus::Closed,
                }
            }
            DriverEvent::SessionInitialized { .. }
            | DriverEvent::SessionTitleUpdated { .. }
            | DriverEvent::ProviderSubagentUpsert(_)
            | DriverEvent::ProviderSubagentTimeline { .. }
            | DriverEvent::ReconcileBackgroundTasks(_)
            | DriverEvent::TurnUsage { .. }
            | DriverEvent::PromptDeliveryUpdated { .. } => unreachable!(),
        };

        let is_status_change = matches!(payload, TimelinePayload::StatusChanged { .. });
        let mut payload = payload;
        sanitize_timeline_payload(&mut payload);
        let has_pending_perms = if is_status_change && should_drain_queued_prompt {
            self.has_pending_permissions(agent_id).await
        } else {
            false
        };
        let aid_save = agent_id.to_string();
        let Ok((saved_ev_opt, seq_bump_ag, status_ag_opt, drained_prompt)) = self
            .with_store(move |s| {
                let mut saved_ev_opt = None;
                let mut seq_bump_ag = None;
                if let Ok((saved_ev, mutated)) =
                    s.append_timeline_event_deduped(&aid_save, 1, None, payload)
                    && mutated
                {
                    let saved_seq = saved_ev.seq;
                    saved_ev_opt = Some(saved_ev);
                    if !is_status_change
                        && let Ok(Some(ag)) = s.get_agent(&aid_save)
                        && saved_seq < ag.latest_seq
                    {
                        seq_bump_ag = Some(ag);
                    }
                }
                if !is_status_change {
                    return (saved_ev_opt, seq_bump_ag, None, None);
                }
                let Some(mut ag) = s.get_agent(&aid_save).ok().flatten() else {
                    return (saved_ev_opt, seq_bump_ag, None, None);
                };
                let mut peeked_prompt_id: Option<String> = None;
                if should_drain_queued_prompt
                    && ag.archived_at.is_none()
                    && !has_pending_perms
                    && let Ok(Some(next_item)) = s.peek_next_queued_prompt(&aid_save)
                {
                    // Peek read-only inside the cancellable ingestion await so aborting this
                    // task (e.g. via `stop_and_wait_driver`) can NEVER lose a queued user prompt.
                    peeked_prompt_id = Some(next_item.queue_id);
                }
                // Only emit `AttentionState::Finished` when no queued follow-up is about to run immediately.
                if should_mark_finished && peeked_prompt_id.is_none() {
                    let _ = s.set_agent_attention(&aid_save, AttentionState::Finished);
                    ag.attention = AttentionState::Finished;
                }
                (saved_ev_opt, seq_bump_ag, Some(ag), peeked_prompt_id)
            })
            .await
        else {
            return;
        };
        if let Some(saved_ev) = saved_ev_opt {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::TimelineLive(saved_ev));
        }
        if let Some(ag) = seq_bump_ag {
            let _ = self.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));
        }
        if let Some(ag) = status_ag_opt {
            let auto_arch = ag.auto_archive && ag.archived_at.is_none() && drained_prompt.is_none();
            let ws_id = ag.workspace_id.clone();
            let finished_idle = should_check_auto_archive && matches!(ag.status, AgentStatus::Idle);
            let ag_handle = ag.handle.clone();
            let ag_bg = ag.background;
            let ag_parent_id = ag.parent_agent_id.clone();
            let ag_settled = ag.archived_at.is_some()
                || matches!(
                    ag.status,
                    AgentStatus::Idle | AgentStatus::Error { .. } | AgentStatus::Closed
                );
            let ag_is_subagent = ag_parent_id.is_some();
            let ag_needs_auto_naming = completed_running_turn
                && !ag_is_subagent
                && !crate::workspace_organizer::is_auto_named(&ag.labels);
            if completed_running_turn || preempted_running_turn {
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
                            let slice = if preempted_running_turn {
                                if let Some(last_prompt_idx) = events.iter().rposition(|ev| {
                                    matches!(ev.payload, TimelinePayload::UserPrompt { .. })
                                }) {
                                    let has_turn_output_after =
                                        events[last_prompt_idx + 1..].iter().any(|ev| {
                                            matches!(
                                                ev.payload,
                                                TimelinePayload::AssistantMessage { .. }
                                                    | TimelinePayload::AssistantThinking { .. }
                                                    | TimelinePayload::ToolCall { .. }
                                            )
                                        });
                                    if !has_turn_output_after && last_prompt_idx > 0 {
                                        &events[..last_prompt_idx]
                                    } else {
                                        &events[..]
                                    }
                                } else {
                                    &events[..]
                                }
                            } else {
                                &events[..]
                            };
                            let (input_tokens, output_tokens) =
                                crate::usage::estimate_fallback_turn_tokens(slice);
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
                let prov_slug = ag.provider.slug().to_string();
                let payload = serde_json::json!({
                    "event": "turn_stop",
                    "agent_id": agent_id,
                    "workspace_id": &ag.workspace_id,
                    "cwd": &ag.cwd,
                    "provider": &prov_slug,
                    "title": &ag.title,
                });
                let _ = self.hooks.dispatch_event_async_with_targets(
                    orbit_protocol::HookEventKind::TurnStop,
                    Some(PathBuf::from(&ag.cwd)),
                    vec![prov_slug.clone()],
                    payload,
                );

                // Dispatch `Notification` hook on foreground turn completion with a brief summary.
                if !ag.background && drained_prompt.is_none() {
                    let store_bg = Arc::clone(&self.store);
                    let aid_bg = agent_id.to_string();
                    let aid_payload = agent_id.to_string();
                    let ws_bg = ag.workspace_id.clone();
                    let cwd_str = ag.cwd.clone();
                    let cwd_hook = PathBuf::from(&ag.cwd);
                    let title_bg = ag.title.clone();
                    let task_summary_fallback = ag.task_summary.clone();
                    let prov_bg = prov_slug;
                    let _ = self.hooks.spawn_bounded_async(
                        orbit_protocol::HookEventKind::Notification,
                        move |hooks| async move {
                            let summary = tokio::task::spawn_blocking(move || {
                                if let Ok(events) = store_bg.fetch_recent_timeline(&aid_bg, 20) {
                                    for ev in events.iter().rev() {
                                        if let TimelinePayload::AssistantMessage { text, .. } =
                                            &ev.payload
                                            && !text.trim().is_empty()
                                        {
                                            return crate::hooks::truncate_hook_preview(text, 300);
                                        }
                                    }
                                }
                                task_summary_fallback
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(|s| crate::hooks::truncate_hook_preview(s, 300))
                                    .unwrap_or_else(|| "Turn completed".to_string())
                            })
                            .await
                            .unwrap_or_else(|_| "Turn completed".to_string());

                            let notif_payload = serde_json::json!({
                                "event": "notification",
                                "kind": "turn_completed",
                                "agent_id": &aid_payload,
                                "agent_title": &title_bg,
                                "workspace_id": &ws_bg,
                                "cwd": &cwd_str,
                                "provider": &prov_bg,
                                "message": summary,
                            });
                            let _ = hooks
                                .dispatch_event_with_targets(
                                    orbit_protocol::HookEventKind::Notification,
                                    Some(&cwd_hook),
                                    &["turn_completed".to_string(), prov_bg],
                                    notif_payload,
                                )
                                .await;
                        },
                    );
                }
            }
            let _ = self.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));

            if ag_settled && let Some(timers) = self.conditional_timers().await {
                if let Some(ref parent_id) = ag_parent_id {
                    let all_settled = self.are_all_subagents_settled_async(parent_id).await;
                    timers.notify_subagent_settled(parent_id, all_settled).await;
                }
                if !ag_bg && !ag_is_subagent {
                    timers.notify_workspace_peer_settled(&ws_id, agent_id).await;
                }
            }

            if finished_idle && !ag_bg {
                if !ag_is_subagent {
                    let ws_id_fin = ws_id.clone();
                    let aid_fin = agent_id.to_string();
                    let who = ag_handle.clone().unwrap_or_else(|| "peer".to_string());
                    if let Ok(Some(ev)) = self
                        .with_store(move |s| {
                            let ws_opt = s.get_workspace(&ws_id_fin).ok().flatten();
                            let is_curated_ws = ws_opt.as_ref().is_some_and(|w| w.is_curated);
                            if !is_curated_ws {
                                return None;
                            }
                            let peer_count = s
                                .list_agents_filtered(Some(&ws_id_fin), false, false, None)
                                .map(|p| {
                                    p.into_iter()
                                        .filter(|a| a.parent_agent_id.is_none())
                                        .count()
                                })
                                .unwrap_or(0);
                            if peer_count < 2 {
                                return None;
                            }
                            let snippet = s
                                .last_assistant_message_text(&aid_fin)
                                .ok()
                                .flatten()
                                .map(|text| {
                                    let one_line: String =
                                        text.split_whitespace().collect::<Vec<_>>().join(" ");
                                    one_line.chars().take(110).collect::<String>()
                                })
                                .unwrap_or_else(|| "finished turn".into());
                            s.append_workspace_event(
                                &ws_id_fin,
                                Some(&aid_fin),
                                Some(&who),
                                WorkspaceEventKind::PeerFinished,
                                &format!("@{who} finished: {snippet}"),
                                None,
                            )
                            .ok()
                        })
                        .await
                    {
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

            if let Some(peeked_queue_id) = drained_prompt {
                let mgr = Arc::clone(self);
                let aid = agent_id.to_string();
                tokio::spawn(async move {
                    let aid_claim = aid.clone();
                    let qid_claim = peeked_queue_id.clone();
                    let claimed = mgr
                        .with_store(move |s| {
                            let Some(ag) = s.get_agent(&aid_claim).ok().flatten() else {
                                return (None, None);
                            };
                            if ag.archived_at.is_some() {
                                return (None, None);
                            }
                            let item = s.take_queued_prompt(&aid_claim, &qid_claim).ok().flatten();
                            if item.is_none() {
                                return (None, None);
                            }
                            let _ = s.set_agent_attention(&aid_claim, AttentionState::None);
                            let _ = s.touch_agent_activity(&aid_claim, Utc::now());
                            let refreshed = s.get_agent(&aid_claim).ok().flatten();
                            (item, refreshed)
                        })
                        .await
                        .unwrap_or((None, None));
                    let (Some(next_item), refreshed_ag) = claimed else {
                        return;
                    };
                    if let Some(refreshed) = refreshed_ag {
                        let _ = mgr
                            .broadcast_tx
                            .send(DaemonBroadcast::AgentUpdated(refreshed));
                    }
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
                    let ws_id_chk = ws_id.clone();
                    let should_archive_wt = mgr
                        .with_store(move |s| {
                            s.get_workspace(&ws_id_chk)
                                .ok()
                                .flatten()
                                .is_some_and(|ws| {
                                    ws.is_worktree
                                        && s.list_agents_filtered(
                                            Some(&ws_id_chk),
                                            false,
                                            true,
                                            None,
                                        )
                                        .map(|a| a.is_empty())
                                        .unwrap_or(false)
                                })
                        })
                        .await
                        .unwrap_or(false);
                    if should_archive_wt {
                        let _ = mgr.archive_workspace(&ws_id).await;
                    }
                });
            }
        }
    }

    // --- Detached PTY Terminals ---

    /// Return the latest provider-reported single-call context telemetry observed in-memory
    /// for `agent_id`, if available (caller falls back to persisted SQLite telemetry on restart).
    pub async fn latest_single_call_prompt_tokens(
        &self,
        agent_id: &str,
    ) -> Option<orbit_store::AgentContextTelemetry> {
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
        let shell = crate::platform::default_shell_program();
        let sock_path = self.shim_socket_path("p", &term_id)?;
        let candidate_cwd = crate::expand_host_path(&cwd);
        let resolved_cwd = if candidate_cwd.is_dir() {
            candidate_cwd
        } else {
            crate::expand_host_path("~")
        };
        let resolved_cwd_str = resolved_cwd.to_string_lossy().to_string();

        let mut env = env;
        let (program, args) = if let Some(cmd_line) = initial_command {
            let has_cached_env = orbit_providers::merge_cached_shell_env_into_map(&mut env);
            crate::platform::shell_command_for_platform(&cmd_line, !has_cached_env)
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
            cleanup_paths: Vec::new(),
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
        let summary_db = summary.clone();
        self.try_store(move |s| s.upsert_terminal(&summary_db))
            .await?;
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
            let tid_del = tid.clone();
            let _ = mgr.try_store(move |s| s.delete_terminal(&tid_del)).await;
            for mut stopped in mgr.scripts.unregister_by_terminal_id(&tid).await {
                stopped.info.status = WorkspaceScriptStatus::Stopped;
                stopped.info.terminal_id = None;
                let _ = mgr
                    .broadcast_tx
                    .send(DaemonBroadcast::WorkspaceScriptUpdated(stopped.info));
            }
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
        for mut stopped in self.scripts.unregister_by_terminal_id(terminal_id).await {
            stopped.info.status = WorkspaceScriptStatus::Stopped;
            stopped.info.terminal_id = None;
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::WorkspaceScriptUpdated(stopped.info));
        }
        let tid = terminal_id.to_string();
        self.try_store(move |s| s.delete_terminal(&tid)).await?;
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

static ORBIT_EXE_OVERRIDE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Override the resolved `orbit` executable path used when spawning detached `orbit shim`
/// supervisors (primarily for integration tests via `env!("CARGO_BIN_EXE_orbit")`).
pub fn set_orbit_exe_override(path: PathBuf) {
    let _ = ORBIT_EXE_OVERRIDE.set(path);
}

fn resolve_orbit_exe() -> PathBuf {
    if let Some(overridden) = ORBIT_EXE_OVERRIDE.get()
        && overridden.is_file()
    {
        return overridden.clone();
    }
    if let Ok(explicit) = std::env::var("ORBIT_BIN")
        && !explicit.trim().is_empty()
    {
        return PathBuf::from(explicit);
    }
    if let Ok(cargo_bin) = std::env::var("CARGO_BIN_EXE_orbit")
        && !cargo_bin.trim().is_empty()
    {
        let p = PathBuf::from(cargo_bin);
        if p.is_file() {
            return p;
        }
    }
    if let Ok(cur) = std::env::current_exe() {
        let is_orbit_bin = cur
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s == "orbit");
        if is_orbit_bin {
            return cur;
        }
        let orbit_bin_name = format!("orbit{}", std::env::consts::EXE_SUFFIX);
        let mut anc = cur.parent();
        while let Some(dir) = anc {
            let candidate = dir.join(&orbit_bin_name);
            if candidate.is_file() {
                return candidate;
            }
            anc = dir.parent();
        }
        return cur;
    }
    PathBuf::from(format!("orbit{}", std::env::consts::EXE_SUFFIX))
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

    #[tokio::test]
    async fn resolve_peer_in_workspace_resolves_historical_handle_alias() {
        let tmp_root =
            std::env::temp_dir().join(format!("orbit_alias_res_{}", Uuid::new_v4().simple()));
        let home_dir = tmp_root.join("orbit_home");
        let repo_dir = tmp_root.join("repo");
        std::fs::create_dir_all(&repo_dir).expect("mkdir repo");
        let (orch, store) = setup_test_orchestrator(&home_dir).await;
        let repo_str = repo_dir.to_string_lossy().to_string();
        seed_source_conversation(&store, "ws_alias_res", &repo_str, "ag_perf", None);
        store
            .set_agent_handle("ag_perf", Some("我这边对orbit"))
            .expect("initial handle");
        store
            .set_agent_semantic_identity("ag_perf", Some("大会话性能优化"), None, None)
            .expect("rename");

        let by_old = orch
            .resolve_peer_in_workspace("ws_alias_res", "@我这边对orbit")
            .expect("resolve old handle")
            .expect("peer found by historical alias");
        assert_eq!(by_old.agent_id, "ag_perf");
        assert_eq!(by_old.handle.as_deref(), Some("大会话性能优化"));

        let by_new = orch
            .resolve_peer_in_workspace("ws_alias_res", "大会话性能优化")
            .expect("resolve new handle")
            .expect("peer found by current handle");
        assert_eq!(by_new.agent_id, "ag_perf");

        orch.shutdown_worktree_pool().await;
        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[tokio::test]
    async fn fork_agent_new_worktree_in_curated_workspace_stays_in_mission_workspace() {
        let tmp_root =
            std::env::temp_dir().join(format!("orbit_curated_wt_{}", Uuid::new_v4().simple()));
        let home_dir = tmp_root.join("orbit_home");
        let repo_dir = tmp_root.join("repo");
        std::fs::create_dir_all(&repo_dir).expect("mkdir repo");

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
        std::fs::write(repo_dir.join("README.md"), "# Curated Fork Test\n").expect("write readme");
        run_git(&["add", "README.md"]);
        run_git(&["commit", "-m", "initial commit"]);

        let (orch, store) = setup_test_orchestrator(&home_dir).await;
        let repo_str = repo_dir.to_string_lossy().to_string();
        seed_source_conversation(&store, "ws_curated_wt", &repo_str, "ag_src", None);
        store
            .set_workspace_curated("ws_curated_wt", true)
            .expect("mark curated");

        let out = orch
            .fork_agent(ForkAgentSpec {
                source_agent_id: "ag_src".into(),
                up_to_seq: None,
                target: ForkTarget::NewWorktree {
                    branch: Some("feat/parallel-b".into()),
                    base_ref: Some("main".into()),
                },
                title: None,
                initial_prompt: None,
            })
            .await
            .expect("curated worktree fork");

        assert!(
            out.workspace.is_none(),
            "curated workspace worktree fork must stay inside the same Mission Workspace"
        );
        assert_eq!(out.agent.workspace_id, "ws_curated_wt");
        assert_ne!(out.agent.cwd, repo_str);
        assert!(Path::new(&out.agent.cwd).join("README.md").is_file());
        assert_eq!(
            out.agent
                .labels
                .get("orbit.worktree_branch")
                .map(String::as_str),
            Some("feat/parallel-b")
        );
        assert_eq!(
            out.agent
                .labels
                .get("orbit.worktree_source_cwd")
                .map(String::as_str),
            Some(repo_str.as_str())
        );

        let ws_events = store
            .list_workspace_events("ws_curated_wt", 20)
            .expect("ws events");
        assert!(
            ws_events.iter().any(|e| {
                e.kind == WorkspaceEventKind::PeerJoined
                    && e.agent_id.as_deref() == Some(out.agent.agent_id.as_str())
                    && e.summary.contains("feat/parallel-b")
            }),
            "PeerJoined event must mention isolated worktree branch: {ws_events:?}"
        );

        let preview = orch
            .preview_worktree_handoff_with_agent(
                "ws_curated_wt",
                Some(&out.agent.agent_id),
                WorktreeHandoffDirection::WorktreeToLocal,
                None,
                true,
            )
            .await
            .expect("handoff preview for curated worktree peer");
        assert!(preview.can_apply_cleanly);

        let wt_cwd = PathBuf::from(&out.agent.cwd);
        orch.archive_agent(&out.agent.agent_id)
            .await
            .expect("archive forked worktree peer");
        assert!(
            !wt_cwd.exists(),
            "archiving peer-owned isolated worktree must clean up worktree directory"
        );

        orch.shutdown_worktree_pool().await;
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
            let exe = if cfg!(windows) {
                std::env::current_exe().expect("current test exe")
            } else if Path::new("/usr/bin/true").exists() {
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
            vec!["config", "core.autocrlf", "false"],
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
                source_agent_id: None,
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
                source_agent_id: None,
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
            vec!["config", "core.autocrlf", "false"],
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
                source_agent_id: None,
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

    #[tokio::test]
    async fn guardian_auto_resolve_suppresses_stale_awaiting_permission_and_mirrors_deny_fallback()
    {
        let home_dir = std::env::temp_dir().join(format!(
            "orbit-guardian-mgr-test-{}",
            Uuid::new_v4().simple()
        ));
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("create ws");
        let store = Arc::new(OrbitStore::open_in_memory().expect("store"));
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_test".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            crate::hooks::HooksEngine::new(home_dir.clone()),
        )
        .await;

        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_g1".into(),
                title: "Repo".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
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
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_g1".into(),
                    workspace_id: "ws_g1".into(),
                    cwd: ws_dir.to_string_lossy().to_string(),
                    title: "Guardian Agent".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Auto,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
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
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        // 1. Live driver receives AutoAllow resolve, and trailing AwaitingPermission does NOT set Permission attention
        let (cmd_tx, mut cmd_rx) = mpsc::channel(8);
        let agent_gen = orch.bump_generation("ag_g1").await;
        let dummy_task = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_g1".into(),
            LiveAgentEntry {
                generation: agent_gen,
                cmd_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy_task.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );
        orch.ingest_driver_event(
            "ag_g1",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "perm_low_1".into(),
                title: "Run tests".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        match cmd_rx.try_recv().expect("auto allow dispatched") {
            DriverCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                assert_eq!(request_id, "perm_low_1");
                assert_eq!(decision, "allow");
            }
            other => panic!("unexpected driver command: {other:?}"),
        }
        // Trailing StatusChanged(AwaitingPermission) for the auto-resolved request must be suppressed
        orch.ingest_driver_event(
            "ag_g1",
            agent_gen,
            DriverEvent::StatusChanged(AgentStatus::AwaitingPermission {
                turn_id: "t1".into(),
                request_id: "perm_low_1".into(),
            }),
        )
        .await;
        let ag_after = store.get_agent("ag_g1").expect("get").expect("exists");
        assert_eq!(ag_after.attention, AttentionState::None);

        // 2. AutoDeny dispatch failure (no live driver) falls back to fail_closed RequireHuman
        let _ = orch
            .guardian
            .update_config(orbit_protocol::GuardianPolicyConfig {
                blocked_command_patterns: vec!["deny:cargo test".into()],
                ..orbit_protocol::GuardianPolicyConfig::default()
            })
            .await
            .expect("update cfg");
        orch.live_agents.write().await.remove("ag_g1");
        orch.ingest_driver_event(
            "ag_g1",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "perm_deny_fail".into(),
                title: "Run tests".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        let pending = orch.list_pending_permissions(Some("ag_g1")).await;
        assert_eq!(pending.len(), 1);
        let assess = pending[0]
            .guardian_assessment
            .as_ref()
            .expect("guardian assessment attached");
        assert_eq!(
            assess.decision,
            orbit_protocol::GuardianDecision::RequireHuman
        );
        assert_eq!(assess.classifier_tier, "fail_closed");

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn startup_mcp_reaper_protects_live_transient_and_archived_uncertain_agents() {
        let short_id: String = Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let home_dir = std::env::temp_dir().join(format!("od_mr_{short_id}"));
        let _ = std::fs::remove_dir_all(&home_dir);
        let ws_dir = home_dir.join("repo");
        let runtime_dir = home_dir.join("runtime");
        std::fs::create_dir_all(&ws_dir).expect("create ws");
        std::fs::create_dir_all(&runtime_dir).expect("create runtime");

        let store = Arc::new(OrbitStore::open_in_memory().expect("store"));
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_test".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            crate::hooks::HooksEngine::new(home_dir.clone()),
        )
        .await;

        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_mr1".into(),
                title: "Repo".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
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
            .expect("upsert ws");

        let mk_agent =
            |id: &str, provider: ProviderKind, archived: bool, acp_id: Option<&str>| AgentSummary {
                agent_id: id.into(),
                workspace_id: "ws_mr1".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
                title: id.into(),
                provider,
                model: None,
                effort: None,
                fast_mode: false,
                mode: SessionMode::Auto,
                status: AgentStatus::Idle,
                parent_agent_id: None,
                parent_host_id: None,
                latest_seq: 0,
                timeline_epoch: 0,
                created_at: now,
                updated_at: now,
                archived_at: archived.then_some(now),
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
                handle: None,
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: None,
                forked_at_seq: None,
                acp_agent_id: acp_id.map(ToOwned::to_owned),
            };

        // 1. Live shim whose reattach fails transiently (nonexistent ACP agent executable).
        store
            .upsert_agent(
                &mk_agent(
                    "ag_live_fail",
                    ProviderKind::Acp,
                    false,
                    Some("nonexistent-acp-xyz"),
                ),
                None,
            )
            .expect("upsert live_fail");
        let live_sock = orch
            .shim_socket_path("a", "ag_live_fail")
            .expect("live sock path");
        let _live_listener = orbit_shim::ShimListener::bind(&live_sock)
            .await
            .expect("bind live mock shim");

        // 2. Archived agent whose `.pid` sidecar is still present (uncertain liveness).
        store
            .upsert_agent(
                &mk_agent("ag_arch_pid", ProviderKind::ClaudeCode, true, None),
                None,
            )
            .expect("upsert arch_pid");
        let arch_sock = orch
            .shim_socket_path("a", "ag_arch_pid")
            .expect("arch sock path");
        std::fs::write(
            orbit_shim::pid_file_path(&arch_sock),
            "{\"shim_pid\":999999}",
        )
        .expect("write arch pid sidecar");

        // 3. Dead agent with neither socket nor pid sidecar.
        store
            .upsert_agent(
                &mk_agent("ag_dead", ProviderKind::ClaudeCode, false, None),
                None,
            )
            .expect("upsert dead");

        // Create pre-snapshotted Claude MCP config files for all three.
        let mk_mcp = |aid: &str| {
            let p = runtime_dir.join(format!("claude-mcp-{aid}-{}.json", Uuid::new_v4()));
            std::fs::write(&p, "{\"mcpServers\":{}}").expect("write mcp");
            p
        };
        let live_fail_cfg = mk_mcp("ag_live_fail");
        let arch_pid_cfg = mk_mcp("ag_arch_pid");
        let dead_cfg = mk_mcp("ag_dead");

        let candidates = orbit_providers::snapshot_claude_mcp_config_files(&runtime_dir);
        let protected = orch
            .reattach_or_recover_agents()
            .await
            .expect("store query succeeded");
        assert!(
            protected.contains("ag_live_fail"),
            "agent with live shim must stay protected even when reattach fails transiently"
        );
        assert!(
            protected.contains("ag_arch_pid"),
            "archived agent with remaining pid sidecar must stay protected"
        );
        assert!(
            !protected.contains("ag_dead"),
            "dead agent with no socket/pid must be eligible for stale MCP reaping"
        );

        let reaped = orbit_providers::reap_snapshotted_claude_mcp_configs(&candidates, &protected);
        assert_eq!(reaped, 1);
        assert!(live_fail_cfg.exists());
        assert!(arch_pid_cfg.exists());
        assert!(!dead_cfg.exists());

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[tokio::test]
    async fn peer_worktree_handoff_resolves_agent_worktree_and_serializes_concurrent_merges() {
        let root = std::env::temp_dir().join(format!("orbit_peer_wt_merge_{}", Uuid::new_v4()));
        let repo_dir = root.join("repo");
        let wt_a = root.join("wt-a");
        let wt_b = root.join("wt-b");
        std::fs::create_dir_all(&repo_dir).expect("mkdir repo");

        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "core.autocrlf", "false"],
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
        std::fs::write(repo_dir.join("shared.txt"), "line1\nline2\nline3\n")
            .expect("write shared.txt");
        for args in [vec!["add", "-A"], vec!["commit", "-q", "-m", "init"]] {
            let st = tokio::process::Command::new("git")
                .args(&args)
                .current_dir(&repo_dir)
                .status()
                .await
                .expect("git commit");
            assert!(st.success());
        }
        for (wt, br) in [(&wt_a, "feat/a"), (&wt_b, "feat/b")] {
            let st = tokio::process::Command::new("git")
                .args(["worktree", "add", "-q", "-b", br])
                .arg(wt)
                .arg("main")
                .current_dir(&repo_dir)
                .status()
                .await
                .expect("git worktree add");
            assert!(st.success());
        }

        let (home_dir, store, orch) = test_orchestrator("peer_wt_handoff").await;
        let ws = orch
            .ensure_workspace(repo_dir.to_str().expect("utf8"), None, None, None)
            .await
            .expect("ensure workspace");
        let _ = store.set_workspace_curated(&ws.workspace_id, true);

        let now = Utc::now();
        for (aid, handle, wt) in [("ag_wt_a", "feat_a", &wt_a), ("ag_wt_b", "feat_b", &wt_b)] {
            let mut labels = HashMap::new();
            labels.insert(
                "orbit.worktree_source_cwd".to_string(),
                repo_dir.to_string_lossy().to_string(),
            );
            store
                .upsert_agent(
                    &AgentSummary {
                        agent_id: aid.into(),
                        workspace_id: ws.workspace_id.clone(),
                        cwd: wt.to_string_lossy().to_string(),
                        title: handle.into(),
                        provider: ProviderKind::ClaudeCode,
                        model: None,
                        effort: None,
                        fast_mode: false,
                        mode: SessionMode::FullAccess,
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
                        labels,
                        attention: AttentionState::None,
                        last_activity_at: Some(now),
                        queued_prompts: Vec::new(),
                        read_seq: 0,
                        is_pinned: false,
                        pinned_order: 0,
                        handle: Some(handle.into()),
                        cluster_suggestion: None,
                        task_summary: None,
                        forked_from_agent_id: None,
                        forked_at_seq: None,
                        acp_agent_id: None,
                    },
                    None,
                )
                .expect("upsert peer");
        }

        std::fs::write(wt_a.join("shared.txt"), "line1\nfrom-peer-a\nline3\n").expect("wt_a edit");
        std::fs::write(wt_a.join("a.txt"), "from-a\n").expect("wt_a a.txt");
        std::fs::write(wt_b.join("shared.txt"), "line1\nfrom-peer-b\nline3\n").expect("wt_b edit");
        std::fs::write(wt_b.join("b.txt"), "from-b\n").expect("wt_b b.txt");

        let preview_a = orch
            .preview_worktree_handoff_with_agent(
                &ws.workspace_id,
                Some("ag_wt_a"),
                WorktreeHandoffDirection::WorktreeToLocal,
                None,
                true,
            )
            .await
            .expect("preview peer a");
        assert!(preview_a.can_apply_cleanly);
        assert_eq!(preview_a.files.len(), 2);

        let (res_a, res_b) = tokio::join!(
            orch.execute_worktree_handoff(ExecuteWorktreeHandoffSpec {
                workspace_id: ws.workspace_id.clone(),
                source_agent_id: Some("ag_wt_a".into()),
                direction: WorktreeHandoffDirection::WorktreeToLocal,
                mode: WorktreeHandoffMode::Apply,
                target_workspace_id: None,
                include_committed: true,
                branch: None,
                allow_three_way_conflicts: true,
            }),
            orch.execute_worktree_handoff(ExecuteWorktreeHandoffSpec {
                workspace_id: ws.workspace_id.clone(),
                source_agent_id: Some("ag_wt_b".into()),
                direction: WorktreeHandoffDirection::WorktreeToLocal,
                mode: WorktreeHandoffMode::Apply,
                target_workspace_id: None,
                include_committed: true,
                branch: None,
                allow_three_way_conflicts: true,
            }),
        );
        let out_a = res_a.expect("peer a handoff succeeds");
        let out_b = res_b.expect("peer b handoff succeeds");

        assert!(repo_dir.join("a.txt").exists(), "a.txt must land in local");
        assert!(repo_dir.join("b.txt").exists(), "b.txt must land in local");
        assert!(
            out_a.had_conflicts || out_b.had_conflicts,
            "second overlapping handoff must report had_conflicts=true"
        );
        let shared = std::fs::read_to_string(repo_dir.join("shared.txt")).expect("read shared");
        assert!(
            shared.contains("<<<<<<<") && shared.contains(">>>>>>>"),
            "overlapping concurrent handoff must leave 3-way conflict markers: {shared}"
        );

        orch.shutdown_worktree_pool().await;
        let _ = std::fs::remove_dir_all(&home_dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn conflict_radar_warns_cross_worktree_and_never_wakes_idle_peers() {
        let (home_dir, store, orch) = test_orchestrator("cr_xwt").await;
        let sent: Arc<tokio::sync::Mutex<Vec<(String, String)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let sent_cb = Arc::clone(&sent);
        let dispatcher: crate::messaging::DispatchCallback = Arc::new(move |batch| {
            let sent_cb = Arc::clone(&sent_cb);
            Box::pin(async move {
                sent_cb
                    .lock()
                    .await
                    .push((batch.recipient_id, batch.wire_prompt));
                Ok(())
            })
        });
        let delivery = crate::messaging::AgentMessageDelivery::new(Duration::ZERO, dispatcher);
        orch.set_message_delivery(delivery).await;

        let dir_a = home_dir.join("wt_a");
        let dir_b = home_dir.join("wt_b");
        std::fs::create_dir_all(&dir_a).expect("mkdir a");
        std::fs::create_dir_all(&dir_b).expect("mkdir b");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_cr".into(),
                title: "Radar Test".into(),
                cwd: dir_a.to_string_lossy().to_string(),
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
                is_curated: true,
            })
            .expect("upsert ws");

        for (aid, handle, cwd, status) in [
            ("ag_cr_idle", "idle-dev", &dir_a, AgentStatus::Idle),
            (
                "ag_cr_run",
                "run_dev",
                &dir_b,
                AgentStatus::Running {
                    turn_id: "t1".into(),
                },
            ),
        ] {
            store
                .upsert_agent(
                    &AgentSummary {
                        agent_id: aid.into(),
                        workspace_id: "ws_cr".into(),
                        cwd: cwd.to_string_lossy().to_string(),
                        title: handle.into(),
                        provider: ProviderKind::ClaudeCode,
                        model: None,
                        effort: None,
                        fast_mode: false,
                        mode: SessionMode::FullAccess,
                        status,
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
                        handle: Some(handle.into()),
                        cluster_suggestion: None,
                        task_summary: None,
                        forked_from_agent_id: None,
                        forked_at_seq: None,
                        acp_agent_id: None,
                    },
                    None,
                )
                .expect("upsert agent");
        }

        let resolved_underscore = orch
            .resolve_peer_in_workspace("ws_cr", "idle_dev")
            .expect("resolve underscore alias")
            .expect("peer exists");
        assert_eq!(resolved_underscore.agent_id, "ag_cr_idle");

        orch.record_file_edit_and_check_conflict("ag_cr_idle", "src/api.rs")
            .await;
        orch.record_file_edit_and_check_conflict("ag_cr_run", "src/api.rs")
            .await;
        tokio::time::sleep(Duration::from_millis(80)).await;

        let events = store
            .list_workspace_events("ws_cr", 20)
            .expect("list events");
        assert!(
            events.iter().any(|ev| {
                ev.kind == orbit_protocol::WorkspaceEventKind::ConflictWarning
                    && ev.summary.contains("Cross-worktree conflict heads-up")
                    && ev.summary.contains("src/api.rs")
            }),
            "Cross-worktree conflict warning must be logged: {events:?}"
        );

        let delivered = sent.lock().await;
        assert!(
            !delivered.iter().any(|(id, _)| id == "ag_cr_idle"),
            "Idle peer must NEVER be woken by Conflict Radar"
        );
        assert!(
            delivered.iter().any(|(id, body)| {
                id == "ag_cr_run" && body.contains("Cross-worktree conflict heads-up")
            }),
            "Running peer should receive active Conflict Radar steer: {delivered:?}"
        );

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[tokio::test]
    async fn flush_queued_user_prompt_now_dispatches_single_interrupt_send_prompt_without_race() {
        let (home_dir, store, orch) = test_orchestrator("flush_q_now").await;
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir ws");
        let now = Utc::now();

        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_fq".into(),
                title: "Repo".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
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
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_fq".into(),
                    workspace_id: "ws_fq".into(),
                    cwd: ws_dir.to_string_lossy().to_string(),
                    title: "Flush Queue Agent".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::FullAccess,
                    status: AgentStatus::Running {
                        turn_id: "t_running".into(),
                    },
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
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        let (cmd_tx, mut cmd_rx) = mpsc::channel(8);
        let agent_gen = orch.bump_generation("ag_fq").await;
        let dummy_task = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_fq".into(),
            LiveAgentEntry {
                generation: agent_gen,
                cmd_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy_task.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        let saved = store
            .enqueue_agent_prompt("ag_fq", "q_now_1", "Run this now", "Run this now", &[])
            .expect("enqueue");
        orch.flush_queued_user_prompt_now("ag_fq", &saved.queue_id)
            .await
            .expect("flush queued prompt");

        // Must receive ONLY a single `SendPrompt { behavior: ActiveTurnBehavior::Interrupt }`
        // with NO preceding separate `DriverCommand::Interrupt`!
        let first_cmd = cmd_rx.try_recv().expect("driver command sent");
        match first_cmd {
            DriverCommand::SendPrompt {
                prompt, behavior, ..
            } => {
                assert_eq!(behavior, ActiveTurnBehavior::Interrupt);
                assert!(prompt.contains("Run this now"));
            }
            other => panic!("expected direct SendPrompt(Interrupt), got {other:?}"),
        }
        assert!(
            cmd_rx.try_recv().is_err(),
            "must not emit extra DriverCommand::Interrupt before SendPrompt"
        );

        // Preflight guard: if a `PrePromptSubmit` hook blocks `flush_queued_user_prompt_now`,
        // the queued prompt AND pending permissions MUST remain intact!
        #[cfg(unix)]
        {
            orch.hooks
                .upsert_hook(
                    None,
                    orbit_protocol::LifecycleHookRule {
                        id: "hk_block_flush".into(),
                        name: "Block flush".into(),
                        scope: orbit_protocol::HookScope::UserGlobal,
                        event: orbit_protocol::HookEventKind::PrePromptSubmit,
                        matcher: None,
                        action: orbit_protocol::HookAction::Command {
                            command: "/bin/sh".into(),
                            args: vec!["-c".into(), "echo policy-denied >&2; exit 2".into()],
                            env: std::collections::BTreeMap::new(),
                        },
                        timeout_ms: 5_000,
                        enabled: true,
                        trusted: true,
                        description: None,
                    },
                )
                .await
                .expect("upsert blocking hook");

            orch.pending_permissions.write().await.insert(
                ("ag_fq".to_string(), "perm_keep_1".to_string()),
                PendingPermissionSummary {
                    agent_id: "ag_fq".into(),
                    agent_title: "Flush Queue Agent".into(),
                    workspace_id: "ws_fq".into(),
                    request_id: "perm_keep_1".into(),
                    title: "Approve".into(),
                    detail: ToolCallDetail::Generic {
                        tool_name: "shell".into(),
                        arguments_json: "{}".into(),
                    },
                    allowed_decisions: vec!["allow".into(), "deny".into()],
                    guardian_assessment: None,
                    plan_approval: None,
                    human_only: false,
                    created_at: now,
                },
            );
            let blocked_item = store
                .enqueue_agent_prompt("ag_fq", "q_blocked_1", "Blocked text", "Blocked text", &[])
                .expect("enqueue blocked");
            let err = orch
                .flush_queued_user_prompt_now("ag_fq", &blocked_item.queue_id)
                .await
                .expect_err("blocking hook must reject flush");
            assert!(err.to_string().contains("blocked by hook"));
            assert!(
                store
                    .peek_queued_prompt("ag_fq", &blocked_item.queue_id)
                    .expect("peek")
                    .is_some(),
                "blocked hook must NOT consume the queued prompt"
            );
            assert!(
                orch.has_pending_permissions("ag_fq").await,
                "blocked hook must NOT cancel pending permissions"
            );
            assert!(
                cmd_rx.try_recv().is_err(),
                "blocked hook must NOT dispatch any driver command"
            );
        }

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[tokio::test]
    async fn plan_approval_approve_switches_mode_forwards_edits_and_request_changes_stays_in_plan_mode()
     {
        let home_dir =
            std::env::temp_dir().join(format!("orbit_plan_approval_test_{}", uuid::Uuid::new_v4()));
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir ws");
        let plan_file = ws_dir.join("implementation_plan.md");
        std::fs::write(&plan_file, "# Original Plan\n\n- Step 1\n").expect("write plan");

        let store = Arc::new(OrbitStore::open_in_memory().expect("open db"));
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_plan".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            crate::hooks::HooksEngine::new(home_dir.clone()),
        )
        .await;
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_plan".into(),
                title: "Plan Repo".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
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
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_plan".into(),
                    workspace_id: "ws_plan".into(),
                    cwd: ws_dir.to_string_lossy().to_string(),
                    title: "Plan Agent".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: Some("claude-sonnet-4-5".into()),
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Plan,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
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
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let agent_gen = orch.bump_generation("ag_plan").await;
        let dummy_task = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_plan".into(),
            LiveAgentEntry {
                generation: agent_gen,
                cmd_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy_task.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        // 1. Ingest ExitPlanMode gate backed by plan_file_path (enriches markdown from disk)
        orch.ingest_driver_event(
            "ag_plan",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "clp_plan_1".into(),
                title: "Review implementation plan".into(),
                detail: ToolCallDetail::Generic {
                    tool_name: "ExitPlanMode".into(),
                    arguments_json: serde_json::json!({
                        "planFilePath": plan_file.to_string_lossy().to_string()
                    })
                    .to_string(),
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;

        let pending = orch.list_pending_permissions(Some("ag_plan")).await;
        assert_eq!(pending.len(), 1);
        let plan_payload = pending[0]
            .plan_approval
            .as_ref()
            .expect("structured plan approval extracted");
        assert_eq!(plan_payload.plan_title.as_deref(), Some("Original Plan"));
        assert!(plan_payload.plan_markdown.contains("- Step 1"));

        // 2. RequestChanges keeps SessionMode::Plan and sends feedback to driver
        orch.resolve_plan_approval(
            "ag_plan",
            "clp_plan_1".into(),
            orbit_protocol::PlanApprovalAction::RequestChanges,
            None,
            Some("Add unit tests step".into()),
            None,
        )
        .await
        .expect("request changes");
        let ag_after_revise = store.get_agent("ag_plan").expect("get").expect("exists");
        assert_eq!(ag_after_revise.mode, SessionMode::Plan);
        match cmd_rx.try_recv().expect("driver cmd") {
            DriverCommand::ResolvePlanApproval {
                request_id,
                action,
                feedback,
                ..
            } => {
                assert_eq!(request_id, "clp_plan_1");
                assert_eq!(action, orbit_protocol::PlanApprovalAction::RequestChanges);
                assert_eq!(feedback.as_deref(), Some("Add unit tests step"));
            }
            other => panic!("unexpected cmd: {other:?}"),
        }

        // 3. Second ExitPlanMode gate -> ApproveAndBuild with edited Markdown switches to Default & updates plan file
        orch.ingest_driver_event(
            "ag_plan",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "clp_plan_2".into(),
                title: "Review implementation plan".into(),
                detail: ToolCallDetail::Generic {
                    tool_name: "ExitPlanMode".into(),
                    arguments_json: serde_json::json!({
                        "plan": "# Original Plan\n\n- Step 1",
                        "planFilePath": plan_file.to_string_lossy().to_string()
                    })
                    .to_string(),
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;

        let edited_md = "# Edited Plan\n\n- Step 1\n- Step 2 (tests)\n".to_string();
        orch.resolve_plan_approval(
            "ag_plan",
            "clp_plan_2".into(),
            orbit_protocol::PlanApprovalAction::ApproveAndBuild,
            Some(edited_md.clone()),
            None,
            None,
        )
        .await
        .expect("approve and build");

        let ag_after_approve = store.get_agent("ag_plan").expect("get").expect("exists");
        assert_eq!(ag_after_approve.mode, SessionMode::Default);
        assert_eq!(
            std::fs::read_to_string(&plan_file).expect("read edited plan"),
            edited_md
        );
        match cmd_rx.try_recv().expect("driver approve cmd") {
            DriverCommand::ResolvePlanApproval {
                request_id,
                action,
                edited_plan_markdown,
                target_mode,
                ..
            } => {
                assert_eq!(request_id, "clp_plan_2");
                assert_eq!(action, orbit_protocol::PlanApprovalAction::ApproveAndBuild);
                assert_eq!(edited_plan_markdown.as_deref(), Some(edited_md.as_str()));
                assert_eq!(target_mode, SessionMode::Default);
            }
            other => panic!("unexpected cmd: {other:?}"),
        }

        // 4. Stale/duplicate resolve_plan_approval MUST be rejected cleanly with zero double execution
        assert!(
            orch.resolve_plan_approval(
                "ag_plan",
                "clp_plan_2".into(),
                orbit_protocol::PlanApprovalAction::ApproveAndBuild,
                None,
                None,
                None,
            )
            .await
            .is_err(),
            "stale/duplicate plan approval must be rejected"
        );
        assert!(cmd_rx.try_recv().is_err());

        // 5. Ordinary non-plan permission MUST NOT be accepted by resolve_plan_approval and MUST stay intact
        orch.ingest_driver_event(
            "ag_plan",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "clp_shell_1".into(),
                title: "Run cargo test".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test".into(),
                    cwd: Some(ws_dir.to_string_lossy().to_string()),
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        assert!(
            orch.resolve_plan_approval(
                "ag_plan",
                "clp_shell_1".into(),
                orbit_protocol::PlanApprovalAction::ApproveAndBuild,
                None,
                None,
                None,
            )
            .await
            .is_err(),
            "ordinary non-plan permission must be rejected by resolve_plan_approval"
        );
        assert_eq!(
            orch.list_pending_permissions(Some("ag_plan")).await.len(),
            1,
            "failed resolve_plan_approval must leave ordinary permission intact"
        );
        orch.resolve_permission("ag_plan", "clp_shell_1".into(), "allow".into())
            .await
            .expect("resolve ordinary perm");
        let _ = cmd_rx.try_recv().expect("ordinary perm cmd");

        // 6. Plan approval in SessionMode::Auto must bypass Guardian auto-approve,
        //    reject agent-originated resolutions, and delegate human `resolve_permission("allow")`
        //    into ApproveAndBuild + mode switch cleanly.
        store
            .update_agent_mode("ag_plan", &SessionMode::Auto)
            .expect("switch to auto");
        orch.ingest_driver_event(
            "ag_plan",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "clp_plan_auto".into(),
                title: "Review plan".into(),
                detail: ToolCallDetail::Generic {
                    tool_name: "ExitPlanMode".into(),
                    arguments_json: serde_json::json!({ "plan": "# Auto Guard Plan\n- Step A" })
                        .to_string(),
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        assert!(
            cmd_rx.try_recv().is_err(),
            "Guardian must never auto-approve ExitPlanMode plan review gates"
        );
        let pending_auto = orch.list_pending_permissions(Some("ag_plan")).await;
        assert_eq!(pending_auto.len(), 1);
        assert_eq!(pending_auto[0].request_id, "clp_plan_auto");

        // Agent/Router resolution on human-only plan gate MUST be rejected without consuming gate
        assert!(
            orch.resolve_permission_by_agent("ag_plan", "clp_plan_auto".into(), "allow".into())
                .await
                .is_err(),
            "agent-originated resolution must be rejected for plan approval gates"
        );
        assert_eq!(
            orch.list_pending_permissions(Some("ag_plan")).await.len(),
            1,
            "rejected agent resolution must leave pending plan gate intact"
        );

        // Authenticated human generic resolve_permission("allow") delegates into ApproveAndBuild
        store
            .update_agent_mode("ag_plan", &SessionMode::Plan)
            .expect("switch back to plan");
        orch.resolve_permission("ag_plan", "clp_plan_auto".into(), "allow".into())
            .await
            .expect("human resolve_permission delegates cleanly");
        assert_eq!(
            store
                .get_agent("ag_plan")
                .expect("get")
                .expect("exists")
                .mode,
            SessionMode::Default
        );
        match cmd_rx.try_recv().expect("delegated plan approve cmd") {
            DriverCommand::ResolvePlanApproval {
                request_id,
                action,
                target_mode,
                ..
            } => {
                assert_eq!(request_id, "clp_plan_auto");
                assert_eq!(action, orbit_protocol::PlanApprovalAction::ApproveAndBuild);
                assert_eq!(target_mode, SessionMode::Default);
            }
            other => panic!(
                "expected ResolvePlanApproval from delegated resolve_permission, got {other:?}"
            ),
        }

        // 7. If driver channel is closed, resolve_plan_approval MUST fail without mutating mode or consuming gate
        store
            .update_agent_mode("ag_plan", &SessionMode::Plan)
            .expect("set plan mode");
        orch.ingest_driver_event(
            "ag_plan",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "clp_plan_dead".into(),
                title: "Review plan".into(),
                detail: ToolCallDetail::Generic {
                    tool_name: "ExitPlanMode".into(),
                    arguments_json: serde_json::json!({ "plan": "# Rollback Plan\n- Step 1" })
                        .to_string(),
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        drop(cmd_rx);
        assert!(
            orch.resolve_plan_approval(
                "ag_plan",
                "clp_plan_dead".into(),
                orbit_protocol::PlanApprovalAction::ApproveAndBuild,
                None,
                None,
                None,
            )
            .await
            .is_err(),
            "dead driver channel must fail cleanly"
        );
        assert_eq!(
            store
                .get_agent("ag_plan")
                .expect("get")
                .expect("exists")
                .mode,
            SessionMode::Plan,
            "mode must NOT mutate when driver delivery fails"
        );
        assert_eq!(
            orch.list_pending_permissions(Some("ag_plan")).await.len(),
            1,
            "pending plan gate must NOT be consumed when driver delivery fails"
        );

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn production_pre_tool_use_post_tool_use_and_notification_hooks_end_to_end() {
        let home_dir =
            std::env::temp_dir().join(format!("orbit-hooks-b303-test-{}", Uuid::new_v4().simple()));
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir ws");
        let post_log = home_dir.join("post_tool_use.jsonl");
        let notif_log = home_dir.join("notifications.jsonl");

        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let hooks = crate::hooks::HooksEngine::new(home_dir.clone());
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_hooks_b303".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            Arc::clone(&hooks),
        )
        .await;

        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_h1".into(),
                title: "Hooks Repo".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
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
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_h1".into(),
                    workspace_id: "ws_h1".into(),
                    cwd: ws_dir.to_string_lossy().to_string(),
                    title: "Hooks Worker".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Auto,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
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
                    handle: Some("worker".into()),
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let agent_gen = orch.bump_generation("ag_h1").await;
        let dummy_task = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_h1".into(),
            LiveAgentEntry {
                generation: agent_gen,
                cmd_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy_task.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        // 1. PreToolUse blocking hook denies a low-risk `cargo test` fail-closed even in Auto mode!
        hooks
            .upsert_hook(
                None,
                orbit_protocol::LifecycleHookRule {
                    id: "hk_pre_block".into(),
                    name: "Deny Shell Policy".into(),
                    scope: orbit_protocol::HookScope::UserGlobal,
                    event: orbit_protocol::HookEventKind::PreToolUse,
                    matcher: Some("Bash:cargo".into()),
                    action: orbit_protocol::HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            r#"printf '{"decision":"block","reason":"cargo blocked during freeze"}'"#
                                .into(),
                        ],
                        env: std::collections::BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert PreToolUse block hook");

        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "perm_blocked_by_hook".into(),
                title: "Run cargo test".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;

        match cmd_rx.try_recv().expect("PreToolUse block must send deny") {
            DriverCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                assert_eq!(request_id, "perm_blocked_by_hook");
                assert_eq!(decision, "deny");
            }
            other => panic!("expected ResolvePermission deny, got {other:?}"),
        }
        assert!(
            orch.list_pending_permissions(Some("ag_h1"))
                .await
                .is_empty(),
            "blocked tool must not remain pending"
        );
        let timeline = store
            .fetch_recent_timeline("ag_h1", 20)
            .expect("fetch timeline");
        assert!(
            timeline.iter().any(|ev| matches!(
                &ev.payload,
                TimelinePayload::Notice { message, .. }
                    if message.contains("Deny Shell Policy")
                        && message.contains("cargo blocked during freeze")
            )),
            "PreToolUse block reason must be surfaced in timeline"
        );

        // 2. CRITICAL SAFETY INVARIANT: PreToolUse returning `allow` MUST NEVER bypass
        //    `plan_approval.is_some()` AND MUST NEVER override an explicit user `Deny`!
        hooks
            .upsert_hook(
                None,
                orbit_protocol::LifecycleHookRule {
                    id: "hk_pre_allow".into(),
                    name: "Always Allow Hook".into(),
                    scope: orbit_protocol::HookScope::UserGlobal,
                    event: orbit_protocol::HookEventKind::PreToolUse,
                    matcher: Some("ExitPlanMode|file_edit".into()),
                    action: orbit_protocol::HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            r#"printf '{"decision":"allow","reason":"hook says allow"}'"#.into(),
                        ],
                        env: std::collections::BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert PreToolUse allow hook");

        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "perm_plan_gate".into(),
                title: "Review plan".into(),
                detail: ToolCallDetail::Generic {
                    tool_name: "ExitPlanMode".into(),
                    arguments_json: serde_json::json!({"plan": "# Plan\n- Step 1"}).to_string(),
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        assert!(
            cmd_rx.try_recv().is_err(),
            "PreToolUse allow MUST NEVER auto-approve a plan_approval gate"
        );
        let pending_plan = orch.list_pending_permissions(Some("ag_h1")).await;
        assert_eq!(pending_plan.len(), 1);
        assert_eq!(pending_plan[0].request_id, "perm_plan_gate");
        assert!(pending_plan[0].plan_approval.is_some());

        // In Default mode, PreToolUse allow still surfaces to the human and respects user Deny.
        store
            .update_agent_mode("ag_h1", &SessionMode::Default)
            .expect("set default mode");
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "perm_user_deny".into(),
                title: "Edit file".into(),
                detail: ToolCallDetail::FileEdit {
                    path: format!("{}/src/lib.rs", ws_dir.display()),
                    unified_diff: Some("+hi\n".into()),
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        assert!(
            cmd_rx.try_recv().is_err(),
            "PreToolUse allow must not auto-resolve in Default mode"
        );
        orch.resolve_permission("ag_h1", "perm_user_deny".into(), "deny".into())
            .await
            .expect("explicit user deny");
        match cmd_rx.try_recv().expect("user deny forwarded") {
            DriverCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                assert_eq!(request_id, "perm_user_deny");
                assert_eq!(decision, "deny");
            }
            other => panic!("expected explicit user deny, got {other:?}"),
        }

        // 3. PostToolUse fires asynchronously on terminal Completed/Failed ToolCall (deduped by call_id)
        hooks
            .upsert_hook(
                None,
                orbit_protocol::LifecycleHookRule {
                    id: "hk_post_tool".into(),
                    name: "Capture PostToolUse".into(),
                    scope: orbit_protocol::HookScope::UserGlobal,
                    event: orbit_protocol::HookEventKind::PostToolUse,
                    matcher: None,
                    action: orbit_protocol::HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            format!(
                                "cat >> '{}'; printf '\\n' >> '{}'",
                                post_log.display(),
                                post_log.display()
                            ),
                        ],
                        env: std::collections::BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert PostToolUse hook");

        // Running status must NOT trigger PostToolUse
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::ToolCall {
                call_id: "call_sh_1".into(),
                status: ToolCallStatus::Running,
                detail: ToolCallDetail::Shell {
                    command: "cargo test".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                background_task: None,
            },
        )
        .await;
        // Completed status fires PostToolUse with exit_code & output_preview
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::ToolCall {
                call_id: "call_sh_1".into(),
                status: ToolCallStatus::Completed,
                detail: ToolCallDetail::Shell {
                    command: "cargo test".into(),
                    cwd: None,
                    exit_code: Some(0),
                    output_preview: Some("12 tests passed".into()),
                },
                background_task: None,
            },
        )
        .await;
        // Duplicate terminal event for same call_id must NOT double-fire
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::ToolCall {
                call_id: "call_sh_1".into(),
                status: ToolCallStatus::Completed,
                detail: ToolCallDetail::Shell {
                    command: "cargo test".into(),
                    cwd: None,
                    exit_code: Some(0),
                    output_preview: Some("12 tests passed".into()),
                },
                background_task: None,
            },
        )
        .await;
        // Failed FileEdit fires PostToolUse with status=failed & modified_files
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::ToolCall {
                call_id: "call_edit_2".into(),
                status: ToolCallStatus::Failed,
                detail: ToolCallDetail::FileEdit {
                    path: "src/main.rs".into(),
                    unified_diff: Some("+broken\n".into()),
                },
                background_task: None,
            },
        )
        .await;

        hooks.wait_async_idle().await;
        let post_raw = std::fs::read_to_string(&post_log).expect("read post_tool_use log");
        let post_events: Vec<serde_json::Value> = post_raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("valid json line"))
            .collect();
        assert_eq!(
            post_events.len(),
            2,
            "expected 2 deduped PostToolUse payloads"
        );
        assert!(post_events.iter().any(|ev| {
            ev["event"] == "post_tool_use"
                && ev["tool_name"] == "shell"
                && ev["status"] == "completed"
                && ev["exit_code"] == 0
                && ev["output_preview"] == "12 tests passed"
        }));
        assert!(post_events.iter().any(|ev| {
            ev["event"] == "post_tool_use"
                && ev["tool_name"] == "file_edit"
                && ev["status"] == "failed"
                && ev["modified_files"] == serde_json::json!(["src/main.rs"])
        }));

        // 4. Notification hook fires on AwaitingPermission, Error, and foreground TurnCompleted!
        hooks
            .upsert_hook(
                None,
                orbit_protocol::LifecycleHookRule {
                    id: "hk_notif".into(),
                    name: "Capture Notifications".into(),
                    scope: orbit_protocol::HookScope::UserGlobal,
                    event: orbit_protocol::HookEventKind::Notification,
                    matcher: None,
                    action: orbit_protocol::HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            format!(
                                "cat >> '{}'; printf '\\n' >> '{}'",
                                notif_log.display(),
                                notif_log.display()
                            ),
                        ],
                        env: std::collections::BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert Notification hook");

        // (a) Entering AwaitingPermission
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::StatusChanged(AgentStatus::AwaitingPermission {
                turn_id: "t1".into(),
                request_id: "perm_plan_gate".into(),
            }),
        )
        .await;
        // (b) Entering Error
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::StatusChanged(AgentStatus::Error {
                message: "upstream rate limit exceeded".into(),
                recoverable: true,
            }),
        )
        .await;
        // (c) Running -> AssistantText -> Idle (foreground TurnCompleted with summary)
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::StatusChanged(AgentStatus::Running {
                turn_id: "t2".into(),
            }),
        )
        .await;
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::AssistantText {
                item_id: "msg_done".into(),
                text: "Refactored hooks and verified tests.".into(),
                streaming: false,
            },
        )
        .await;
        orch.ingest_driver_event(
            "ag_h1",
            agent_gen,
            DriverEvent::StatusChanged(AgentStatus::Idle),
        )
        .await;

        hooks.wait_async_idle().await;
        let notif_raw = std::fs::read_to_string(&notif_log).expect("read notifications log");
        let notif_events: Vec<serde_json::Value> = notif_raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("valid notif json"))
            .collect();
        assert!(
            notif_events.iter().any(|ev| {
                ev["event"] == "notification"
                    && ev["kind"] == "awaiting_permission"
                    && ev["agent_id"] == "ag_h1"
                    && ev["agent_title"] == "Hooks Worker"
            }),
            "missing awaiting_permission notification in {notif_events:?}"
        );
        assert!(
            notif_events.iter().any(|ev| {
                ev["event"] == "notification"
                    && ev["kind"] == "agent_error"
                    && ev["message"]
                        .as_str()
                        .is_some_and(|m| m.contains("upstream rate limit"))
            }),
            "missing agent_error notification in {notif_events:?}"
        );
        assert!(
            notif_events.iter().any(|ev| {
                ev["event"] == "notification"
                    && ev["kind"] == "turn_completed"
                    && ev["message"]
                        .as_str()
                        .is_some_and(|m| m.contains("Refactored hooks"))
            }),
            "missing turn_completed notification in {notif_events:?}"
        );

        // Verify HookExecutionRecord audit history contains PreToolUse, PostToolUse, and Notification.
        let snap = hooks.list_snapshot(None).await.expect("list hook snapshot");
        assert!(
            snap.recent_executions
                .iter()
                .any(|r| r.event == orbit_protocol::HookEventKind::PreToolUse
                    && r.decision == orbit_protocol::HookDecision::Block)
        );
        assert!(
            snap.recent_executions
                .iter()
                .any(|r| r.event == orbit_protocol::HookEventKind::PostToolUse)
        );
        assert!(
            snap.recent_executions
                .iter()
                .any(|r| r.event == orbit_protocol::HookEventKind::Notification)
        );

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn evaluating_permission_blocks_premature_resolve_and_provider_updated_input_fails_closed()
     {
        let home_dir =
            std::env::temp_dir().join(format!("orbit-eval-race-b303-{}", Uuid::new_v4().simple()));
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir ws");
        let ready_file = home_dir.join("hook_started.flag");

        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let hooks = crate::hooks::HooksEngine::new(home_dir.clone());
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_eval_race".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            Arc::clone(&hooks),
        )
        .await;

        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_race".into(),
                title: "Race WS".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
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
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_race".into(),
                    workspace_id: "ws_race".into(),
                    cwd: ws_dir.to_string_lossy().to_string(),
                    title: "Race Worker".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Auto,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
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
                    handle: Some("race".into()),
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let agent_gen = orch.bump_generation("ag_race").await;
        let dummy_task = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_race".into(),
            LiveAgentEntry {
                generation: agent_gen,
                cmd_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy_task.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        // 1. Slow blocking PreToolUse hook signals `ready_file`, sleeps 350ms, then blocks (exit 2).
        hooks
            .upsert_hook(
                None,
                orbit_protocol::LifecycleHookRule {
                    id: "hk_slow_block".into(),
                    name: "Slow Block".into(),
                    scope: orbit_protocol::HookScope::UserGlobal,
                    event: orbit_protocol::HookEventKind::PreToolUse,
                    matcher: Some("Bash:echo*".into()),
                    action: orbit_protocol::HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            format!(
                                "touch '{}'; sleep 0.35; echo 'blocked after delay' >&2; exit 2",
                                ready_file.display()
                            ),
                        ],
                        env: std::collections::BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert slow block hook");

        let orch_bg = Arc::clone(&orch);
        let ingest_task = tokio::spawn(async move {
            orch_bg
                .ingest_driver_event(
                    "ag_race",
                    agent_gen,
                    DriverEvent::PermissionRequest {
                        request_id: "perm_race_1".into(),
                        title: "Run echo".into(),
                        detail: ToolCallDetail::Shell {
                            command: "echo hi".into(),
                            cwd: None,
                            exit_code: None,
                            output_preview: None,
                        },
                        allowed_decisions: vec!["allow".into(), "deny".into()],
                        plan_approval: None,
                    },
                )
                .await;
        });

        // Wait until the PreToolUse hook is actively running in-flight.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        while !ready_file.exists() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        }
        assert!(ready_file.exists(), "PreToolUse hook should have started");

        // Premature external resolves (both human & agent, AND unknown request_id) MUST be rejected!
        let err_human = orch
            .resolve_permission("ag_race", "perm_race_1".into(), "allow".into())
            .await
            .expect_err("premature human resolve while evaluating must be rejected");
        assert!(err_human.to_string().contains("currently being evaluated"));

        let err_agent = orch
            .resolve_permission_by_agent("ag_race", "perm_race_1".into(), "allow".into())
            .await
            .expect_err("premature agent resolve while evaluating must be rejected");
        assert!(err_agent.to_string().contains("currently being evaluated"));

        let err_unknown = orch
            .resolve_permission("ag_race", "perm_unknown_99".into(), "allow".into())
            .await
            .expect_err("unknown request_id while evaluating must not forward to driver");
        assert!(err_unknown.to_string().contains("no pending permission"));

        // Confirm zero commands reached the driver prematurely.
        assert!(
            cmd_rx.try_recv().is_err(),
            "driver must NOT receive any premature allow command during PreToolUse evaluation"
        );

        ingest_task.await.expect("ingest task completes");
        match cmd_rx.try_recv() {
            Ok(DriverCommand::ResolvePermission {
                request_id,
                decision,
            }) => {
                assert_eq!(request_id, "perm_race_1");
                assert_eq!(decision, "deny");
            }
            other => panic!("expected PreToolUse deny after hook completed, got {other:?}"),
        }

        // 2. Provider PermissionRequest receiving `updated_input` from PreToolUse MUST fail closed!
        hooks
            .delete_hook(None, "hk_slow_block", orbit_protocol::HookScope::UserGlobal)
            .await
            .expect("delete slow block");
        hooks
            .upsert_hook(
                None,
                orbit_protocol::LifecycleHookRule {
                    id: "hk_provider_rewrite".into(),
                    name: "Provider Rewrite".into(),
                    scope: orbit_protocol::HookScope::UserGlobal,
                    event: orbit_protocol::HookEventKind::PreToolUse,
                    matcher: Some("Bash:ls*".into()),
                    action: orbit_protocol::HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            r#"printf '{"decision":"modify","updated_input":{"command":"ls -la"}}'"#
                                .into(),
                        ],
                        env: std::collections::BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert provider rewrite hook");

        orch.ingest_driver_event(
            "ag_race",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "perm_rewrite_1".into(),
                title: "Run ls".into(),
                detail: ToolCallDetail::Shell {
                    command: "ls".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;

        match cmd_rx.try_recv() {
            Ok(DriverCommand::ResolvePermission {
                request_id,
                decision,
            }) => {
                assert_eq!(request_id, "perm_rewrite_1");
                assert_eq!(
                    decision, "deny",
                    "unsupported provider updated_input must fail closed with deny"
                );
            }
            other => panic!("expected fail-closed deny for provider updated_input, got {other:?}"),
        }
        let timeline = store
            .fetch_recent_timeline("ag_race", 10)
            .expect("fetch timeline");
        assert!(
            timeline.iter().any(|ev| {
                matches!(
                    &ev.payload,
                    TimelinePayload::Notice { message, .. }
                        if message.contains("updated_input is not supported")
                )
            }),
            "expected explicit Notice about unsupported updated_input in timeline"
        );

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn multi_file_diff_triggers_pre_tool_block_and_post_tool_modified_files() {
        let home_dir =
            std::env::temp_dir().join(format!("orbit-multidiff-b303-{}", Uuid::new_v4().simple()));
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir ws");
        let post_log = home_dir.join("post_multidiff.jsonl");

        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let hooks = crate::hooks::HooksEngine::new(home_dir.clone());
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_multidiff".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            Arc::clone(&hooks),
        )
        .await;

        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_md".into(),
                title: "MultiDiff WS".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
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
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_md".into(),
                    workspace_id: "ws_md".into(),
                    cwd: ws_dir.to_string_lossy().to_string(),
                    title: "MultiDiff Worker".into(),
                    provider: ProviderKind::Codex,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Auto,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
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
                    handle: Some("md".into()),
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let agent_gen = orch.bump_generation("ag_md").await;
        let dummy_task = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_md".into(),
            LiveAgentEntry {
                generation: agent_gen,
                cmd_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy_task.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        hooks
            .upsert_hook(
                None,
                orbit_protocol::LifecycleHookRule {
                    id: "hk_block_env".into(),
                    name: "Block Env Diff".into(),
                    scope: orbit_protocol::HookScope::UserGlobal,
                    event: orbit_protocol::HookEventKind::PreToolUse,
                    matcher: Some("*.env".into()),
                    action: orbit_protocol::HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec!["-c".into(), "echo 'env file forbidden' >&2; exit 2".into()],
                        env: std::collections::BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert env block hook");
        hooks
            .upsert_hook(
                None,
                orbit_protocol::LifecycleHookRule {
                    id: "hk_post_md".into(),
                    name: "Capture MultiDiff Post".into(),
                    scope: orbit_protocol::HookScope::UserGlobal,
                    event: orbit_protocol::HookEventKind::PostToolUse,
                    matcher: Some("file_edit".into()),
                    action: orbit_protocol::HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            format!(
                                "cat >> '{}'; printf '\\n' >> '{}'",
                                post_log.display(),
                                post_log.display()
                            ),
                        ],
                        env: std::collections::BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert post md hook");

        let multi_diff = "\
diff --git a/src/lib.rs b/src/lib.rs\n\
--- a/src/lib.rs\n\
+++ b/src/lib.rs\n\
@@ -1 +1 @@\n\
+pub fn ok() {}\n\
diff --git a/config/prod.env b/config/prod.env\n\
--- a/config/prod.env\n\
+++ b/config/prod.env\n\
@@ -1 +1 @@\n\
+API_KEY=secret\n";

        // PreToolUse matcher `*.env` must match the SECOND file inside unified_diff and deny!
        orch.ingest_driver_event(
            "ag_md",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "perm_md_1".into(),
                title: "Apply patch".into(),
                detail: ToolCallDetail::FileEdit {
                    path: "src/lib.rs".into(),
                    unified_diff: Some(multi_diff.into()),
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        match cmd_rx.try_recv() {
            Ok(DriverCommand::ResolvePermission {
                request_id,
                decision,
            }) => {
                assert_eq!(request_id, "perm_md_1");
                assert_eq!(decision, "deny");
            }
            other => panic!("expected multi-file diff *.env block, got {other:?}"),
        }

        // PostToolUse must report BOTH modified files in `modified_files`.
        orch.ingest_driver_event(
            "ag_md",
            agent_gen,
            DriverEvent::ToolCall {
                call_id: "tc_md_1".into(),
                status: ToolCallStatus::Completed,
                detail: ToolCallDetail::FileEdit {
                    path: "src/lib.rs".into(),
                    unified_diff: Some(multi_diff.into()),
                },
                background_task: None,
            },
        )
        .await;
        hooks.wait_async_idle().await;
        let raw = std::fs::read_to_string(&post_log).expect("read post_multidiff");
        let parsed: serde_json::Value = serde_json::from_str(raw.trim()).expect("valid post json");
        let files: Vec<String> = parsed["modified_files"]
            .as_array()
            .expect("modified_files array")
            .iter()
            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
            .collect();
        assert_eq!(
            files,
            vec!["src/lib.rs".to_string(), "config/prod.env".to_string()]
        );

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[tokio::test]
    async fn fail_closed_guardian_verdict_marks_human_only_and_rejects_router_and_agent_allow() {
        let home_dir =
            std::env::temp_dir().join(format!("orbit-human-only-b303-{}", Uuid::new_v4().simple()));
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(ws_dir.join("src")).expect("mkdir src");
        std::fs::write(ws_dir.join("src/lib.rs"), "pub fn ok() {}\n").expect("write lib.rs");

        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let hooks = crate::hooks::HooksEngine::new(home_dir.clone());
        let listen_port = Arc::new(RwLock::new(0u16));
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_human_only".into(),
            "tok".into(),
            Arc::clone(&listen_port),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            Arc::clone(&hooks),
        )
        .await;

        // Configure Guardian with explicit unsupported LLM provider (`Antigravity`) so Tier-2
        // deterministically fails closed (`Medium + RequireHuman + heuristic_fallback + fail_closed`).
        orch.guardian
            .update_config(orbit_protocol::GuardianPolicyConfig {
                enabled: true,
                auto_approve_max_risk: orbit_protocol::GuardianRiskLevel::Medium,
                use_ai_classifier_for_medium: true,
                llm_tier2_enabled: true,
                llm_provider: Some(ProviderKind::Antigravity),
                ..orbit_protocol::GuardianPolicyConfig::default()
            })
            .await
            .expect("update guardian config");

        let host_info = orbit_protocol::HostInfo {
            host_id: "host_human_only".into(),
            hostname: "Local".into(),
            os: "macos".into(),
            arch: "aarch64".into(),
            public_key_b64: String::new(),
            protocol_version: orbit_protocol::PROTOCOL_VERSION,
            daemon_version: "0.10.3".into(),
            available_providers: Vec::new(),
            relay_url: None,
        };
        let connected = crate::connected_hosts::ConnectedHostsManager::new(
            host_info.host_id.clone(),
            Arc::clone(&store),
        );
        let router = crate::personal_router::PersonalRouterManager::new(
            home_dir.clone(),
            host_info,
            Arc::clone(&listen_port),
            "tok".into(),
            Arc::clone(&store),
            Arc::clone(&orch),
            connected,
        );
        orch.set_personal_router(Arc::clone(&router)).await;

        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_ho".into(),
                title: "HO WS".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
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
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_ho".into(),
                    workspace_id: "ws_ho".into(),
                    cwd: ws_dir.to_string_lossy().to_string(),
                    title: "HO Worker".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Auto,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
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
                    handle: Some("ho".into()),
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let agent_gen = orch.bump_generation("ag_ho").await;
        let dummy_task = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_ho".into(),
            LiveAgentEntry {
                generation: agent_gen,
                cmd_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy_task.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        orch.ingest_driver_event(
            "ag_ho",
            agent_gen,
            DriverEvent::PermissionRequest {
                request_id: "perm_ho_1".into(),
                title: "Edit src/lib.rs".into(),
                detail: ToolCallDetail::FileEdit {
                    path: ws_dir.join("src/lib.rs").to_string_lossy().to_string(),
                    unified_diff: Some("+pub fn added() {}\n".into()),
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;

        let pending = orch
            .get_pending_permission("ag_ho", "perm_ho_1")
            .await
            .expect("fail-closed LLM verdict must remain pending for human");
        assert!(
            pending.human_only,
            "fail-closed LLM fallback verdict MUST be marked human_only"
        );
        let assess = pending
            .guardian_assessment
            .as_ref()
            .expect("guardian_assessment attached");
        assert_eq!(assess.risk_level, orbit_protocol::GuardianRiskLevel::Medium);
        assert_eq!(
            assess.decision,
            orbit_protocol::GuardianDecision::RequireHuman
        );
        assert_eq!(assess.classifier_tier, "heuristic_fallback");
        assert!(assess.signals.iter().any(|s| s == "fail_closed"));

        // Personal Router policy check MUST reject `allow` even though risk_level is Medium!
        assert!(
            router
                .validate_router_permission_decision("allow", Some(assess))
                .await
                .is_err(),
            "Personal Router must reject fail-closed Medium assessment"
        );

        // Agent-originated resolution MUST reject `human_only` permissions!
        let err_agent = orch
            .resolve_permission_by_agent("ag_ho", "perm_ho_1".into(), "allow".into())
            .await
            .expect_err("agent resolution must be rejected for human_only permission");
        assert!(err_agent.to_string().contains("human-only"));

        // Explicit human resolution MUST still succeed cleanly!
        orch.resolve_permission("ag_ho", "perm_ho_1".into(), "allow".into())
            .await
            .expect("human resolve_permission must succeed");
        match cmd_rx.try_recv() {
            Ok(DriverCommand::ResolvePermission {
                request_id,
                decision,
            }) => {
                assert_eq!(request_id, "perm_ho_1");
                assert_eq!(decision, "allow");
            }
            other => panic!("expected human allow delivered to driver, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[tokio::test]
    async fn permission_rules_capture_allow_always_across_providers_and_block_compound_bypass() {
        let home_dir =
            std::env::temp_dir().join(format!("orbit-perm-rules-orch-{}", Uuid::new_v4().simple()));
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("create repo dir");

        let store = Arc::new(OrbitStore::open_in_memory().expect("open db"));
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_pr".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            crate::hooks::HooksEngine::new(home_dir.clone()),
        )
        .await;

        let now = Utc::now();
        let ws_cwd = ws_dir.to_string_lossy().to_string();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_pr".into(),
                title: "Rules Repo".into(),
                cwd: ws_cwd.clone(),
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
            .expect("upsert ws");

        for (agent_id, provider) in [
            ("ag_codex", ProviderKind::Codex),
            ("ag_claude", ProviderKind::ClaudeCode),
        ] {
            store
                .upsert_agent(
                    &AgentSummary {
                        agent_id: agent_id.into(),
                        workspace_id: "ws_pr".into(),
                        cwd: ws_cwd.clone(),
                        title: format!("Agent {agent_id}"),
                        provider,
                        model: None,
                        effort: None,
                        fast_mode: false,
                        mode: SessionMode::Default,
                        status: AgentStatus::Running {
                            turn_id: "t1".into(),
                        },
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
                        handle: None,
                        cluster_suggestion: None,
                        task_summary: None,
                        forked_from_agent_id: None,
                        forked_at_seq: None,
                        acp_agent_id: None,
                    },
                    None,
                )
                .expect("upsert agent");
        }

        let (codex_tx, mut codex_rx) = mpsc::channel(16);
        let codex_gen = orch.bump_generation("ag_codex").await;
        let t1 = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_codex".into(),
            LiveAgentEntry {
                generation: codex_gen,
                cmd_tx: codex_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: t1.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        let (claude_tx, mut claude_rx) = mpsc::channel(16);
        let claude_gen = orch.bump_generation("ag_claude").await;
        let t2 = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_claude".into(),
            LiveAgentEntry {
                generation: claude_gen,
                cmd_tx: claude_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: t2.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        // 1. Codex emits `cargo test -p orbit-store` permission prompt; human clicks `allow_always`.
        orch.ingest_driver_event(
            "ag_codex",
            codex_gen,
            DriverEvent::PermissionRequest {
                request_id: "cperm_1".into(),
                title: "Run command: cargo test -p orbit-store".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test -p orbit-store".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "allow_always".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        assert_eq!(
            orch.list_pending_permissions(Some("ag_codex")).await.len(),
            1
        );

        // Finding 13: invalid alias `"always"` MUST be rejected without consuming the pending prompt.
        assert!(
            orch.resolve_permission("ag_codex", "cperm_1".into(), "always".into())
                .await
                .is_err(),
            "invalid alias `always` must be rejected"
        );
        assert_eq!(
            orch.list_pending_permissions(Some("ag_codex")).await.len(),
            1
        );

        // Finding 11: peer/agent callers MUST NEVER mint persistent `allow_always` grants.
        assert!(
            orch.resolve_permission_by_agent("ag_codex", "cperm_1".into(), "allow_always".into())
                .await
                .is_err(),
            "agent-originated allow_always must be rejected"
        );
        assert!(
            orch.list_permission_rules(Some(&ws_cwd))
                .expect("list rules")
                .is_empty(),
            "rejected agent allow_always must not persist any rule"
        );

        orch.resolve_permission("ag_codex", "cperm_1".into(), "allow_always".into())
            .await
            .expect("resolve allow_always");
        // Finding 12: Orbit is the single persistence authority — provider MUST receive one-shot
        // `"allow"` (not `"allow_always"`) whenever `"allow"` is offered so rule revocation works!
        match codex_rx.try_recv().expect("codex resolved") {
            DriverCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                assert_eq!(request_id, "cperm_1");
                assert_eq!(
                    decision, "allow",
                    "provider must receive one-shot `allow` so Orbit SQLite remains authoritative"
                );
            }
            other => panic!("unexpected cmd: {other:?}"),
        }

        // Rule MUST be persisted as Project-scoped AlwaysAllowGrant across all providers (`provider: None`).
        let rules = orch
            .list_permission_rules(Some(&ws_cwd))
            .expect("list rules");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].pattern, "cargo test");
        assert_eq!(
            rules[0].pattern_kind,
            orbit_protocol::PermissionRulePatternKind::CommandPrefix
        );
        assert_eq!(
            rules[0].source,
            orbit_protocol::PermissionRuleSource::AlwaysAllowGrant
        );
        assert_eq!(rules[0].provider, None);

        // 2. Subsequent Codex AND Claude `cargo test` prompts MUST auto-approve without human prompt!
        orch.ingest_driver_event(
            "ag_codex",
            codex_gen,
            DriverEvent::PermissionRequest {
                request_id: "cperm_2".into(),
                title: "Run cargo test --lib".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test --lib".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "allow_always".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        orch.ingest_driver_event(
            "ag_claude",
            claude_gen,
            DriverEvent::PermissionRequest {
                request_id: "clperm_2".into(),
                title: "Run cargo test".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test --workspace".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "allow_always".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;

        assert!(orch.list_pending_permissions(None).await.is_empty());
        match codex_rx.try_recv().expect("codex auto-approved") {
            DriverCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                assert_eq!(request_id, "cperm_2");
                assert_eq!(decision, "allow");
            }
            other => panic!("unexpected cmd: {other:?}"),
        }
        match claude_rx.try_recv().expect("claude auto-approved") {
            DriverCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                assert_eq!(request_id, "clperm_2");
                assert_eq!(decision, "allow");
            }
            other => panic!("unexpected cmd: {other:?}"),
        }

        let updated_rules = orch
            .list_permission_rules(Some(&ws_cwd))
            .expect("list rules");
        assert_eq!(updated_rules[0].match_count, 2);
        assert!(updated_rules[0].last_matched_at.is_some());

        // 3. Compound injection `cargo test && rm -rf /` MUST NOT match the Allow rule!
        orch.ingest_driver_event(
            "ag_codex",
            codex_gen,
            DriverEvent::PermissionRequest {
                request_id: "cperm_evil".into(),
                title: "Run compound command".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test && rm -rf /".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "allow_always".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        assert_eq!(
            orch.list_pending_permissions(Some("ag_codex")).await.len(),
            1,
            "compound command MUST remain pending for human review"
        );
        assert!(codex_rx.try_recv().is_err());

        // Finding 12: Attempting `allow_always` on a non-derivable compound command MUST fail
        // explicitly and leave the pending prompt intact for one-time review.
        assert!(
            orch.resolve_permission("ag_codex", "cperm_evil".into(), "allow_always".into())
                .await
                .is_err(),
            "non-derivable compound command must reject allow_always"
        );
        assert_eq!(
            orch.list_pending_permissions(Some("ag_codex")).await.len(),
            1
        );
        assert!(codex_rx.try_recv().is_err());

        // Finding 12: Revoking the Orbit `cargo test` rule MUST take immediate effect (no stale
        // provider grant), causing the next `cargo test` prompt to require human review again!
        let allow_rule_id = updated_rules[0].id.clone();
        assert!(
            orch.delete_permission_rule(&allow_rule_id)
                .expect("revoke allow rule")
        );
        orch.ingest_driver_event(
            "ag_claude",
            claude_gen,
            DriverEvent::PermissionRequest {
                request_id: "clperm_after_revoke".into(),
                title: "Run cargo test after revoke".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test --lib".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "allow_always".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        assert_eq!(
            orch.list_pending_permissions(Some("ag_claude")).await.len(),
            1,
            "revoked rule must immediately stop auto-allowing"
        );
        assert!(claude_rx.try_recv().is_err());

        // 4. Adding a Host Deny rule for `rm -rf` MUST auto-deny even compound commands!
        orch.upsert_permission_rule(orbit_protocol::UpsertPermissionRuleInput {
            id: None,
            scope_kind: orbit_protocol::PermissionRuleScopeKind::Host,
            project_cwd: None,
            provider: None,
            tool_kind: orbit_protocol::PermissionRuleToolKind::Shell,
            pattern_kind: orbit_protocol::PermissionRulePatternKind::CommandPrefix,
            pattern: "rm -rf".into(),
            decision: orbit_protocol::PermissionRuleDecision::Deny,
            source: orbit_protocol::PermissionRuleSource::Manual,
            note: Some("Never allow rm -rf".into()),
        })
        .expect("upsert deny rule");

        orch.ingest_driver_event(
            "ag_claude",
            claude_gen,
            DriverEvent::PermissionRequest {
                request_id: "clperm_deny".into(),
                title: "Run compound rm".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test && rm -rf /tmp/foo".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        match claude_rx.try_recv().expect("claude auto-denied") {
            DriverCommand::ResolvePermission {
                request_id,
                decision,
            } => {
                assert_eq!(request_id, "clperm_deny");
                assert_eq!(decision, "deny");
            }
            other => panic!("unexpected cmd: {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[tokio::test]
    async fn resolve_permissions_batch_enforces_server_side_safety_invariants() {
        let home_dir =
            std::env::temp_dir().join(format!("orbit-perm-batch-orch-{}", Uuid::new_v4().simple()));
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("create repo dir");

        let store = Arc::new(OrbitStore::open_in_memory().expect("open db"));
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_batch".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            crate::hooks::HooksEngine::new(home_dir.clone()),
        )
        .await;

        let now = Utc::now();
        let ws_cwd = ws_dir.to_string_lossy().to_string();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_b".into(),
                title: "Batch Repo".into(),
                cwd: ws_cwd.clone(),
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
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_b".into(),
                    workspace_id: "ws_b".into(),
                    cwd: ws_cwd.clone(),
                    title: "Batch Agent".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
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
                    attention: AttentionState::Permission,
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
                },
                None,
            )
            .expect("upsert agent");

        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let agent_gen = orch.bump_generation("ag_b").await;
        let dummy = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_b".into(),
            LiveAgentEntry {
                generation: agent_gen,
                cmd_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        let insert_pending =
            |req_id: &str,
             detail: ToolCallDetail,
             allowed: Vec<String>,
             assessment: Option<orbit_protocol::GuardianAssessment>,
             plan: Option<orbit_protocol::PlanApprovalPayload>| {
                PendingPermissionSummary {
                    agent_id: "ag_b".into(),
                    agent_title: "Batch Agent".into(),
                    workspace_id: "ws_b".into(),
                    request_id: req_id.into(),
                    title: req_id.into(),
                    detail,
                    allowed_decisions: allowed,
                    guardian_assessment: assessment,
                    plan_approval: plan,
                    human_only: false,
                    created_at: now,
                }
            };

        let make_assessment =
            |risk: orbit_protocol::GuardianRiskLevel| orbit_protocol::GuardianAssessment {
                risk_level: risk,
                decision: orbit_protocol::GuardianDecision::RequireHuman,
                confidence: 0.95,
                classifier_tier: "deterministic".into(),
                reason: "test".into(),
                signals: vec![],
                latency_ms: 0,
            };

        {
            let mut guard = orch.pending_permissions.write().await;
            // 1. Safe Low-risk permission -> SHOULD RESOLVE
            guard.insert(
                ("ag_b".into(), "p_low".into()),
                insert_pending(
                    "p_low",
                    ToolCallDetail::Shell {
                        command: "cargo test".into(),
                        cwd: None,
                        exit_code: None,
                        output_preview: None,
                    },
                    vec!["allow".into(), "allow_always".into(), "deny".into()],
                    Some(make_assessment(orbit_protocol::GuardianRiskLevel::Low)),
                    None,
                ),
            );
            // 2. Safe Medium-risk permission -> SHOULD RESOLVE
            guard.insert(
                ("ag_b".into(), "p_med".into()),
                insert_pending(
                    "p_med",
                    ToolCallDetail::FileEdit {
                        path: "src/lib.rs".into(),
                        unified_diff: None,
                    },
                    vec!["allow".into(), "deny".into()],
                    Some(make_assessment(orbit_protocol::GuardianRiskLevel::Medium)),
                    None,
                ),
            );
            // 3. High-risk cached assessment -> MUST SKIP
            guard.insert(
                ("ag_b".into(), "p_high".into()),
                insert_pending(
                    "p_high",
                    ToolCallDetail::FileEdit {
                        path: "/etc/hosts".into(),
                        unified_diff: None,
                    },
                    vec!["allow".into(), "deny".into()],
                    Some(make_assessment(orbit_protocol::GuardianRiskLevel::High)),
                    None,
                ),
            );
            // 4. Unassessed destructive command (`rm -rf /`) -> on-demand Guardian classifies Critical -> MUST SKIP
            guard.insert(
                ("ag_b".into(), "p_unassessed_crit".into()),
                insert_pending(
                    "p_unassessed_crit",
                    ToolCallDetail::Shell {
                        command: "rm -rf /".into(),
                        cwd: None,
                        exit_code: None,
                        output_preview: None,
                    },
                    vec!["allow".into(), "deny".into()],
                    None,
                    None,
                ),
            );
            // 5. Human-only plan approval gate -> MUST SKIP
            guard.insert(
                ("ag_b".into(), "p_plan".into()),
                insert_pending(
                    "p_plan",
                    ToolCallDetail::Generic {
                        tool_name: "ExitPlanMode".into(),
                        arguments_json: "{}".into(),
                    },
                    vec!["allow".into(), "deny".into()],
                    Some(make_assessment(orbit_protocol::GuardianRiskLevel::Low)),
                    Some(orbit_protocol::PlanApprovalPayload {
                        plan_markdown: "# Plan".into(),
                        plan_title: Some("Plan".into()),
                        plan_file_path: None,
                        summary: None,
                        plan_file_backed: false,
                        allowed_prompts: vec![],
                        source: Some("exit_plan_mode".into()),
                    }),
                ),
            );
            // 6. Deny-only allowed_decisions -> MUST SKIP
            guard.insert(
                ("ag_b".into(), "p_deny_only".into()),
                insert_pending(
                    "p_deny_only",
                    ToolCallDetail::FileRead {
                        path: "README.md".into(),
                        start_line: None,
                        end_line: None,
                    },
                    vec!["deny".into()],
                    Some(make_assessment(orbit_protocol::GuardianRiskLevel::Low)),
                    None,
                ),
            );
        }

        // Batch with `allow_always` MUST be rejected!
        assert!(
            orch.resolve_permissions_batch(
                vec![("ag_b".into(), "p_low".into())],
                "allow_always".into(),
            )
            .await
            .is_err()
        );

        let batch = orch
            .resolve_permissions_batch(
                vec![
                    ("ag_b".into(), "p_low".into()),
                    ("ag_b".into(), "p_med".into()),
                    ("ag_b".into(), "p_high".into()),
                    ("ag_b".into(), "p_unassessed_crit".into()),
                    ("ag_b".into(), "p_plan".into()),
                    ("ag_b".into(), "p_deny_only".into()),
                ],
                "allow".into(),
            )
            .await
            .expect("batch resolve");

        assert_eq!(batch.resolved_count, 2);
        assert_eq!(batch.skipped_count, 4);

        let reason_for = |id: &str| {
            batch
                .skipped_reasons
                .iter()
                .find(|r| r.permission_request_id == id)
                .map(|r| r.reason.as_str())
        };
        assert_eq!(reason_for("p_high"), Some("high_or_critical_risk"));
        assert_eq!(
            reason_for("p_unassessed_crit"),
            Some("high_or_critical_risk")
        );
        assert_eq!(
            reason_for("p_plan"),
            Some("plan_approval_requires_individual_review")
        );
        assert_eq!(
            reason_for("p_deny_only"),
            Some("allow_decision_not_permitted")
        );

        // Verify exact 2 driver commands were dispatched (`p_low`, `p_med`)
        let mut resolved_ids = Vec::new();
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let DriverCommand::ResolvePermission {
                request_id,
                decision,
            } = cmd
            {
                assert_eq!(decision, "allow");
                resolved_ids.push(request_id);
            }
        }
        assert_eq!(resolved_ids, vec!["p_low", "p_med"]);

        // Remaining pending items MUST be exactly the 4 skipped items
        assert_eq!(orch.list_pending_permissions(Some("ag_b")).await.len(), 4);

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[tokio::test]
    async fn resolve_permissions_batch_audit_hardening_guardian_policy_atomic_claim_and_fail_closed()
     {
        let home_dir = std::env::temp_dir().join(format!(
            "orbit-perm-batch-audit-{}",
            Uuid::new_v4().simple()
        ));
        let ws_dir = home_dir.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("create repo dir");

        let store = Arc::new(OrbitStore::open_in_memory().expect("open db"));
        let orch = AgentOrchestrator::new(
            home_dir.clone(),
            "host_audit".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            crate::hooks::HooksEngine::new(home_dir.clone()),
        )
        .await;

        let now = Utc::now();
        let ws_cwd = ws_dir.to_string_lossy().to_string();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_a".into(),
                title: "Audit Repo".into(),
                cwd: ws_cwd.clone(),
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
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_a".into(),
                    workspace_id: "ws_a".into(),
                    cwd: ws_cwd.clone(),
                    title: "Audit Agent".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
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
                    attention: AttentionState::Permission,
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
                },
                None,
            )
            .expect("upsert agent");

        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
        let agent_gen = orch.bump_generation("ag_a").await;
        let dummy = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_a".into(),
            LiveAgentEntry {
                generation: agent_gen,
                cmd_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        // Live ghost session that is NOT in SQLite (simulates missing agent scope -> Finding 9)
        let (ghost_tx, mut ghost_rx) = mpsc::channel(16);
        let ghost_gen = orch.bump_generation("ag_ghost").await;
        let dummy_ghost = tokio::spawn(async {});
        orch.live_agents.write().await.insert(
            "ag_ghost".into(),
            LiveAgentEntry {
                generation: ghost_gen,
                cmd_tx: ghost_tx,
                done_notify: Arc::new(tokio::sync::Notify::new()),
                ingest_abort: dummy_ghost.abort_handle(),
                shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            },
        );

        let make_pending =
            |agent_id: &str,
             req_id: &str,
             cmd: &str,
             assessment: Option<orbit_protocol::GuardianAssessment>| {
                PendingPermissionSummary {
                    agent_id: agent_id.into(),
                    agent_title: agent_id.into(),
                    workspace_id: "ws_a".into(),
                    request_id: req_id.into(),
                    title: format!("Run {cmd}"),
                    detail: ToolCallDetail::Shell {
                        command: cmd.into(),
                        cwd: None,
                        exit_code: None,
                        output_preview: None,
                    },
                    allowed_decisions: vec!["allow".into(), "deny".into()],
                    guardian_assessment: assessment,
                    plan_approval: None,
                    human_only: false,
                    created_at: now,
                }
            };

        let low_assessment = orbit_protocol::GuardianAssessment {
            risk_level: orbit_protocol::GuardianRiskLevel::Low,
            decision: orbit_protocol::GuardianDecision::RequireHuman,
            confidence: 0.95,
            classifier_tier: "deterministic".into(),
            reason: "safe test".into(),
            signals: vec![],
            latency_ms: 0,
        };
        let disabled_medium_assessment = orbit_protocol::GuardianAssessment {
            risk_level: orbit_protocol::GuardianRiskLevel::Medium,
            decision: orbit_protocol::GuardianDecision::RequireHuman,
            confidence: 0.0,
            classifier_tier: "disabled".into(),
            reason: "Guardian disabled".into(),
            signals: vec!["guardian_disabled".into()],
            latency_ms: 0,
        };

        // 1. Finding 8: Disabled host Guardian MUST skip ALL allow items with `guardian_disabled`.
        let mut disabled_cfg = orch.guardian.get_config().await;
        disabled_cfg.enabled = false;
        orch.guardian
            .update_config(disabled_cfg)
            .await
            .expect("disable guardian");

        {
            let mut guard = orch.pending_permissions.write().await;
            guard.insert(
                ("ag_a".into(), "p_disabled_low".into()),
                make_pending(
                    "ag_a",
                    "p_disabled_low",
                    "cargo test",
                    Some(low_assessment.clone()),
                ),
            );
            guard.insert(
                ("ag_a".into(), "p_disabled_med".into()),
                make_pending(
                    "ag_a",
                    "p_disabled_med",
                    "echo hi",
                    Some(disabled_medium_assessment.clone()),
                ),
            );
        }

        let res_disabled = orch
            .resolve_permissions_batch(
                vec![
                    ("ag_a".into(), "p_disabled_low".into()),
                    ("ag_a".into(), "p_disabled_med".into()),
                ],
                "allow".into(),
            )
            .await
            .expect("batch when guardian disabled");
        assert_eq!(res_disabled.resolved_count, 0);
        assert_eq!(res_disabled.skipped_count, 2);
        assert!(
            res_disabled
                .skipped_reasons
                .iter()
                .all(|r| r.reason == "guardian_disabled")
        );
        assert!(cmd_rx.try_recv().is_err());

        // 2. Finding 8: Re-enable Guardian with host `blocked_command_patterns` -> MUST block
        //    `cargo test` AND reject a cached `guardian_disabled` Medium assessment!
        let mut policy_cfg = orch.guardian.get_config().await;
        policy_cfg.enabled = true;
        policy_cfg.blocked_command_patterns = vec!["cargo test".into()];
        orch.guardian
            .update_config(policy_cfg)
            .await
            .expect("update policy");

        {
            let mut guard = orch.pending_permissions.write().await;
            guard.insert(
                ("ag_a".into(), "p_uncached_blocked".into()),
                make_pending("ag_a", "p_uncached_blocked", "cargo test --lib", None),
            );
            guard.insert(
                ("ag_ghost".into(), "p_ghost".into()),
                make_pending("ag_ghost", "p_ghost", "ls", Some(low_assessment.clone())),
            );
        }

        let res_policy = orch
            .resolve_permissions_batch(
                vec![
                    ("ag_a".into(), "p_disabled_low".into()),
                    ("ag_a".into(), "p_disabled_med".into()),
                    ("ag_a".into(), "p_uncached_blocked".into()),
                    ("ag_ghost".into(), "p_ghost".into()),
                ],
                "allow".into(),
            )
            .await
            .expect("batch under host policy");
        assert_eq!(res_policy.resolved_count, 0);
        assert_eq!(res_policy.skipped_count, 4);
        let reason_for = |id: &str| {
            res_policy
                .skipped_reasons
                .iter()
                .find(|r| r.permission_request_id == id)
                .map(|r| r.reason.as_str())
        };
        assert_eq!(reason_for("p_disabled_low"), Some("high_or_critical_risk"));
        assert_eq!(reason_for("p_disabled_med"), Some("guardian_disabled"));
        assert_eq!(
            reason_for("p_uncached_blocked"),
            Some("high_or_critical_risk")
        );
        // Finding 9: missing agent scope fails closed even with cached Low assessment
        assert_eq!(reason_for("p_ghost"), Some("agent_scope_unavailable"));
        assert!(cmd_rx.try_recv().is_err());
        assert!(ghost_rx.try_recv().is_err());

        // 3. Finding 7: Concurrent batch calls on the SAME safe item MUST resolve at most ONCE!
        let mut clean_cfg = orch.guardian.get_config().await;
        clean_cfg.enabled = true;
        clean_cfg.blocked_command_patterns.clear();
        orch.guardian
            .update_config(clean_cfg)
            .await
            .expect("clean config");

        orch.pending_permissions.write().await.insert(
            ("ag_a".into(), "p_race".into()),
            make_pending("ag_a", "p_race", "ls -la", Some(low_assessment.clone())),
        );
        let (r1, r2) = tokio::join!(
            orch.resolve_permissions_batch(vec![("ag_a".into(), "p_race".into())], "allow".into(),),
            orch.resolve_permissions_batch(vec![("ag_a".into(), "p_race".into())], "allow".into(),),
        );
        let r1 = r1.expect("race 1");
        let r2 = r2.expect("race 2");
        assert_eq!(
            r1.resolved_count + r2.resolved_count,
            1,
            "concurrent batch callers must claim and resolve item at most once"
        );
        assert_eq!(r1.skipped_count + r2.skipped_count, 1);
        let mut dispatched = 0usize;
        while cmd_rx.try_recv().is_ok() {
            dispatched += 1;
        }
        assert_eq!(dispatched, 1, "driver must receive exactly one command");

        // 4. Finding 9 (ingress): Unknown agent scope on DriverEvent::PermissionRequest MUST NOT
        //    auto-allow even when Guardian auto_approve_max_risk == Low!
        let mut auto_cfg = orch.guardian.get_config().await;
        auto_cfg.enabled = true;
        auto_cfg.auto_approve_max_risk = orbit_protocol::GuardianRiskLevel::Low;
        orch.guardian
            .update_config(auto_cfg)
            .await
            .expect("auto config");

        orch.ingest_driver_event(
            "ag_ghost",
            ghost_gen,
            DriverEvent::PermissionRequest {
                request_id: "p_ghost_ingress".into(),
                title: "Read README".into(),
                detail: ToolCallDetail::FileRead {
                    path: "README.md".into(),
                    start_line: None,
                    end_line: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
            },
        )
        .await;
        assert!(
            ghost_rx.try_recv().is_err(),
            "missing agent scope on ingress must fail-closed and never auto-allow"
        );

        let _ = std::fs::remove_dir_all(&home_dir);
    }
}
