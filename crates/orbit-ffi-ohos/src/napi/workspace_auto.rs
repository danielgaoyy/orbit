use super::core::OrbitHarmonyCore;
use super::{MobileRpcTask, run_mobile_job};
use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::*;
mod tasks;
use tasks::{
    CancelConditionalTimerTask, CancelWorkflowRunTask, ControlGoalRunTask,
    ExecuteWorktreeHandoffTask, FireConditionalTimerNowTask, ListConditionalTimersTask,
    ListGoalRunsTask, ListMonitorWatchesTask, ListWorkflowRunsTask, PreviewWorktreeHandoffTask,
    StopMonitorWatchTask, UpdateGoalRunTask,
};

#[napi]
impl OrbitHarmonyCore {
    #[napi]
    pub fn preview_worktree_handoff_json(
        &self,
        req_json: String,
    ) -> AsyncTask<PreviewWorktreeHandoffTask> {
        AsyncTask::new(PreviewWorktreeHandoffTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn execute_worktree_handoff_json(
        &self,
        req_json: String,
    ) -> AsyncTask<ExecuteWorktreeHandoffTask> {
        AsyncTask::new(ExecuteWorktreeHandoffTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn fetch_workspace_board(&self, host_id: String, workspace_id: String) {
        self.facade.fetch_workspace_board(&host_id, &workspace_id);
    }

    #[napi]
    pub fn workspace_board_json(&self, host_id: String, workspace_id: String) -> Result<String> {
        self.facade
            .workspace_board_json(&host_id, &workspace_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn update_workspace_board(
        &self,
        host_id: String,
        workspace_id: String,
        content: String,
        section: Option<String>,
        updated_by: Option<String>,
    ) {
        self.facade
            .update_workspace_board(&host_id, &workspace_id, content, section, updated_by);
    }

    #[napi]
    pub fn fetch_workspace_events(
        &self,
        host_id: String,
        workspace_id: String,
        limit: Option<u32>,
    ) {
        self.facade
            .fetch_workspace_events(&host_id, &workspace_id, limit);
    }

    #[napi]
    pub fn workspace_events_json(&self, host_id: String, workspace_id: String) -> Result<String> {
        self.facade
            .workspace_events_json(&host_id, &workspace_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn spawn_workspace_peer_json(&self, req_json: String) -> Result<String> {
        self.facade
            .spawn_workspace_peer_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn create_schedule_json(&self, req_json: String) -> Result<()> {
        self.facade
            .create_schedule_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn update_schedule_json(&self, req_json: String) -> Result<()> {
        self.facade
            .update_schedule_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn pause_schedule(&self, host_id: String, schedule_id: String) {
        self.facade.pause_schedule(&host_id, &schedule_id);
    }

    #[napi]
    pub fn resume_schedule(&self, host_id: String, schedule_id: String) {
        self.facade.resume_schedule(&host_id, &schedule_id);
    }

    #[napi]
    pub fn run_schedule_once(&self, host_id: String, schedule_id: String) {
        self.facade.run_schedule_once(&host_id, &schedule_id);
    }

    #[napi]
    pub fn delete_schedule(&self, host_id: String, schedule_id: String) {
        self.facade.delete_schedule(&host_id, &schedule_id);
    }

    #[napi]
    pub fn fetch_schedule_runs(&self, host_id: String, schedule_id: String, limit: Option<u32>) {
        self.facade
            .fetch_schedule_runs(&host_id, &schedule_id, limit);
    }

    #[napi]
    pub fn schedule_runs_json(&self, host_id: String, schedule_id: String) -> Result<String> {
        self.facade
            .schedule_runs_json(&host_id, &schedule_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn list_conditional_timers_json(
        &self,
        host_id: String,
        agent_id: Option<String>,
        include_resolved: bool,
    ) -> AsyncTask<ListConditionalTimersTask> {
        AsyncTask::new(ListConditionalTimersTask {
            facade: self.facade.clone(),
            host_id,
            agent_id,
            include_resolved,
        })
    }

    #[napi]
    pub fn cancel_conditional_timer_json(
        &self,
        host_id: String,
        timer_id: String,
    ) -> AsyncTask<CancelConditionalTimerTask> {
        AsyncTask::new(CancelConditionalTimerTask {
            facade: self.facade.clone(),
            host_id,
            timer_id,
        })
    }

    #[napi]
    pub fn fire_conditional_timer_now_json(
        &self,
        host_id: String,
        timer_id: String,
    ) -> AsyncTask<FireConditionalTimerNowTask> {
        AsyncTask::new(FireConditionalTimerNowTask {
            facade: self.facade.clone(),
            host_id,
            timer_id,
        })
    }

    #[napi]
    pub fn list_goal_runs_json(
        &self,
        host_id: String,
        agent_id: Option<String>,
    ) -> AsyncTask<ListGoalRunsTask> {
        AsyncTask::new(ListGoalRunsTask {
            facade: self.facade.clone(),
            host_id,
            agent_id,
        })
    }

    #[napi]
    pub fn control_goal_run_json(&self, req_json: String) -> AsyncTask<ControlGoalRunTask> {
        AsyncTask::new(ControlGoalRunTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn update_goal_run_json(&self, req_json: String) -> AsyncTask<UpdateGoalRunTask> {
        AsyncTask::new(UpdateGoalRunTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn list_monitor_watches_json(
        &self,
        host_id: String,
        agent_id: Option<String>,
    ) -> AsyncTask<ListMonitorWatchesTask> {
        AsyncTask::new(ListMonitorWatchesTask {
            facade: self.facade.clone(),
            host_id,
            agent_id,
        })
    }

    #[napi]
    pub fn stop_monitor_watch_json(&self, req_json: String) -> AsyncTask<StopMonitorWatchTask> {
        AsyncTask::new(StopMonitorWatchTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn list_workflow_runs_json(
        &self,
        host_id: String,
        include_completed: bool,
    ) -> AsyncTask<ListWorkflowRunsTask> {
        AsyncTask::new(ListWorkflowRunsTask {
            facade: self.facade.clone(),
            host_id,
            include_completed,
        })
    }

    #[napi]
    pub fn cancel_workflow_run_json(&self, req_json: String) -> AsyncTask<CancelWorkflowRunTask> {
        AsyncTask::new(CancelWorkflowRunTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn archive_workspace(
        &self,
        host_id: String,
        workspace_id: String,
        keep_agents_unclassified: bool,
    ) {
        self.facade
            .archive_workspace(host_id, workspace_id, keep_agents_unclassified)
    }

    #[napi]
    pub fn cancel_timer_json(
        &self,
        host_id: String,
        timer_id: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.cancel_timer_json(host_id, timer_id)
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[napi]
    pub fn create_mission_workspace(
        &self,
        host_id: String,
        source_cwd: String,
        title: String,
        mission_goal: Option<String>,
        use_worktree: bool,
        worktree_branch: Option<String>,
        base_ref: Option<String>,
    ) {
        self.facade.create_mission_workspace(
            host_id,
            source_cwd,
            title,
            mission_goal,
            use_worktree,
            worktree_branch,
            base_ref,
        )
    }

    #[napi]
    pub fn create_monitor_json(&self, req_json: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.create_monitor_json(req_json)
        })
    }

    #[napi]
    pub fn create_timer_json(
        &self,
        host_id: String,
        agent_id: String,
        duration_seconds: i64,
        prompt: String,
        condition_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.create_timer_json(
                host_id,
                agent_id,
                duration_seconds.max(0) as u64,
                prompt,
                condition_json,
            )
        })
    }

    #[napi]
    pub fn dismiss_cluster_suggestion(&self, host_id: String, agent_id: String) {
        self.facade.dismiss_cluster_suggestion(&host_id, &agent_id)
    }

    #[napi]
    pub fn merge_workspaces(
        &self,
        host_id: String,
        source_workspace_id: String,
        target_workspace_id: String,
    ) {
        self.facade
            .merge_workspaces(&host_id, &source_workspace_id, &target_workspace_id)
    }

    #[napi]
    pub fn move_agent_to_workspace(
        &self,
        host_id: String,
        agent_id: String,
        target_workspace_id: Option<String>,
    ) {
        self.facade
            .move_agent_to_workspace(&host_id, &agent_id, target_workspace_id)
    }

    #[napi]
    pub fn move_agent_to_workspace_v2(
        &self,
        host_id: String,
        agent_id: String,
        target_workspace_id: Option<String>,
        new_workspace_title: Option<String>,
        make_unclassified: bool,
    ) {
        self.facade.move_agent_to_workspace_v2(
            host_id,
            agent_id,
            target_workspace_id,
            new_workspace_title,
            make_unclassified,
        )
    }

    #[napi]
    pub fn organize_project_workspaces(
        &self,
        host_id: String,
        project_root: String,
        agent_ids: Vec<String>,
        auto_apply: bool,
    ) {
        self.facade
            .organize_project_workspaces(host_id, project_root, agent_ids, auto_apply)
    }

    #[napi]
    pub fn rename_workspace(&self, host_id: String, workspace_id: String, title: String) {
        self.facade.rename_workspace(host_id, workspace_id, title)
    }

    #[napi]
    pub fn start_goal_json(&self, req_json: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| facade.start_goal_json(req_json))
    }

    #[napi]
    pub fn start_workflow_json(&self, req_json: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.start_workflow_json(req_json)
        })
    }

    #[napi]
    pub fn update_workspace_goal(&self, host_id: String, workspace_id: String, goal: String) {
        self.facade
            .update_workspace_goal(host_id, workspace_id, goal)
    }
}
