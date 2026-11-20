//! Core Agent & Detached PTY lifecycle manager inside `orbit-daemon`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Utc;
use orbit_protocol::{
    AgentStatus, AgentSummary, PromptBadge, ProviderKind, QuestionItem, ReasoningEffort,
    SessionMode, TerminalSummary, TimelineEvent, TimelinePayload, ToolCallDetail, ToolCallStatus,
    WorkspaceSummary,
};
use orbit_providers::{
    DriverCommand, DriverEvent, SessionLaunchConfig, find_provider_executable,
    spawn_provider_session,
};
use orbit_shim::{ShimClient, ShimFrame, ShimMode, ShimSpawnSpec, spawn_detached_shim};
use orbit_store::OrbitStore;
use tokio::sync::{RwLock, broadcast, mpsc};
use uuid::Uuid;

use crate::git::{
    capture_git_tree_snapshot, create_isolated_worktree, inspect_git_status,
    restore_git_tree_snapshot, reverse_apply_file_edit_diffs,
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
    TimelineLive(TimelineEvent),
    AgentRewound {
        origin_session_id: Uuid,
        outcome: RewindAgentOutcome,
    },
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
    last_snapshot: Arc<RwLock<Vec<u8>>>,
}

enum PtyAction {
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Kill,
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
}

pub struct AgentOrchestrator {
    home_dir: PathBuf,
    orbit_exe: PathBuf,
    store: Arc<OrbitStore>,
    live_agents: RwLock<HashMap<String, LiveAgentEntry>>,
    agent_generations: RwLock<HashMap<String, u64>>,
    agent_extra_dirs: RwLock<HashMap<String, Vec<PathBuf>>>,
    pending_permissions: RwLock<HashMap<String, Vec<String>>>,
    pending_questions: RwLock<HashMap<String, Vec<String>>>,
    live_terminals: RwLock<HashMap<String, Arc<LiveTerminal>>>,
    pub broadcast_tx: broadcast::Sender<DaemonBroadcast>,
}

impl AgentOrchestrator {
    pub async fn new(home_dir: PathBuf, store: Arc<OrbitStore>) -> Arc<Self> {
        let orbit_exe = resolve_orbit_exe();
        let (broadcast_tx, _) = broadcast::channel(1024);
        let this = Arc::new(Self {
            home_dir,
            orbit_exe,
            store,
            live_agents: RwLock::new(HashMap::new()),
            agent_generations: RwLock::new(HashMap::new()),
            agent_extra_dirs: RwLock::new(HashMap::new()),
            pending_permissions: RwLock::new(HashMap::new()),
            pending_questions: RwLock::new(HashMap::new()),
            live_terminals: RwLock::new(HashMap::new()),
            broadcast_tx,
        });
        this.reattach_surviving_terminals().await;
        this
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

    /// Re-attach to any `orbit shim` PTY sockets that survived a Daemon restart!
    async fn reattach_surviving_terminals(self: &Arc<Self>) {
        let Ok(saved) = self.store.list_terminals() else {
            return;
        };
        for term in saved {
            let sock = self
                .home_dir
                .join("shims")
                .join(format!("pty_{}.sock", term.terminal_id));
            if sock.exists()
                && let Ok(shim) = ShimClient::connect(&sock).await
            {
                self.bind_terminal_shim(term, shim).await;
            } else {
                let _ = self.store.delete_terminal(&term.terminal_id);
            }
        }
    }

    pub async fn ensure_workspace(
        &self,
        source_cwd: &str,
        title: Option<String>,
        worktree_branch: Option<String>,
        base_ref: Option<String>,
    ) -> anyhow::Result<WorkspaceSummary> {
        let src_path = crate::expand_host_path(source_cwd);
        let (final_path, is_worktree) = if let Some(ref branch) = worktree_branch {
            let wt =
                create_isolated_worktree(&self.home_dir, &src_path, branch, base_ref.as_deref())
                    .await?;
            (wt, true)
        } else {
            (src_path, false)
        };

        let cwd_str = final_path.to_string_lossy().to_string();
        if !is_worktree && let Ok(Some(existing)) = self.store.find_workspace_by_cwd(&cwd_str) {
            return Ok(existing);
        }

        let git_stat = inspect_git_status(&final_path).await;
        let default_title = final_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("workspace")
            .to_string();

        let ws = WorkspaceSummary {
            workspace_id: format!("ws_{}", Uuid::new_v4().simple()),
            title: title.unwrap_or(default_title),
            cwd: cwd_str,
            git_branch: git_stat.branch,
            is_worktree,
            updated_at: Utc::now(),
        };
        self.store.upsert_workspace(&ws)?;
        Ok(ws)
    }

    pub async fn create_and_start_agent(
        self: &Arc<Self>,
        spec: CreateAgentSpec,
    ) -> anyhow::Result<AgentSummary> {
        let CreateAgentSpec {
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
        } = spec;
        let executable = find_provider_executable(provider).ok_or_else(|| {
            anyhow::anyhow!(
                "{} binary (`{}`) not found on host PATH",
                provider.display_name(),
                provider.default_binary()
            )
        })?;

        let human_seed = derive_human_seed(
            initial_display_prompt.as_deref(),
            &initial_badges,
            &initial_prompt,
        );
        let ws = if worktree {
            let auto_branch = auto_worktree_branch(&human_seed);
            match self
                .ensure_workspace(&cwd, None, Some(auto_branch), None)
                .await
            {
                Ok(isolated) => isolated,
                Err(_) => self.ensure_workspace(&cwd, None, None, None).await?,
            }
        } else {
            self.ensure_workspace(&cwd, None, None, None).await?
        };
        let now = Utc::now();
        let agent_id = format!("ag_{}", Uuid::new_v4().simple());
        let computed_title = title.unwrap_or_else(|| summarize_prompt_title(&human_seed));

        let extra_paths: Vec<PathBuf> = extra_dirs
            .into_iter()
            .filter(|s| !s.trim().is_empty() && s.trim() != ws.cwd)
            .map(|s| crate::expand_host_path(&s))
            .collect();
        if !extra_paths.is_empty() {
            self.agent_extra_dirs
                .write()
                .await
                .insert(agent_id.clone(), extra_paths);
        }

        let summary = AgentSummary {
            agent_id: agent_id.clone(),
            workspace_id: ws.workspace_id,
            cwd: ws.cwd.clone(),
            title: computed_title,
            provider,
            model: model.clone(),
            effort,
            mode: mode.clone(),
            status: AgentStatus::Initializing,
            parent_agent_id: None,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
        };
        self.store.upsert_agent(&summary, None)?;
        let launch_gen = self.bump_generation(&agent_id).await;

        // Persist the initial user prompt IMMEDIATELY (`< 1ms`) so `AgentCreated` returns right away
        // and the user sees their clean message + badges at 0ms without blocking the socket!
        let initial_turn = if !initial_prompt.trim().is_empty() {
            let tid = format!("turn_{}", Uuid::new_v4().simple());
            let visible_text = initial_display_prompt.unwrap_or_else(|| initial_prompt.clone());
            let ev = self.store.append_timeline_event(
                &agent_id,
                1,
                Some(&tid),
                TimelinePayload::UserPrompt {
                    message_id: format!("u_{}", Uuid::new_v4().simple()),
                    text: visible_text,
                    badges: initial_badges,
                },
            )?;
            let prompt_seq = ev.seq;
            let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));
            Some((tid, prompt_seq))
        } else {
            None
        };

        let mgr = self.clone();
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
                .spawn_and_wire_driver(&summary_bg, executable, None, launch_gen)
                .await
            {
                Ok(cmd_tx) => {
                    if mgr.current_generation(&ag_id_bg).await != launch_gen {
                        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
                        return;
                    }
                    if let Some((turn_id, _)) = initial_turn {
                        let _ = cmd_tx
                            .send(DriverCommand::SendPrompt {
                                turn_id,
                                prompt: initial_prompt,
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
        self.send_prompt_with_meta(agent_id, prompt, None, Vec::new())
            .await
    }

    pub async fn send_prompt_with_meta(
        self: &Arc<Self>,
        agent_id: &str,
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
    ) -> anyhow::Result<()> {
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
        )
        .await
    }

    async fn dispatch_user_prompt_with_handoff(
        &self,
        agent_id: &str,
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
        handoff_context: Option<String>,
    ) -> anyhow::Result<()> {
        let cmd_tx = self
            .live_agent_tx(agent_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("agent session not active"))?;
        let turn_id = format!("turn_{}", Uuid::new_v4().simple());
        let visible_text = display_prompt.unwrap_or_else(|| prompt.clone());
        let ev = self.store.append_timeline_event(
            agent_id,
            1,
            Some(&turn_id),
            TimelinePayload::UserPrompt {
                message_id: format!("u_{}", Uuid::new_v4().simple()),
                text: visible_text,
                badges,
            },
        )?;
        let prompt_seq = ev.seq;
        let _ = self.broadcast_tx.send(DaemonBroadcast::TimelineLive(ev));

        // Capture a zero-side-effect pre-turn workspace tree snapshot before the agent mutates files.
        if let Ok(Some(ag)) = self.store.get_agent(agent_id)
            && let Some(tree_oid) = capture_git_tree_snapshot(Path::new(&ag.cwd)).await
        {
            let _ =
                self.store
                    .save_turn_checkpoint(agent_id, prompt_seq, Some(&turn_id), &tree_oid);
        }

        let wire_prompt = match handoff_context {
            Some(ctx) => format!("{ctx}\n\n[Latest User Request]\n{prompt}"),
            None => prompt,
        };

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id,
                prompt: wire_prompt,
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
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            let _ = tx.send(DriverCommand::Interrupt).await;
        }
        let cancelled_perms = self
            .pending_permissions
            .write()
            .await
            .remove(agent_id)
            .unwrap_or_default();
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
        self.pending_permissions.write().await.remove(agent_id);
        self.pending_questions.write().await.remove(agent_id);
    }

    pub async fn resolve_permission(
        &self,
        agent_id: &str,
        request_id: String,
        decision: String,
    ) -> anyhow::Result<()> {
        if let Some(tx) = self.live_agent_tx(agent_id).await {
            tx.send(DriverCommand::ResolvePermission {
                request_id: request_id.clone(),
                decision: decision.clone(),
            })
            .await
            .map_err(|_| anyhow::anyhow!("agent session not active"))?;

            Self::remove_pending_request(&self.pending_permissions, agent_id, &request_id).await;

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
        }
        Ok(())
    }

    pub async fn answer_question(
        &self,
        agent_id: &str,
        request_id: String,
        answers: Vec<Vec<String>>,
    ) -> anyhow::Result<()> {
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

    pub async fn archive_agent(&self, agent_id: &str) -> anyhow::Result<()> {
        let _ = self.stop_and_wait_driver(agent_id).await;
        self.store.delete_agent(agent_id)?;
        Ok(())
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

        self.spawn_and_wire_driver(&summary, executable, resume_ref, launch_gen)
            .await
    }

    async fn spawn_and_wire_driver(
        self: &Arc<Self>,
        summary: &AgentSummary,
        executable: PathBuf,
        resume_session_id: Option<String>,
        expected_gen: u64,
    ) -> anyhow::Result<mpsc::Sender<DriverCommand>> {
        let shim_socket = self
            .home_dir
            .join("shims")
            .join(format!("agent_{}.sock", summary.agent_id));

        let mut extra_dirs = self
            .agent_extra_dirs
            .read()
            .await
            .get(&summary.agent_id)
            .cloned()
            .unwrap_or_default();

        let attachments_dir = self.home_dir.join("attachments");
        let _ = std::fs::create_dir_all(&attachments_dir);
        if !extra_dirs.contains(&attachments_dir) {
            extra_dirs.push(attachments_dir.clone());
        }

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
        let mgr = self.clone();
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

    async fn ingest_driver_event(&self, agent_id: &str, expected_gen: u64, evt: DriverEvent) {
        if self.current_generation(agent_id).await != expected_gen {
            return;
        }
        let payload = match evt {
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
            DriverEvent::StatusChanged(status) => TimelinePayload::StatusChanged { status },
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
            } => TimelinePayload::ToolCall {
                call_id,
                status,
                detail,
            },
            DriverEvent::PermissionRequest {
                request_id,
                title,
                detail,
                allowed_decisions,
            } => {
                Self::record_pending_request(&self.pending_permissions, agent_id, &request_id)
                    .await;
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
            DriverEvent::Notice { level, message } => TimelinePayload::Notice { level, message },
            DriverEvent::Exited { .. } => {
                self.clear_pending_requests(agent_id).await;
                TimelinePayload::StatusChanged {
                    status: AgentStatus::Closed,
                }
            }
        };

        let is_status_change = matches!(payload, TimelinePayload::StatusChanged { .. });
        if let Ok(saved_ev) = self.store.append_timeline_event(agent_id, 1, None, payload) {
            let _ = self
                .broadcast_tx
                .send(DaemonBroadcast::TimelineLive(saved_ev));
        }
        if is_status_change && let Ok(Some(ag)) = self.store.get_agent(agent_id) {
            let _ = self.broadcast_tx.send(DaemonBroadcast::AgentUpdated(ag));
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
        let term_id = format!("term_{}", Uuid::new_v4().simple());
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let sock_path = self
            .home_dir
            .join("shims")
            .join(format!("pty_{term_id}.sock"));
        let candidate_cwd = crate::expand_host_path(&cwd);
        let resolved_cwd = if candidate_cwd.is_dir() {
            candidate_cwd
        } else {
            crate::expand_host_path("~")
        };
        let resolved_cwd_str = resolved_cwd.to_string_lossy().to_string();

        let spec = ShimSpawnSpec {
            shim_id: format!("pty_{term_id}"),
            mode: ShimMode::Pty { cols, rows },
            program: shell.clone(),
            args: vec![],
            cwd: resolved_cwd,
            env: HashMap::new(),
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
        let last_snapshot = Arc::new(RwLock::new(Vec::new()));

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

        let mgr = self.clone();
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
                                    *last_snapshot.write().await = bytes.clone();
                                    let _ = out_tx.send(PtyBroadcast::Snapshot(bytes));
                                }
                            }
                            ShimFrame::PtyOutput { data_b64 } => {
                                if let Ok(bytes) = B64.decode(data_b64) {
                                    {
                                        let mut sb = last_snapshot.write().await;
                                        sb.extend_from_slice(&bytes);
                                        if sb.len() > 64 * 1024 {
                                            let excess = sb.len() - 64 * 1024;
                                            sb.drain(..excess);
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
        let snap = live.last_snapshot.read().await.clone();
        Some((live.summary.clone(), snap, live.out_tx.subscribe()))
    }

    pub async fn write_terminal_input(&self, terminal_id: &str, data: Vec<u8>) {
        if let Some(live) = self.live_terminals.read().await.get(terminal_id).cloned() {
            let _ = live.shim_tx.send(PtyAction::Input(data)).await;
        }
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
