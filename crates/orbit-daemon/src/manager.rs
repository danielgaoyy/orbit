//! Core Agent, Workspace/Worktree, Script, and Detached PTY lifecycle manager inside `orbit-daemon`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Utc;
use orbit_protocol::{
    ActiveTurnBehavior, AgentStatus, AgentSummary, AttentionState, HostProjectEntry, HostSyncState,
    OrchestrationConfig, PendingPermissionSummary, PromptBadge, ProviderKind,
    ProviderSubagentDescriptor, ProviderSubagentEvent, QuestionItem, ReasoningEffort, ScheduleSpec,
    SessionMode, TerminalSummary, TimelineEvent, TimelinePayload, ToolCallDetail, ToolCallStatus,
    WorkspaceBoardSnapshot, WorkspaceEventKind, WorkspaceEventRecord, WorkspaceIsolation,
    WorkspaceScriptInfo, WorkspaceScriptKind, WorkspaceScriptStatus, WorkspaceSetupProgress,
    WorkspaceSetupStatus, WorkspaceSummary, WorktreeCreateMode,
};
use orbit_providers::{
    DriverCommand, DriverEvent, OrbitMcpLaunchSpec, PersistedBackgroundTask, SessionLaunchConfig,
    find_provider_executable, spawn_provider_session,
};
use orbit_shim::{
    ShimClient, ShimFrame, ShimMode, ShimSpawnSpec, connect_existing_shim, kill_existing_shim,
    spawn_detached_shim,
};
use orbit_store::OrbitStore;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{info, warn};
use uuid::Uuid;

use crate::git::{
    capture_git_tree_snapshot, inspect_git_status, resolve_git_scope, restore_git_tree_snapshot,
    reverse_apply_file_edit_diffs,
};
use crate::messaging::{CoalescedDispatchBatch, sanitize_human_prompt_for_wire};
use crate::workspace_board::{
    board_file_path, build_workspace_roster_context, default_board_template,
    upsert_markdown_section, write_board_file,
};
use crate::workspace_cluster::{allocate_unique_handle, evaluate_auto_cluster, sanitize_handle};
use crate::workspace_runtime::{
    WorkspaceScriptRegistry, allocate_ephemeral_port, build_service_proxy_host,
    create_worktree_with_mode, load_workspace_manifest_with_fallback, run_worktree_setup_hooks,
    teardown_and_remove_worktree,
};

#[derive(Debug, Clone)]
pub struct RewindAgentOutcome {
    pub agent: AgentSummary,
    pub to_before_seq: u64,
    pub events: Vec<TimelineEvent>,
    pub restored_files: u32,
    pub restored_prompt: Option<String>,
    pub notice: Option<String>,
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
}

struct LiveAgentEntry {
    generation: u64,
    cmd_tx: mpsc::Sender<DriverCommand>,
    done_notify: Arc<tokio::sync::Notify>,
    ingest_abort: tokio::task::AbortHandle,
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
}

pub struct CreateAgentSpec {
    pub cwd: String,
    pub extra_dirs: Vec<String>,
    pub title: Option<String>,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
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
    live_agents: RwLock<HashMap<String, LiveAgentEntry>>,
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
    pub broadcast_tx: broadcast::Sender<DaemonBroadcast>,
}

impl AgentOrchestrator {
    pub async fn new(
        home_dir: PathBuf,
        host_id: String,
        mcp_token: String,
        listen_port: Arc<RwLock<u16>>,
        orch_config: Arc<RwLock<OrchestrationConfig>>,
        store: Arc<OrbitStore>,
        scripts: Arc<WorkspaceScriptRegistry>,
    ) -> Arc<Self> {
        let orbit_exe = resolve_orbit_exe();
        let (broadcast_tx, _) = broadcast::channel(2048);
        Arc::new(Self {
            home_dir,
            orbit_exe,
            host_id,
            mcp_token,
            listen_port,
            orch_config,
            store,
            scripts,
            live_agents: RwLock::new(HashMap::new()),
            agent_generations: RwLock::new(HashMap::new()),
            pending_questions: RwLock::new(HashMap::new()),
            live_terminals: RwLock::new(HashMap::new()),
            pending_permissions: RwLock::new(HashMap::new()),
            roster_fingerprints: RwLock::new(HashMap::new()),
            conflict_cooldowns: RwLock::new(HashMap::new()),
            suppress_auto_drain: RwLock::new(std::collections::HashSet::new()),
            broadcast_tx,
        })
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
    /// and wait (bounded) for the old driver ingestion loop to drain before mutating files or store.
    async fn stop_and_wait_driver(&self, agent_id: &str) -> u64 {
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
        self.clear_pending_requests(agent_id).await;
        next_gen
    }

    /// Activate live shim reattachment and startup agent recovery once the Daemon HTTP/WS listener
    /// is bound (must NOT be called from short-lived CLI read commands like `orbit pair`).
    pub async fn activate_runtime(self: &Arc<Self>) {
        self.reattach_surviving_terminals().await;
        self.reattach_or_recover_agents().await;
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
            let surviving_sock =
                if primary.exists() && connect_existing_shim(&primary).await.is_some() {
                    Some(primary)
                } else if legacy.exists() && connect_existing_shim(&legacy).await.is_some() {
                    Some(legacy)
                } else {
                    None
                };
            let was_running = ag.status.is_foreground_busy();

            if surviving_sock.is_some()
                && let Some(executable) = find_provider_executable(ag.provider)
            {
                let resume_ref = self.store.get_persistence_ref(&ag.agent_id).ok().flatten();
                let launch_gen = self.bump_generation(&ag.agent_id).await;
                if self
                    .spawn_and_wire_driver(&ag, executable, resume_ref, true, launch_gen)
                    .await
                    .is_ok()
                {
                    info!(agent_id = %ag.agent_id, "Reattached surviving agent shim across daemon restart");
                    continue;
                }
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
        })
        .await
    }

    pub async fn create_workspace_v2(
        &self,
        spec: CreateWorkspaceSpec,
    ) -> anyhow::Result<WorkspaceSummary> {
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
            let (wt_path, br) = create_worktree_with_mode(
                &self.home_dir,
                &src_path,
                spec.worktree_mode,
                spec.worktree_branch.as_deref(),
                spec.base_ref.as_deref(),
                spec.pr_number,
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
            return Ok(existing);
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
            setup_status: if has_setup {
                WorkspaceSetupStatus::Running
            } else {
                WorkspaceSetupStatus::Idle
            },
            mission_goal: spec.mission_goal,
            project_root,
            board_revision: 0,
            title_locked,
            auto_clustered: false,
        };
        self.store.upsert_workspace(&ws)?;
        let _ = self
            .broadcast_tx
            .send(DaemonBroadcast::WorkspaceUpdated(ws.clone()));

        // Execute `worktree.setup` asynchronously so workspace readiness publishes immediately
        if has_setup {
            let store_bg = Arc::clone(&self.store);
            let bcast_bg = self.broadcast_tx.clone();
            let ws_id = ws.workspace_id.clone();
            let src_bg = src_path;
            let wt_bg = final_path;
            let br_bg = branch_final;
            tokio::spawn(async move {
                let res = run_worktree_setup_hooks(
                    &ws_id,
                    &src_bg,
                    &wt_bg,
                    br_bg.as_deref(),
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
                let _ = store_bg.set_workspace_setup_status(&ws_id, final_status);
                if let Ok(Some(updated_ws)) = store_bg.get_workspace(&ws_id) {
                    let _ = bcast_bg.send(DaemonBroadcast::WorkspaceUpdated(updated_ws));
                }
            });
        }

        Ok(ws)
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

    pub async fn archive_workspace(self: &Arc<Self>, workspace_id: &str) -> anyhow::Result<()> {
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

        // 2. Soft-archive all active agents belonging to this workspace
        let agents = self
            .store
            .list_agents_filtered(Some(workspace_id), false, true, None)?;
        for ag in agents {
            let _ = self.archive_agent(&ag.agent_id).await;
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
        }

        Ok(())
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
        let model = spec
            .model
            .or_else(|| profile.as_ref().and_then(|p| p.model.clone()));
        let effort = spec
            .effort
            .or_else(|| profile.as_ref().and_then(|p| p.effort));
        let mode = if profile.is_some() && matches!(spec.mode, SessionMode::Default) {
            profile
                .as_ref()
                .map(|p| p.mode.clone())
                .unwrap_or(spec.mode)
        } else {
            spec.mode
        };

        let executable = find_provider_executable(provider).ok_or_else(|| {
            anyhow::anyhow!(
                "{} binary (`{}`) not found on host PATH",
                provider.display_name(),
                provider.default_binary()
            )
        })?;

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
                let (wt_path, created_br) = create_worktree_with_mode(
                    &self.home_dir,
                    &src_for_wt,
                    spec.worktree_mode,
                    Some(&auto_branch),
                    spec.base_ref.as_deref(),
                    spec.pr_number,
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
            existing_ws
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
                })
                .await?
            }
        };

        let now = Utc::now();
        let agent_id = format!("ag_{}", Uuid::new_v4().simple());
        let final_cwd = agent_cwd_override.unwrap_or_else(|| ws.cwd.clone());

        let existing_peers = self
            .store
            .list_agents_filtered(Some(&ws.workspace_id), false, false, None)
            .unwrap_or_default();
        let allocated_handle = allocate_unique_handle(
            spec.handle.as_deref(),
            provider,
            profile.as_ref().map(|p| p.name.as_str()),
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
        };
        self.store.upsert_agent(&summary, None)?;
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

        let initial_prompt = spec.initial_prompt;
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
                .spawn_and_wire_driver(&summary_bg, executable, None, false, launch_gen)
                .await
            {
                Ok(cmd_tx) => {
                    if mgr.current_generation(&ag_id_bg).await != launch_gen {
                        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
                        return;
                    }
                    if let Some((turn_id, _)) = initial_turn {
                        let sanitized = sanitize_human_prompt_for_wire(&initial_prompt, &turn_id);
                        let wire = mgr
                            .augment_prompt_with_workspace_context(&ag_id_bg, sanitized)
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
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
        behavior: ActiveTurnBehavior,
        message_id: Option<String>,
    ) -> anyhow::Result<()> {
        // Sending a human prompt acknowledges unread attention and unarchives if needed
        if let Ok(Some(ag)) = self.store.get_agent(agent_id)
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
            None => sanitized,
        };
        let wire_prompt = self
            .augment_prompt_with_workspace_context(agent_id, with_handoff)
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
        let (events, _) = self.store.fetch_timeline_since(agent_id, 0, 200).ok()?;
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

    pub async fn set_agent_config(
        &self,
        agent_id: &str,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
    ) -> anyhow::Result<()> {
        self.store
            .update_agent_config(agent_id, model.as_deref(), effort)?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx.send(DriverCommand::SetConfig { model, effort }).await;
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
        self.store.update_agent_model(agent_id, model.as_deref())?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx
                .send(DriverCommand::SetConfig {
                    model,
                    effort: None,
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
        mode: Option<SessionMode>,
        labels: Option<HashMap<String, String>>,
        handle: Option<String>,
    ) -> anyhow::Result<AgentSummary> {
        let clean_handle = handle.map(|h| sanitize_handle(&h));
        self.store.update_agent_fields(
            agent_id,
            title.as_deref(),
            model.as_deref().map(Some),
            effort.map(Some),
            mode.as_ref(),
            labels.as_ref(),
            clean_handle.as_deref(),
        )?;
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            if let Some(md) = mode {
                let _ = tx.send(DriverCommand::SetMode { mode: md }).await;
            }
            if model.is_some() || effort.is_some() {
                let _ = tx.send(DriverCommand::SetConfig { model, effort }).await;
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
    ) -> anyhow::Result<()> {
        let _ = find_provider_executable(provider).ok_or_else(|| {
            anyhow::anyhow!(
                "{} CLI (`{}`) is not installed on this host",
                provider.display_name(),
                provider.default_binary()
            )
        })?;

        let _ = self.stop_and_wait_driver(agent_id).await;
        kill_existing_shim(&self.shim_socket_path("a", agent_id)).await;
        kill_existing_shim(&self.legacy_shim_socket_path("agent", agent_id)).await;
        self.store
            .switch_agent_provider(agent_id, provider, model.as_deref(), effort)?;

        if let Ok(ev) = self.store.append_timeline_event(
            agent_id,
            1,
            None,
            TimelinePayload::Notice {
                level: "info".into(),
                message: format!("已切换至 {} 接力当前对话", provider.display_name()),
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
        kill_existing_shim(&self.shim_socket_path("a", agent_id)).await;
        kill_existing_shim(&self.legacy_shim_socket_path("agent", agent_id)).await;
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
        if let Some(updated) = self.store.mark_agent_read_seq(agent_id, seen_seq)? {
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
        let agent = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        let target_ws = match target_workspace_id {
            Some(ref ws_id) if !ws_id.trim().is_empty() => self
                .store
                .get_workspace(ws_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id}` not found"))?,
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
                })
                .await?
            }
        };

        // Ensure unique handle in the destination workspace
        let dest_peers = self
            .store
            .list_agents_filtered(Some(&target_ws.workspace_id), false, false, None)
            .unwrap_or_default();
        let next_handle =
            allocate_unique_handle(agent.handle.as_deref(), agent.provider, None, &dest_peers);
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
        if let Ok(ev) = self.store.append_workspace_event(
            &target_ws.workspace_id,
            Some(agent_id),
            Some(&next_handle),
            WorkspaceEventKind::PeerJoined,
            &format!("@{} joined workspace `{}`", next_handle, target_ws.title),
            None,
        ) {
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
            let unique_handle =
                allocate_unique_handle(ag.handle.as_deref(), ag.provider, None, &dst_peers);
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
            match guard.get(&key) {
                Some(prev) if now.duration_since(*prev) < Duration::from_secs(60) => false,
                _ => {
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

    async fn get_or_resume_driver(
        self: &Arc<Self>,
        agent_id: &str,
    ) -> anyhow::Result<mpsc::Sender<DriverCommand>> {
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            return Ok(tx);
        }

        let summary = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
        let resume_ref = self.store.get_persistence_ref(agent_id)?;
        let executable = find_provider_executable(summary.provider).ok_or_else(|| {
            anyhow::anyhow!("{} CLI not installed", summary.provider.display_name())
        })?;
        let launch_gen = self.bump_generation(agent_id).await;

        self.spawn_and_wire_driver(&summary, executable, resume_ref, false, launch_gen)
            .await
    }

    async fn spawn_and_wire_driver(
        self: &Arc<Self>,
        summary: &AgentSummary,
        executable: PathBuf,
        resume_session_id: Option<String>,
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
            mode: summary.mode.clone(),
            resume_session_id,
            mcp_spec,
            system_prompt_append: profile.and_then(|p| p.system_prompt_append),
            extra_env,
            reattach_existing_shim,
            persisted_running_bg_tasks,
            persisted_bg_wait,
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

        let ingest_task = tokio::spawn(async move {
            while let Some(evt) = evt_rx.recv().await {
                if mgr.current_generation(&ag_id).await != expected_gen {
                    continue;
                }
                mgr.ingest_driver_event(&ag_id, expected_gen, evt).await;
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
                payload,
            } => {
                if let Ok(saved) =
                    self.store
                        .append_provider_subagent_event(agent_id, &subagent_id, payload)
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
            _ => {}
        }

        let mut should_check_auto_archive = false;
        let mut should_drain_queued_prompt = false;
        let mut should_mark_finished = false;
        let payload = match evt {
            DriverEvent::StatusChanged(status) => {
                let prev = self.store.get_agent(agent_id).ok().flatten();
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
            } => TimelinePayload::AssistantMessage {
                item_id,
                text,
                streaming,
            },
            DriverEvent::ToolCall {
                call_id,
                status,
                detail,
                background_task,
            } => {
                if status == ToolCallStatus::Completed
                    && let ToolCallDetail::FileEdit { ref path, .. } = detail
                {
                    self.record_file_edit_and_check_conflict(agent_id, path)
                        .await;
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
                should_check_auto_archive = true;
                TimelinePayload::StatusChanged {
                    status: AgentStatus::Closed,
                }
            }
            DriverEvent::SessionInitialized { .. }
            | DriverEvent::ProviderSubagentUpsert(_)
            | DriverEvent::ProviderSubagentTimeline { .. }
            | DriverEvent::ReconcileBackgroundTasks(_) => unreachable!(),
        };

        let is_status_change = matches!(payload, TimelinePayload::StatusChanged { .. });
        if let Ok(saved_ev) = self.store.append_timeline_event(agent_id, 1, None, payload) {
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
            let _ = self.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));

            if finished_idle && !ag_bg {
                let peer_count = self
                    .store
                    .list_agents_filtered(Some(&ws_id), false, false, None)
                    .map(|p| p.len())
                    .unwrap_or(0);
                if peer_count >= 2 {
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

        let (program, args) = if let Some(cmd_line) = initial_command {
            ("/bin/sh".to_string(), vec!["-lc".to_string(), cmd_line])
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
