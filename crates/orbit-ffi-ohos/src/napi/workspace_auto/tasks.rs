use crate::OhosCoreFacade;
use napi_ohos::bindgen_prelude::*;

pub struct PreviewWorktreeHandoffTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for PreviewWorktreeHandoffTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .preview_worktree_handoff_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ExecuteWorktreeHandoffTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for ExecuteWorktreeHandoffTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .execute_worktree_handoff_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ListConditionalTimersTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) agent_id: Option<String>,
    pub(super) include_resolved: bool,
}

impl Task for ListConditionalTimersTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .list_conditional_timers_json(
                &self.host_id,
                self.agent_id.clone(),
                self.include_resolved,
            )
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CancelConditionalTimerTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) timer_id: String,
}

impl Task for CancelConditionalTimerTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cancel_conditional_timer_json(&self.host_id, &self.timer_id)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct FireConditionalTimerNowTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) timer_id: String,
}

impl Task for FireConditionalTimerNowTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .fire_conditional_timer_now_json(&self.host_id, &self.timer_id)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ListGoalRunsTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) agent_id: Option<String>,
}

impl Task for ListGoalRunsTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .list_goal_runs_json(&self.host_id, self.agent_id.clone())
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ControlGoalRunTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for ControlGoalRunTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .control_goal_run_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct UpdateGoalRunTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for UpdateGoalRunTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .update_goal_run_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ListMonitorWatchesTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) agent_id: Option<String>,
}

impl Task for ListMonitorWatchesTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .list_monitor_watches_json(&self.host_id, self.agent_id.clone())
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct StopMonitorWatchTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for StopMonitorWatchTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .stop_monitor_watch_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ListWorkflowRunsTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) include_completed: bool,
}

impl Task for ListWorkflowRunsTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .list_workflow_runs_json(&self.host_id, self.include_completed)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CancelWorkflowRunTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for CancelWorkflowRunTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cancel_workflow_run_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}
