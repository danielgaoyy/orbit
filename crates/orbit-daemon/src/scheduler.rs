//! Unified Cron Scheduler & Heartbeat Engine (`ScheduleEngine`).
//!
//! Runs a lightweight 1-second background loop driving:
//! - **Schedules**: creates a fresh workspace/worktree + agent on cron cadence, records run logs,
//!   and auto-archives run workspaces when `archive_on_finish` is enabled (default `true`).
//! - **Heartbeats**: wakes an existing long-lived agent periodically via `AgentMessageDelivery`,
//!   skips ticks cleanly if the agent is already busy, and auto-completes if the agent is archived.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageKind, AgentStatus, ScheduleKind, ScheduleRunRecord,
    ScheduleRunStatus, ScheduleSpec, ScheduleStatus, ScheduleTarget, WorktreeCreateMode,
};
use orbit_store::OrbitStore;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

use crate::cron::{CronSchedule, normalize_cron_expr};
use crate::manager::{AgentOrchestrator, CreateAgentSpec, DaemonBroadcast};
use crate::messaging::{AgentMessageDelivery, AgentPromptSource, QueuedAgentMessage};

pub struct ScheduleEngine {
    store: Arc<OrbitStore>,
    orchestrator: Arc<AgentOrchestrator>,
    delivery: Arc<AgentMessageDelivery>,
}

impl ScheduleEngine {
    pub fn new(
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        delivery: Arc<AgentMessageDelivery>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            orchestrator,
            delivery,
        })
    }

    pub fn spawn_background(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            loop {
                ticker.tick().await;
                this.tick_due_schedules(Utc::now()).await;
            }
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_schedule(
        &self,
        name: String,
        cron_raw: &str,
        timezone: Option<String>,
        prompt: String,
        target: ScheduleTarget,
        max_runs: Option<u32>,
        expires_at: Option<DateTime<Utc>>,
        created_by_agent: Option<String>,
    ) -> anyhow::Result<ScheduleSpec> {
        let cron_expr = normalize_cron_expr(cron_raw)?;
        let parsed = CronSchedule::parse(&cron_expr)?;
        let now = Utc::now();
        let next_run_at = parsed.next_after(now);
        let kind = match target {
            ScheduleTarget::NewAgent(_) => ScheduleKind::Schedule,
            ScheduleTarget::ExistingAgent { .. } => ScheduleKind::Heartbeat,
        };
        let sched = ScheduleSpec {
            schedule_id: format!(
                "{}_{}",
                if kind == ScheduleKind::Heartbeat {
                    "hb"
                } else {
                    "sch"
                },
                &Uuid::new_v4().simple().to_string()[..8]
            ),
            kind,
            name: if name.trim().is_empty() {
                if kind == ScheduleKind::Heartbeat {
                    "Heartbeat".to_string()
                } else {
                    "Scheduled Task".to_string()
                }
            } else {
                name.trim().to_string()
            },
            cron_expr,
            timezone,
            prompt,
            status: ScheduleStatus::Active,
            target,
            max_runs,
            run_count: 0,
            expires_at,
            last_run_at: None,
            next_run_at,
            created_by_agent,
            created_at: now,
            updated_at: now,
        };
        self.store.upsert_schedule(&sched)?;
        let _ = self
            .orchestrator
            .broadcast_tx
            .send(DaemonBroadcast::ScheduleUpdated(sched.clone()));
        Ok(sched)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_schedule(
        &self,
        schedule_id: &str,
        name: Option<String>,
        cron_raw: Option<&str>,
        prompt: Option<String>,
        max_runs: Option<u32>,
        expires_at: Option<DateTime<Utc>>,
        archive_on_finish: Option<bool>,
    ) -> anyhow::Result<ScheduleSpec> {
        let mut sched = self
            .store
            .get_schedule(schedule_id)?
            .ok_or_else(|| anyhow::anyhow!("schedule `{schedule_id}` not found"))?;
        if let Some(n) = name.filter(|s| !s.trim().is_empty()) {
            sched.name = n;
        }
        if let Some(c) = cron_raw {
            let norm = normalize_cron_expr(c)?;
            let parsed = CronSchedule::parse(&norm)?;
            sched.cron_expr = norm;
            sched.next_run_at = parsed.next_after(Utc::now());
        }
        if let Some(p) = prompt {
            sched.prompt = p;
        }
        if max_runs.is_some() {
            sched.max_runs = max_runs;
        }
        if expires_at.is_some() {
            sched.expires_at = expires_at;
        }
        if let Some(aof) = archive_on_finish
            && let ScheduleTarget::NewAgent(ref mut cfg) = sched.target
        {
            cfg.archive_on_finish = aof;
        }
        sched.updated_at = Utc::now();
        self.store.upsert_schedule(&sched)?;
        let _ = self
            .orchestrator
            .broadcast_tx
            .send(DaemonBroadcast::ScheduleUpdated(sched.clone()));
        Ok(sched)
    }

    pub fn set_schedule_status(
        &self,
        schedule_id: &str,
        status: ScheduleStatus,
    ) -> anyhow::Result<ScheduleSpec> {
        let mut sched = self
            .store
            .get_schedule(schedule_id)?
            .ok_or_else(|| anyhow::anyhow!("schedule `{schedule_id}` not found"))?;
        sched.status = status;
        if status == ScheduleStatus::Active
            && let Ok(parsed) = CronSchedule::parse(&sched.cron_expr)
        {
            sched.next_run_at = parsed.next_after(Utc::now());
        }
        sched.updated_at = Utc::now();
        self.store.upsert_schedule(&sched)?;
        let _ = self
            .orchestrator
            .broadcast_tx
            .send(DaemonBroadcast::ScheduleUpdated(sched.clone()));
        Ok(sched)
    }

    pub fn delete_schedule(&self, schedule_id: &str) -> anyhow::Result<()> {
        self.store.delete_schedule(schedule_id)?;
        let _ = self
            .orchestrator
            .broadcast_tx
            .send(DaemonBroadcast::ScheduleDeleted(schedule_id.to_string()));
        Ok(())
    }

    /// Trigger one immediate execution of a schedule without altering its cron cadence.
    pub async fn run_once(
        self: &Arc<Self>,
        schedule_id: &str,
    ) -> anyhow::Result<ScheduleRunRecord> {
        let sched = self
            .store
            .get_schedule(schedule_id)?
            .ok_or_else(|| anyhow::anyhow!("schedule `{schedule_id}` not found"))?;
        Ok(self.execute_schedule_run(sched, false).await)
    }

    /// Evaluate all active schedules and fire any whose `next_run_at <= now`.
    pub async fn tick_due_schedules(self: &Arc<Self>, now: DateTime<Utc>) {
        let Ok(list) = self.store.list_schedules(false) else {
            return;
        };
        for mut sched in list {
            if sched.status != ScheduleStatus::Active {
                continue;
            }
            if let Some(exp) = sched.expires_at
                && now >= exp
            {
                sched.status = ScheduleStatus::Completed;
                sched.next_run_at = None;
                sched.updated_at = now;
                let _ = self.store.upsert_schedule(&sched);
                let _ = self
                    .orchestrator
                    .broadcast_tx
                    .send(DaemonBroadcast::ScheduleUpdated(sched));
                continue;
            }
            let Some(next) = sched.next_run_at else {
                continue;
            };
            if next > now {
                continue;
            }

            // Advance schedule cursor BEFORE spawning so a slow run never double-triggers
            sched.last_run_at = Some(now);
            sched.run_count = sched.run_count.saturating_add(1);
            let exhausted = sched.max_runs.is_some_and(|max| sched.run_count >= max);
            if exhausted {
                sched.status = ScheduleStatus::Completed;
                sched.next_run_at = None;
            } else if let Ok(parsed) = CronSchedule::parse(&sched.cron_expr) {
                sched.next_run_at = parsed.next_after(now);
            }
            sched.updated_at = now;
            let _ = self.store.upsert_schedule(&sched);
            let _ = self
                .orchestrator
                .broadcast_tx
                .send(DaemonBroadcast::ScheduleUpdated(sched.clone()));

            let engine = Arc::clone(self);
            tokio::spawn(async move {
                let _ = engine.execute_schedule_run(sched, true).await;
            });
        }
    }

    async fn execute_schedule_run(
        self: &Arc<Self>,
        mut sched: ScheduleSpec,
        scheduled_tick: bool,
    ) -> ScheduleRunRecord {
        let run_id = format!("run_{}", &Uuid::new_v4().simple().to_string()[..10]);
        let started_at = Utc::now();
        let mut run = ScheduleRunRecord {
            run_id: run_id.clone(),
            schedule_id: sched.schedule_id.clone(),
            agent_id: None,
            workspace_id: None,
            status: ScheduleRunStatus::Running,
            summary: None,
            error: None,
            started_at,
            finished_at: None,
        };
        let _ = self.store.insert_schedule_run(&run);

        let target_clone = sched.target.clone();
        match &target_clone {
            ScheduleTarget::ExistingAgent { agent_id } => {
                // Heartbeat target
                match self.store.get_agent(agent_id) {
                    Ok(Some(ag)) if ag.archived_at.is_none() => {
                        if matches!(
                            ag.status,
                            AgentStatus::Running { .. }
                                | AgentStatus::Initializing
                                | AgentStatus::AwaitingPermission { .. }
                        ) {
                            run.status = ScheduleRunStatus::Skipped;
                            run.summary =
                                Some("Skipped heartbeat because agent is currently busy".into());
                            run.finished_at = Some(Utc::now());
                            let _ = self.store.finish_schedule_run(
                                &run_id,
                                ScheduleRunStatus::Skipped,
                                Some(agent_id),
                                Some(&ag.workspace_id),
                                run.summary.as_deref(),
                                None,
                            );
                            return run;
                        }
                        let body = format!(
                            "[Heartbeat {} · {}]\n\n{}",
                            sched.schedule_id,
                            started_at.to_rfc3339(),
                            sched.prompt
                        );
                        let res = self
                            .delivery
                            .enqueue(QueuedAgentMessage {
                                recipient_id: agent_id.clone(),
                                message_id: format!("hb_{run_id}"),
                                source: AgentPromptSource {
                                    kind: AgentMessageKind::Heartbeat,
                                    source_agent_id: None,
                                    source_host_id: None,
                                    source_title: Some(sched.name.clone()),
                                    event: Some("heartbeat".into()),
                                    permission_context: None,
                                },
                                body,
                                active_turn_behavior: ActiveTurnBehavior::Queue,
                                session_mode: None,
                                unarchive: false,
                            })
                            .await;
                        match res {
                            Ok((_, ack_rx)) => match ack_rx.await {
                                Ok(Ok(())) => {
                                    run.agent_id = Some(agent_id.clone());
                                    run.workspace_id = Some(ag.workspace_id.clone());
                                    run.status = ScheduleRunStatus::Succeeded;
                                    run.summary = Some("Heartbeat delivered".into());
                                    run.finished_at = Some(Utc::now());
                                    let _ = self.store.finish_schedule_run(
                                        &run_id,
                                        ScheduleRunStatus::Succeeded,
                                        Some(agent_id),
                                        Some(&ag.workspace_id),
                                        run.summary.as_deref(),
                                        None,
                                    );
                                }
                                Ok(Err(err)) => {
                                    run.status = ScheduleRunStatus::Failed;
                                    run.error = Some(err.clone());
                                    run.finished_at = Some(Utc::now());
                                    let _ = self.store.finish_schedule_run(
                                        &run_id,
                                        ScheduleRunStatus::Failed,
                                        Some(agent_id),
                                        Some(&ag.workspace_id),
                                        None,
                                        Some(&err),
                                    );
                                }
                                Err(_) => {
                                    run.status = ScheduleRunStatus::Failed;
                                    run.error = Some("heartbeat delivery canceled".into());
                                    run.finished_at = Some(Utc::now());
                                    let _ = self.store.finish_schedule_run(
                                        &run_id,
                                        ScheduleRunStatus::Failed,
                                        Some(agent_id),
                                        Some(&ag.workspace_id),
                                        None,
                                        run.error.as_deref(),
                                    );
                                }
                            },
                            Err(e) => {
                                let err = e.to_string();
                                run.status = ScheduleRunStatus::Failed;
                                run.error = Some(err.clone());
                                run.finished_at = Some(Utc::now());
                                let _ = self.store.finish_schedule_run(
                                    &run_id,
                                    ScheduleRunStatus::Failed,
                                    Some(agent_id),
                                    Some(&ag.workspace_id),
                                    None,
                                    Some(&err),
                                );
                            }
                        }
                    }
                    _ => {
                        // Target agent was archived or deleted -> retire heartbeat cleanly
                        if scheduled_tick {
                            sched.status = ScheduleStatus::Completed;
                            sched.next_run_at = None;
                            sched.updated_at = Utc::now();
                            let _ = self.store.upsert_schedule(&sched);
                            let _ = self
                                .orchestrator
                                .broadcast_tx
                                .send(DaemonBroadcast::ScheduleUpdated(sched));
                        }
                        run.status = ScheduleRunStatus::Skipped;
                        run.summary =
                            Some("Target agent archived or deleted; heartbeat retired".into());
                        run.finished_at = Some(Utc::now());
                        let _ = self.store.finish_schedule_run(
                            &run_id,
                            ScheduleRunStatus::Skipped,
                            Some(agent_id),
                            None,
                            run.summary.as_deref(),
                            None,
                        );
                    }
                }
            }
            ScheduleTarget::NewAgent(cfg) => {
                info!(schedule_id = %sched.schedule_id, name = %sched.name, "Launching scheduled agent run");
                let spec = CreateAgentSpec {
                    cwd: cfg.cwd.clone(),
                    extra_dirs: Vec::new(),
                    title: Some(format!(
                        "{} · {}",
                        sched.name,
                        started_at.format("%m-%d %H:%M")
                    )),
                    provider: cfg.provider,
                    model: cfg.model.clone(),
                    effort: cfg.effort,
                    mode: cfg.mode.clone(),
                    initial_prompt: sched.prompt.clone(),
                    initial_display_prompt: None,
                    initial_badges: Vec::new(),
                    worktree: cfg.worktree,
                    workspace_id: cfg.workspace_id.clone(),
                    parent_agent_id: None,
                    parent_host_id: None,
                    profile_id: cfg.profile_id.clone(),
                    background: cfg.background,
                    auto_archive: cfg.archive_on_finish,
                    base_ref: cfg.base_ref.clone(),
                    worktree_mode: WorktreeCreateMode::BranchOff,
                    branch: None,
                    pr_number: None,
                    handle: None,
                    auto_cluster: Some(false),
                };
                match self.orchestrator.create_and_start_agent(spec).await {
                    Ok(agent) => {
                        run.agent_id = Some(agent.agent_id.clone());
                        run.workspace_id = Some(agent.workspace_id.clone());
                        let _ = self.store.finish_schedule_run(
                            &run_id,
                            ScheduleRunStatus::Running,
                            Some(&agent.agent_id),
                            Some(&agent.workspace_id),
                            None,
                            None,
                        );
                        // Observe completion asynchronously to record final run status & summary
                        let store_bg = Arc::clone(&self.store);
                        let orch_bg = Arc::clone(&self.orchestrator);
                        let ag_id = agent.agent_id.clone();
                        let ws_id = agent.workspace_id.clone();
                        let rid = run_id.clone();
                        tokio::spawn(async move {
                            let mut rx = orch_bg.broadcast_tx.subscribe();
                            let deadline = tokio::time::Instant::now() + Duration::from_secs(3600);
                            loop {
                                let remaining =
                                    deadline.saturating_duration_since(tokio::time::Instant::now());
                                if remaining.is_zero() {
                                    let _ = store_bg.finish_schedule_run(
                                        &rid,
                                        ScheduleRunStatus::Failed,
                                        Some(&ag_id),
                                        Some(&ws_id),
                                        None,
                                        Some("scheduled run timed out"),
                                    );
                                    return;
                                }
                                match tokio::time::timeout(
                                    remaining.min(Duration::from_secs(15)),
                                    rx.recv(),
                                )
                                .await
                                {
                                    Ok(Ok(DaemonBroadcast::AgentUpdated(updated)))
                                        if updated.agent_id == ag_id =>
                                    {
                                        match updated.status {
                                            AgentStatus::Idle => {
                                                let summary = store_bg
                                                    .last_assistant_message_text(&ag_id)
                                                    .ok()
                                                    .flatten()
                                                    .map(|s| {
                                                        s.chars().take(400).collect::<String>()
                                                    });
                                                let _ = store_bg.finish_schedule_run(
                                                    &rid,
                                                    ScheduleRunStatus::Succeeded,
                                                    Some(&ag_id),
                                                    Some(&ws_id),
                                                    summary.as_deref(),
                                                    None,
                                                );
                                                return;
                                            }
                                            AgentStatus::Error { message, .. } => {
                                                let _ = store_bg.finish_schedule_run(
                                                    &rid,
                                                    ScheduleRunStatus::Failed,
                                                    Some(&ag_id),
                                                    Some(&ws_id),
                                                    None,
                                                    Some(&message),
                                                );
                                                return;
                                            }
                                            AgentStatus::Closed
                                                if updated.archived_at.is_some() =>
                                            {
                                                let summary = store_bg
                                                    .last_assistant_message_text(&ag_id)
                                                    .ok()
                                                    .flatten()
                                                    .map(|s| {
                                                        s.chars().take(400).collect::<String>()
                                                    });
                                                let _ = store_bg.finish_schedule_run(
                                                    &rid,
                                                    ScheduleRunStatus::Succeeded,
                                                    Some(&ag_id),
                                                    Some(&ws_id),
                                                    summary.as_deref(),
                                                    None,
                                                );
                                                return;
                                            }
                                            _ => {}
                                        }
                                    }
                                    Ok(Err(broadcast::error::RecvError::Closed)) => return,
                                    _ => {}
                                }
                            }
                        });
                    }
                    Err(e) => {
                        let err = e.to_string();
                        warn!(schedule_id = %sched.schedule_id, error = %err, "Scheduled agent launch failed");
                        run.status = ScheduleRunStatus::Failed;
                        run.error = Some(err.clone());
                        run.finished_at = Some(Utc::now());
                        let _ = self.store.finish_schedule_run(
                            &run_id,
                            ScheduleRunStatus::Failed,
                            None,
                            None,
                            None,
                            Some(&err),
                        );
                    }
                }
            }
        }

        run
    }
}
