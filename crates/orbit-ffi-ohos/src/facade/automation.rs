use super::{
    OhosCoreFacade, block_on_core_rpc, parse_effort_opt, parse_mode_str, parse_provider_str,
};
use crate::dto::{OhosCreateSchedulePayload, OhosUpdateSchedulePayload};
use orbit_client_core::contract::{
    CancelWorkflowRunRequest, ControlGoalRunRequest, StopMonitorWatchRequest, UpdateGoalRunRequest,
};
use orbit_protocol::{ScheduleNewAgentConfig, ScheduleTarget};

impl OhosCoreFacade {
    // --- Automations: Schedules, Conditional Timers, Goals, Monitors & Workflows ---

    pub fn create_schedule_json(&self, req_json: &str) -> Result<(), String> {
        let payload: OhosCreateSchedulePayload = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid create schedule payload: {e}"))?;
        let target = match payload
            .existing_agent_id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
        {
            Some(agent_id) => ScheduleTarget::ExistingAgent { agent_id },
            None => ScheduleTarget::NewAgent(ScheduleNewAgentConfig {
                cwd: if payload.cwd.trim().is_empty() {
                    "~".to_string()
                } else {
                    payload.cwd
                },
                workspace_id: payload.workspace_id,
                provider: parse_provider_str(&payload.provider),
                acp_agent_id: payload.acp_agent_id,
                model: payload.model,
                effort: parse_effort_opt(payload.effort.as_deref()),
                fast_mode: payload.fast_mode,
                mode: parse_mode_str(&payload.mode),
                profile_id: None,
                worktree: payload.worktree,
                base_ref: payload.base_ref,
                archive_on_finish: payload.archive_on_finish,
                background: payload.background,
            }),
        };
        self.inner.create_schedule(
            &payload.host_id,
            payload.name,
            payload.cron_expr,
            payload.timezone,
            payload.prompt,
            target,
            payload.max_runs,
        );
        Ok(())
    }

    pub fn update_schedule_json(&self, req_json: &str) -> Result<(), String> {
        let payload: OhosUpdateSchedulePayload = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid update schedule payload: {e}"))?;
        self.inner.update_schedule(
            &payload.host_id,
            &payload.schedule_id,
            payload.name,
            payload.cron_expr,
            payload.timezone,
            payload.prompt,
            payload.max_runs,
            payload.archive_on_finish,
        );
        Ok(())
    }

    pub fn pause_schedule(&self, host_id: &str, schedule_id: &str) {
        self.inner.pause_schedule(host_id, schedule_id);
    }

    pub fn resume_schedule(&self, host_id: &str, schedule_id: &str) {
        self.inner.resume_schedule(host_id, schedule_id);
    }

    pub fn run_schedule_once(&self, host_id: &str, schedule_id: &str) {
        self.inner.run_schedule_once(host_id, schedule_id);
    }

    pub fn delete_schedule(&self, host_id: &str, schedule_id: &str) {
        self.inner.delete_schedule(host_id, schedule_id);
    }

    pub fn fetch_schedule_runs(&self, host_id: &str, schedule_id: &str, limit: Option<u32>) {
        self.inner.fetch_schedule_runs(host_id, schedule_id, limit);
    }

    pub fn schedule_runs_json(&self, host_id: &str, schedule_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.schedule_runs(host_id, schedule_id))
            .map_err(|e| e.to_string())
    }

    pub fn list_conditional_timers_json(
        &self,
        host_id: &str,
        agent_id: Option<String>,
        include_resolved: bool,
    ) -> Result<String, String> {
        let timers = block_on_core_rpc(self.inner.list_conditional_timers_on_host(
            host_id,
            agent_id,
            include_resolved,
        ))?;
        serde_json::to_string(&timers).map_err(|e| e.to_string())
    }

    pub fn cancel_conditional_timer_json(
        &self,
        host_id: &str,
        timer_id: &str,
    ) -> Result<String, String> {
        let record = block_on_core_rpc(
            self.inner
                .cancel_conditional_timer_on_host(host_id, timer_id),
        )?;
        serde_json::to_string(&record).map_err(|e| e.to_string())
    }

    pub fn fire_conditional_timer_now_json(
        &self,
        host_id: &str,
        timer_id: &str,
    ) -> Result<String, String> {
        let record = block_on_core_rpc(
            self.inner
                .fire_conditional_timer_now_on_host(host_id, timer_id),
        )?;
        serde_json::to_string(&record).map_err(|e| e.to_string())
    }

    pub fn list_goal_runs_json(
        &self,
        host_id: &str,
        agent_id: Option<String>,
    ) -> Result<String, String> {
        let goals = block_on_core_rpc(self.inner.list_goal_runs_on_host(host_id, agent_id))?;
        serde_json::to_string(&goals).map_err(|e| e.to_string())
    }

    pub fn control_goal_run_json(&self, req_json: &str) -> Result<String, String> {
        let req: ControlGoalRunRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid ControlGoalRunRequest: {e}"))?;
        let record = block_on_core_rpc(self.inner.control_goal_run_on_host(req))?;
        serde_json::to_string(&record).map_err(|e| e.to_string())
    }

    pub fn update_goal_run_json(&self, req_json: &str) -> Result<String, String> {
        let req: UpdateGoalRunRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid UpdateGoalRunRequest: {e}"))?;
        let record = block_on_core_rpc(self.inner.update_goal_run_on_host(req))?;
        serde_json::to_string(&record).map_err(|e| e.to_string())
    }

    pub fn list_monitor_watches_json(
        &self,
        host_id: &str,
        agent_id: Option<String>,
    ) -> Result<String, String> {
        let monitors =
            block_on_core_rpc(self.inner.list_monitor_watches_on_host(host_id, agent_id))?;
        serde_json::to_string(&monitors).map_err(|e| e.to_string())
    }

    pub fn stop_monitor_watch_json(&self, req_json: &str) -> Result<String, String> {
        let req: StopMonitorWatchRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid StopMonitorWatchRequest: {e}"))?;
        let record = block_on_core_rpc(self.inner.stop_monitor_watch_on_host(req))?;
        serde_json::to_string(&record).map_err(|e| e.to_string())
    }

    pub fn list_workflow_runs_json(
        &self,
        host_id: &str,
        include_completed: bool,
    ) -> Result<String, String> {
        let runs = block_on_core_rpc(
            self.inner
                .list_workflow_runs_on_host(host_id, include_completed),
        )?;
        serde_json::to_string(&runs).map_err(|e| e.to_string())
    }

    pub fn cancel_workflow_run_json(&self, req_json: &str) -> Result<String, String> {
        let req: CancelWorkflowRunRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid CancelWorkflowRunRequest: {e}"))?;
        let run = block_on_core_rpc(self.inner.cancel_workflow_run_on_host(req))?;
        serde_json::to_string(&run).map_err(|e| e.to_string())
    }

    pub fn start_goal_json(&self, req_json: String) -> Result<String, String> {
        serde_json::to_string(&self.inner.run_mobile_rpc(
            self.inner.start_goal_run_on_host(
                serde_json::from_str(&req_json).map_err(|e| e.to_string())?,
            ),
        )?)
        .map_err(|e| e.to_string())
    }

    pub fn create_monitor_json(&self, req_json: String) -> Result<String, String> {
        serde_json::to_string(&self.inner.run_mobile_rpc(
            self.inner.start_monitor_watch_on_host(
                serde_json::from_str(&req_json).map_err(|e| e.to_string())?,
            ),
        )?)
        .map_err(|e| e.to_string())
    }

    pub fn create_timer_json(
        &self,
        host_id: String,
        agent_id: String,
        duration_seconds: u64,
        prompt: String,
        condition_json: String,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.run_mobile_rpc(
            self.inner.create_conditional_timer_on_host(
                &host_id,
                &agent_id,
                duration_seconds,
                prompt,
                serde_json::from_str(&condition_json).map_err(|e| e.to_string())?,
            ),
        )?)
        .map_err(|e| e.to_string())
    }

    pub fn cancel_timer_json(&self, host_id: String, timer_id: String) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner
                    .cancel_conditional_timer_on_host(&host_id, &timer_id),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn start_workflow_json(&self, req_json: String) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.start_dag_workflow_on_host(
                    serde_json::from_str(&req_json).map_err(|e| e.to_string())?,
                ))?,
        )
        .map_err(|e| e.to_string())
    }
}
