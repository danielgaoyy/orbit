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

use crate::cron::{
    CronSchedule, ScheduleTimezone, normalize_cron_expr, parse_and_normalize_timezone,
};
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
        match self.recover_schedules(Utc::now()) {
            Ok(recovered) if recovered > 0 => {
                info!(recovered, "Recovered persisted schedules on daemon startup");
            }
            Ok(_) => {}
            Err(e) => {
                warn!(error = %e, "Failed to recover persisted schedules on daemon startup");
            }
        }
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            loop {
                ticker.tick().await;
                this.tick_due_schedules(Utc::now()).await;
            }
        })
    }

    /// Reconcile persisted active schedules on daemon startup:
    /// - retires expired or `max_runs`-exhausted schedules,
    /// - preserves valid due cursors (`next_run_at <= now` and strictly after `last_run_at`) so the
    ///   first ticker iteration dispatches them once instead of skipping to the next period,
    /// - repairs missing, clock-rolled-back, or timezone-mismatched cursors using `max(now, last_run_at)`,
    /// - pauses corrupted persisted schedules whose cron expression or timezone fails parsing.
    pub fn recover_schedules(&self, now: DateTime<Utc>) -> anyhow::Result<usize> {
        let list = self.store.list_schedules(false)?;
        let mut updated_count = 0usize;
        for mut sched in list {
            if sched.status != ScheduleStatus::Active {
                continue;
            }
            let expired = sched.expires_at.is_some_and(|exp| now >= exp);
            let exhausted = sched.max_runs.is_some_and(|max| sched.run_count >= max);
            if expired || exhausted {
                sched.status = ScheduleStatus::Completed;
                sched.next_run_at = None;
                sched.updated_at = now;
                if self.persist_and_broadcast(&sched) {
                    updated_count += 1;
                }
                continue;
            }

            let parsed_and_tz = CronSchedule::parse(&sched.cron_expr).and_then(|parsed| {
                ScheduleTimezone::parse(sched.timezone.as_deref()).map(|tz| (parsed, tz))
            });
            match parsed_and_tz {
                Ok((parsed, tz)) => {
                    let keep_valid_due = sched.next_run_at.is_some_and(|existing| {
                        existing <= now
                            && sched.last_run_at.is_none_or(|last| existing > last)
                            && parsed.matches_in_timezone(existing, tz)
                    });
                    if keep_valid_due {
                        continue;
                    }
                    let lower_bound = sched.last_run_at.map_or(now, |last| now.max(last));
                    let recomputed = parsed.next_after_in_timezone(lower_bound, tz);
                    if sched.next_run_at != recomputed {
                        sched.next_run_at = recomputed;
                        sched.updated_at = now;
                        if self.persist_and_broadcast(&sched) {
                            updated_count += 1;
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        schedule_id = %sched.schedule_id,
                        cron_expr = %sched.cron_expr,
                        timezone = ?sched.timezone,
                        error = %e,
                        "Pausing persisted schedule during startup recovery due to invalid cron or timezone"
                    );
                    sched.status = ScheduleStatus::Paused;
                    sched.next_run_at = None;
                    sched.updated_at = now;
                    if self.persist_and_broadcast(&sched) {
                        updated_count += 1;
                    }
                }
            }
        }
        Ok(updated_count)
    }

    fn persist_and_broadcast(&self, sched: &ScheduleSpec) -> bool {
        if let Err(e) = self.store.upsert_schedule(sched) {
            warn!(
                schedule_id = %sched.schedule_id,
                error = %e,
                "Failed to persist schedule state; skipping broadcast"
            );
            return false;
        }
        let _ = self
            .orchestrator
            .broadcast_tx
            .send(DaemonBroadcast::ScheduleUpdated(sched.clone()));
        true
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
        let (timezone, tz) = parse_and_normalize_timezone(timezone.as_deref())?;
        let parsed = CronSchedule::parse(&cron_expr)?;
        let now = Utc::now();
        let next_run_at = parsed.next_after_in_timezone(now, tz);
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
        timezone: Option<&str>,
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
        let mut parsed_tz = None;
        let mut timing_changed = false;
        if let Some(tz_raw) = timezone {
            let (norm_tz, tz) = parse_and_normalize_timezone(Some(tz_raw))?;
            sched.timezone = norm_tz;
            parsed_tz = Some(tz);
            timing_changed = true;
        }
        if let Some(c) = cron_raw {
            sched.cron_expr = normalize_cron_expr(c)?;
            timing_changed = true;
        }
        let now = Utc::now();
        if timing_changed {
            let tz = match parsed_tz {
                Some(tz) => tz,
                None => ScheduleTimezone::parse(sched.timezone.as_deref())?,
            };
            let parsed = CronSchedule::parse(&sched.cron_expr)?;
            let base = sched.last_run_at.map_or(now, |last| now.max(last));
            sched.next_run_at = parsed.next_after_in_timezone(base, tz);
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
        sched.updated_at = now;
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
        let now = Utc::now();
        sched.status = status;
        if status == ScheduleStatus::Active {
            let tz = ScheduleTimezone::parse(sched.timezone.as_deref())?;
            let parsed = CronSchedule::parse(&sched.cron_expr)?;
            let base = sched.last_run_at.map_or(now, |last| now.max(last));
            sched.next_run_at = parsed.next_after_in_timezone(base, tz);
        }
        sched.updated_at = now;
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
        let list = match self.store.list_schedules(false) {
            Ok(list) => list,
            Err(e) => {
                warn!(error = %e, "Failed to list active schedules for tick");
                return;
            }
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
                let _ = self.persist_and_broadcast(&sched);
                continue;
            }
            let Some(next) = sched.next_run_at else {
                continue;
            };
            if next > now {
                continue;
            }

            let parsed_and_tz = CronSchedule::parse(&sched.cron_expr).and_then(|parsed| {
                ScheduleTimezone::parse(sched.timezone.as_deref()).map(|tz| (parsed, tz))
            });
            let (parsed, tz) = match parsed_and_tz {
                Ok(pair) => pair,
                Err(e) => {
                    warn!(
                        schedule_id = %sched.schedule_id,
                        cron_expr = %sched.cron_expr,
                        timezone = ?sched.timezone,
                        error = %e,
                        "Pausing corrupted schedule without dispatching due to invalid cron or timezone"
                    );
                    sched.status = ScheduleStatus::Paused;
                    sched.next_run_at = None;
                    sched.updated_at = now;
                    let _ = self.persist_and_broadcast(&sched);
                    continue;
                }
            };

            // Advance schedule cursor BEFORE spawning so a slow run never double-triggers
            let base = sched.last_run_at.map_or(now, |last| now.max(last));
            sched.last_run_at = Some(base);
            sched.run_count = sched.run_count.saturating_add(1);
            let exhausted = sched.max_runs.is_some_and(|max| sched.run_count >= max);
            if exhausted {
                sched.status = ScheduleStatus::Completed;
                sched.next_run_at = None;
            } else {
                sched.next_run_at = parsed.next_after_in_timezone(base, tz);
            }
            sched.updated_at = now;
            if !self.persist_and_broadcast(&sched) {
                continue;
            }

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
        let _keep_awake_lease = self
            .orchestrator
            .keep_awake()
            .acquire_lease(format!("schedule:{run_id}"));
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
                let run_tz = ScheduleTimezone::parse(sched.timezone.as_deref())
                    .unwrap_or(ScheduleTimezone::Local);
                let spec = CreateAgentSpec {
                    cwd: cfg.cwd.clone(),
                    extra_dirs: Vec::new(),
                    title: Some(format!(
                        "{} · {}",
                        sched.name,
                        run_tz.format_utc(started_at, "%m-%d %H:%M")
                    )),
                    provider: cfg.provider,
                    model: cfg.model.clone(),
                    effort: cfg.effort,
                    fast_mode: if cfg.fast_mode { Some(true) } else { None },
                    mode: cfg.mode.clone(),
                    initial_prompt: sched.prompt.clone(),
                    initial_display_prompt: None,
                    initial_badges: Vec::new(),
                    worktree: cfg.worktree,
                    workspace_id: cfg.workspace_id.clone(),
                    parent_agent_id: None,
                    parent_host_id: None,
                    initial_prompt_source: None,
                    profile_id: cfg.profile_id.clone(),
                    background: cfg.background,
                    auto_archive: cfg.archive_on_finish,
                    base_ref: cfg.base_ref.clone(),
                    worktree_mode: WorktreeCreateMode::BranchOff,
                    branch: None,
                    pr_number: None,
                    handle: None,
                    auto_cluster: Some(false),
                    acp_agent_id: cfg.acp_agent_id.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};
    use chrono_tz::Tz;
    use orbit_protocol::{ProviderKind, ScheduleNewAgentConfig, SessionMode};

    fn test_target(cwd: &str) -> ScheduleTarget {
        ScheduleTarget::NewAgent(ScheduleNewAgentConfig {
            cwd: cwd.to_string(),
            workspace_id: None,
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            profile_id: None,
            worktree: false,
            base_ref: None,
            archive_on_finish: true,
            background: true,
        })
    }

    #[tokio::test]
    async fn schedule_engine_validates_wires_and_recovers_timezones() {
        let tmp_home =
            std::env::temp_dir().join(format!("orbit-scheduler-tz-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_home).expect("create temp home");
        let rt = crate::DaemonRuntime::bootstrap(&tmp_home)
            .await
            .expect("bootstrap runtime");
        let cwd = tmp_home.to_string_lossy().to_string();

        // 1. Invalid timezone is rejected on creation (never falls back to UTC).
        let bad = rt.scheduler.create_schedule(
            "Bad TZ".into(),
            "daily 09:00",
            Some("Mars/Olympus".into()),
            "noop".into(),
            test_target(&cwd),
            None,
            None,
            None,
        );
        assert!(bad.is_err(), "invalid timezone must fail creation");

        // 2. Valid IANA timezone (`Asia/Shanghai`) computes next_run_at in CST (09:00 CST == 01:00 UTC).
        let created = rt
            .scheduler
            .create_schedule(
                "Morning Triage".into(),
                "daily 09:00",
                Some("  Asia/Shanghai  ".into()),
                "noop".into(),
                test_target(&cwd),
                None,
                None,
                None,
            )
            .expect("create Asia/Shanghai schedule");
        assert_eq!(created.timezone.as_deref(), Some("Asia/Shanghai"));
        let next_sh = created.next_run_at.expect("next_run_at");
        let next_sh_local = next_sh.with_timezone(&Tz::Asia__Shanghai);
        assert_eq!((next_sh_local.hour(), next_sh_local.minute()), (9, 0));

        // 3. Invalid timezone is rejected on update without corrupting persisted schedule.
        let bad_update = rt.scheduler.update_schedule(
            &created.schedule_id,
            None,
            None,
            Some("Invalid/Zone"),
            None,
            None,
            None,
            None,
        );
        assert!(bad_update.is_err(), "invalid timezone must fail update");
        let unchanged = rt
            .store
            .get_schedule(&created.schedule_id)
            .expect("get schedule")
            .expect("schedule exists");
        assert_eq!(unchanged.timezone.as_deref(), Some("Asia/Shanghai"));

        // 4. Valid timezone update (`America/New_York`) recomputes next_run_at in NY wall-clock.
        let updated = rt
            .scheduler
            .update_schedule(
                &created.schedule_id,
                None,
                Some("30 1 * * *"),
                Some("America/New_York"),
                None,
                None,
                None,
                None,
            )
            .expect("update schedule to America/New_York");
        assert_eq!(updated.timezone.as_deref(), Some("America/New_York"));
        let next_ny = updated
            .next_run_at
            .expect("updated next_run_at")
            .with_timezone(&Tz::America__New_York);
        assert_eq!((next_ny.hour(), next_ny.minute()), (1, 30));

        // 5. Pause & Resume recomputes next_run_at in `America/New_York`.
        rt.scheduler
            .set_schedule_status(&created.schedule_id, ScheduleStatus::Paused)
            .expect("pause");
        let resumed = rt
            .scheduler
            .set_schedule_status(&created.schedule_id, ScheduleStatus::Active)
            .expect("resume");
        let resumed_ny = resumed
            .next_run_at
            .expect("resumed next_run_at")
            .with_timezone(&Tz::America__New_York);
        assert_eq!((resumed_ny.hour(), resumed_ny.minute()), (1, 30));

        // 6. Startup recovery repairs missing/stale next_run_at using schedule timezone.
        let mut stale = resumed.clone();
        stale.next_run_at = None;
        rt.store.upsert_schedule(&stale).expect("upsert stale");
        let recovery_now = Utc
            .with_ymd_and_hms(2026, 11, 1, 4, 30, 0)
            .single()
            .expect("recovery_now"); // 2026-11-01 00:30 EDT (before fall-back)
        let recovered_count = rt
            .scheduler
            .recover_schedules(recovery_now)
            .expect("recover missing");
        assert_eq!(recovered_count, 1);
        let recovered = rt
            .store
            .get_schedule(&created.schedule_id)
            .expect("get recovered")
            .expect("recovered exists");
        // First 01:30 on 2026-11-01 is 01:30 EDT == 05:30 UTC.
        let due_tick = Utc
            .with_ymd_and_hms(2026, 11, 1, 5, 30, 0)
            .single()
            .expect("05:30Z");
        assert_eq!(recovered.next_run_at, Some(due_tick));

        // 6b. Startup recovery 1 second AFTER a valid due tick preserves that due tick
        //     instead of discarding it and jumping to tomorrow.
        let after_due_1s = Utc
            .with_ymd_and_hms(2026, 11, 1, 5, 30, 1)
            .single()
            .expect("05:30:01Z");
        let preserved_updates = rt
            .scheduler
            .recover_schedules(after_due_1s)
            .expect("recover due tick");
        assert_eq!(preserved_updates, 0);
        let still_due = rt
            .store
            .get_schedule(&created.schedule_id)
            .expect("get still_due")
            .expect("still_due exists");
        assert_eq!(still_due.next_run_at, Some(due_tick));

        // 6c. Clock rollback (`now < last_run_at`) never moves cursor back to repeat a completed tick.
        let mut rolled_back = still_due.clone();
        rolled_back.last_run_at = Some(due_tick);
        rolled_back.next_run_at = None;
        rt.store
            .upsert_schedule(&rolled_back)
            .expect("upsert rolled_back");
        let rolled_back_now = Utc
            .with_ymd_and_hms(2026, 11, 1, 5, 20, 0)
            .single()
            .expect("05:20Z");
        let rollback_updates = rt
            .scheduler
            .recover_schedules(rolled_back_now)
            .expect("recover rollback");
        assert_eq!(rollback_updates, 1);
        let after_rollback = rt
            .store
            .get_schedule(&created.schedule_id)
            .expect("get after_rollback")
            .expect("after_rollback exists");
        assert_eq!(
            after_rollback.next_run_at,
            Some(
                Utc.with_ymd_and_hms(2026, 11, 2, 6, 30, 0)
                    .single()
                    .expect("next day 01:30 EST == 06:30Z")
            )
        );

        // 6d. Explicit empty timezone string in update_schedule resets to host local (`None`),
        //     while omitted `None` preserves existing timezone.
        let kept_tz = rt
            .scheduler
            .update_schedule(
                &created.schedule_id,
                Some("Renamed".into()),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .expect("update name only");
        assert_eq!(kept_tz.timezone.as_deref(), Some("America/New_York"));
        let reset_local = rt
            .scheduler
            .update_schedule(
                &created.schedule_id,
                None,
                None,
                Some(""),
                None,
                None,
                None,
                None,
            )
            .expect("reset timezone to local");
        assert_eq!(reset_local.timezone, None);

        // Restore America/New_York due tick for step 7 post-fire DST fall-back check.
        let mut restored_due = recovered;
        restored_due.last_run_at = None;
        restored_due.next_run_at = Some(due_tick);
        rt.store
            .upsert_schedule(&restored_due)
            .expect("restore due");

        // 7. Post-fire next_run_at calculation across DST fall-back advances to next day (no double-fire).
        // Insert a persisted busy agent so `execute_schedule_run` deterministically skips the
        // heartbeat without retiring the schedule or spawning an external CLI process.
        let test_ws = orbit_protocol::WorkspaceSummary {
            workspace_id: "ws_tz_test".into(),
            title: "TZ Test Workspace".into(),
            cwd: cwd.clone(),
            git_branch: None,
            is_worktree: false,
            updated_at: recovery_now,
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
        };
        rt.store
            .upsert_workspace(&test_ws)
            .expect("upsert test workspace");
        let busy_agent = orbit_protocol::AgentSummary {
            agent_id: "ag_busy_tz_test".into(),
            workspace_id: "ws_tz_test".into(),
            cwd: cwd.clone(),
            title: "Busy Test Agent".into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Running {
                turn_id: "turn_1".into(),
            },
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: recovery_now,
            updated_at: recovery_now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: std::collections::HashMap::new(),
            attention: orbit_protocol::AttentionState::default(),
            last_activity_at: None,
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
        rt.store
            .upsert_agent(&busy_agent, None)
            .expect("upsert busy agent");
        let mut hb_dst = restored_due;
        hb_dst.target = ScheduleTarget::ExistingAgent {
            agent_id: busy_agent.agent_id.clone(),
        };
        rt.store.upsert_schedule(&hb_dst).expect("upsert hb_dst");

        let fire_now = due_tick;
        rt.scheduler.tick_due_schedules(fire_now).await;
        // Let the spawned heartbeat runner finish its busy-skip path cleanly before teardown.
        tokio::task::yield_now().await;
        let post_fire = rt
            .store
            .get_schedule(&created.schedule_id)
            .expect("get post_fire")
            .expect("post_fire exists");
        assert_eq!(post_fire.status, ScheduleStatus::Active);
        assert_eq!(post_fire.run_count, 1);
        assert_eq!(
            post_fire.next_run_at,
            Some(
                Utc.with_ymd_and_hms(2026, 11, 2, 6, 30, 0)
                    .single()
                    .expect("next day 01:30 EST == 06:30Z")
            )
        );

        // 8. Corrupted persisted schedule (invalid timezone) is paused WITHOUT incrementing
        //    run_count or dispatching a run on tick_due_schedules.
        let mut corrupt = post_fire;
        corrupt.schedule_id = "sch_corrupt_tz".into();
        corrupt.timezone = Some("Invalid/Zone".into());
        corrupt.status = ScheduleStatus::Active;
        corrupt.run_count = 0;
        corrupt.last_run_at = None;
        corrupt.next_run_at = Some(due_tick);
        rt.store.upsert_schedule(&corrupt).expect("upsert corrupt");
        rt.scheduler.tick_due_schedules(fire_now).await;
        tokio::task::yield_now().await;
        let paused_corrupt = rt
            .store
            .get_schedule("sch_corrupt_tz")
            .expect("get corrupt")
            .expect("corrupt exists");
        assert_eq!(paused_corrupt.status, ScheduleStatus::Paused);
        assert_eq!(paused_corrupt.run_count, 0);
        assert_eq!(paused_corrupt.last_run_at, None);
        assert_eq!(paused_corrupt.next_run_at, None);
        let corrupt_runs = rt
            .store
            .list_schedule_runs("sch_corrupt_tz", 10)
            .expect("list corrupt runs");
        assert!(
            corrupt_runs.is_empty(),
            "corrupted schedule must not dispatch"
        );

        let _ = std::fs::remove_dir_all(&tmp_home);
    }
}
