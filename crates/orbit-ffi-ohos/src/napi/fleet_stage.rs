use super::core::OrbitHarmonyCore;
use super::{MobileRpcTask, run_mobile_job};
use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::*;
mod tasks;
use tasks::{ForkAgentTask, RenderDiagramTask, RewindAgentTask};

#[napi]
impl OrbitHarmonyCore {
    #[napi]
    pub fn debug_replay_stage_deltas_json(
        &self,
        host_id: String,
        agent_id: String,
        frames_json: String,
    ) -> Result<()> {
        self.facade
            .debug_replay_stage_deltas_json(&host_id, &agent_id, &frames_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn discover_local_daemon_uri(&self) -> Option<String> {
        self.facade.discover_local_daemon_uri()
    }

    #[napi]
    pub fn fleet_overview_json(&self) -> Result<String> {
        self.facade
            .fleet_overview_json()
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn bridge_info_json(&self) -> Result<String> {
        self.facade.bridge_info_json().map_err(Error::from_reason)
    }

    #[napi]
    pub fn search_messages_json(&self, query: String, limit: Option<u32>) -> Result<String> {
        self.facade
            .search_messages_json(&query, limit)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn export_agent_transcript_json(
        &self,
        host_id: String,
        agent_id: String,
        options_json: String,
    ) -> Result<String> {
        self.facade
            .export_agent_transcript_json(&host_id, &agent_id, &options_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn list_agent_code_blocks_json(&self, host_id: String, agent_id: String) -> Result<String> {
        self.facade
            .list_agent_code_blocks_json(&host_id, &agent_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn fleet_sidebar_by_request_json(&self, req_json: String) -> Result<String> {
        self.facade
            .fleet_sidebar_by_request_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn agent_stage_delta_by_request_json(&self, req_json: String) -> Result<String> {
        self.facade
            .agent_stage_delta_by_request_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn host_detail_json(&self, host_id: String) -> Result<String> {
        self.facade
            .host_detail_json(&host_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn agent_stage_json(&self, host_id: String, agent_id: String) -> Result<String> {
        self.facade
            .agent_stage_json(&host_id, &agent_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn agent_stage_delta_json(
        &self,
        host_id: String,
        agent_id: String,
        since_revision: i64,
    ) -> Result<String> {
        self.facade
            .agent_stage_delta_json(&host_id, &agent_id, since_revision.max(0) as u64)
            .map_err(Error::from_reason)
    }

    #[napi]
    #[allow(clippy::too_many_arguments)]
    pub fn fleet_sidebar_json(
        &self,
        sort: Option<String>,
        filter: Option<String>,
        query: Option<String>,
        active_host_id: Option<String>,
        active_agent_id: Option<String>,
        active_cwd: Option<String>,
    ) -> Result<String> {
        self.facade
            .fleet_sidebar_json(
                sort.as_deref(),
                filter.as_deref(),
                query.as_deref(),
                active_host_id.as_deref(),
                active_agent_id.as_deref(),
                active_cwd.as_deref(),
            )
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn mark_agent_seen(&self, host_id: String, agent_id: String, seq: i64) {
        self.facade
            .mark_agent_seen(&host_id, &agent_id, seq.max(0) as u64);
    }

    #[napi]
    pub fn mark_agent_unread(&self, host_id: String, agent_id: String) {
        self.facade.mark_agent_unread(&host_id, &agent_id);
    }

    #[napi]
    pub fn set_agent_pinned(&self, host_id: String, agent_id: String, pinned: bool) {
        self.facade.set_agent_pinned(&host_id, &agent_id, pinned);
    }

    #[napi]
    pub fn toggle_pin_agent(&self, host_id: String, agent_id: String) -> bool {
        self.facade.toggle_pin_agent(&host_id, &agent_id)
    }

    #[napi]
    pub fn seed_thread_prefs_json(&self, prefs_json: String) -> Result<()> {
        self.facade
            .seed_thread_prefs_json(&prefs_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn agent_timeline_json(&self, host_id: String, agent_id: String) -> Result<String> {
        self.facade
            .agent_timeline_json(&host_id, &agent_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn agent_timeline_delta_json(
        &self,
        host_id: String,
        agent_id: String,
        since_seq: i64,
    ) -> Result<String> {
        self.facade
            .agent_timeline_delta_json(&host_id, &agent_id, since_seq.max(0) as u64)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn describe_tool_item_json(
        &self,
        item_json: String,
        cwd: Option<String>,
    ) -> Result<Option<String>> {
        self.facade
            .describe_tool_item_json(&item_json, cwd.as_deref())
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn upsert_project_json(
        &self,
        project_id: Option<String>,
        host_id: String,
        name: String,
        directories_json: String,
    ) -> Result<String> {
        self.facade
            .upsert_project_json(project_id.as_deref(), &host_id, &name, &directories_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn remove_project(&self, project_id: String) -> Result<()> {
        self.facade
            .remove_project(&project_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn connect_all_hosts(&self) {
        self.facade.connect_all_hosts();
    }

    #[napi]
    pub fn connect_host(&self, host_id: String) {
        self.facade.connect_host(&host_id);
    }

    #[napi]
    pub fn refresh_host(&self, host_id: String) {
        self.facade.refresh_host(&host_id);
    }

    #[napi]
    pub fn refresh_quotas(&self, host_id: String) {
        self.facade.refresh_quotas(&host_id);
    }

    #[napi]
    pub fn open_agent_timeline(&self, host_id: String, agent_id: String) {
        self.facade.open_agent_timeline(&host_id, &agent_id);
    }

    #[napi]
    pub fn load_older_agent_timeline(
        &self,
        host_id: String,
        agent_id: String,
        limit_events: u32,
    ) -> bool {
        self.facade
            .load_older_agent_timeline(&host_id, &agent_id, limit_events)
    }

    #[napi]
    pub fn interrupt_agent(&self, host_id: String, agent_id: String) {
        self.facade.interrupt_agent(&host_id, &agent_id);
    }

    #[napi]
    pub fn interrupt_agent_and_continue(&self, host_id: String, agent_id: String) {
        self.facade
            .interrupt_agent_and_continue(&host_id, &agent_id);
    }

    #[napi]
    pub fn archive_agent(&self, host_id: String, agent_id: String) {
        self.facade.archive_agent(&host_id, &agent_id);
    }

    #[napi]
    pub fn unarchive_agent(&self, host_id: String, agent_id: String) {
        self.facade.unarchive_agent(&host_id, &agent_id);
    }

    #[napi]
    pub fn delete_agent(&self, host_id: String, agent_id: String) {
        self.facade.delete_agent(&host_id, &agent_id);
    }

    #[napi]
    pub fn rewind_agent_json(
        &self,
        host_id: String,
        agent_id: String,
        to_before_seq: i64,
        restore_files: bool,
    ) -> AsyncTask<RewindAgentTask> {
        AsyncTask::new(RewindAgentTask {
            facade: self.facade.clone(),
            host_id,
            agent_id,
            to_before_seq: to_before_seq.max(0) as u64,
            restore_files,
        })
    }

    #[napi]
    pub fn fork_agent_json(&self, req_json: String) -> AsyncTask<ForkAgentTask> {
        AsyncTask::new(ForkAgentTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn detect_diagram_kind(&self, language: String, source: String) -> Option<String> {
        self.facade.detect_diagram_kind(&language, &source)
    }

    #[napi]
    pub fn render_diagram_json(&self, req_json: String) -> AsyncTask<RenderDiagramTask> {
        AsyncTask::new(RenderDiagramTask {
            facade: self.facade.clone(),
            req_json,
        })
    }

    #[napi]
    pub fn parse_markdown_preview_json(&self, markdown: String, streaming: bool) -> Result<String> {
        self.facade
            .parse_markdown_preview_json(&markdown, streaming)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn detach_agent(&self, host_id: String, agent_id: String) {
        self.facade.detach_agent(host_id, agent_id)
    }

    #[napi]
    pub fn discover_fleet_router_hosts_json(&self) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.discover_fleet_router_hosts_json()
        })
    }

    #[napi]
    pub fn get_fleet_mesh_snapshot_json(&self) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.get_fleet_mesh_snapshot_json()
        })
    }

    #[napi]
    pub fn get_session_usage_json(
        &self,
        host_id: String,
        agent_id: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.get_session_usage_json(host_id, agent_id)
        })
    }

    #[napi]
    pub fn list_provider_subagents(&self, host_id: String, parent_agent_id: String) {
        self.facade
            .list_provider_subagents(host_id, parent_agent_id)
    }

    #[napi]
    pub fn open_provider_subagent_timeline(
        &self,
        host_id: String,
        parent_agent_id: String,
        subagent_id: String,
    ) {
        self.facade
            .open_provider_subagent_timeline(host_id, parent_agent_id, subagent_id)
    }

    #[napi]
    pub fn project_order_json(&self) -> Result<String> {
        self.facade.project_order_json().map_err(Error::from_reason)
    }

    #[napi]
    pub fn provider_subagent_timeline_json(
        &self,
        host_id: String,
        parent_agent_id: String,
        subagent_id: String,
    ) -> Result<String> {
        self.facade
            .provider_subagent_timeline_json(host_id, parent_agent_id, subagent_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn reorder_projects_json(&self, ordered_keys_json: String) -> Result<String> {
        self.facade
            .reorder_projects_json(&ordered_keys_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn rewind_agent_request_json(&self, req_json: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.rewind_agent_request_json(req_json)
        })
    }

    #[napi]
    pub fn search_messages_remote_json(
        &self,
        req_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.search_messages_remote_json(req_json)
        })
    }

    #[napi]
    pub fn summarize_agent(&self, host_id: String, agent_id: String) {
        self.facade.summarize_agent(host_id, agent_id)
    }
}
