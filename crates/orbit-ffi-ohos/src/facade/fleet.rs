use super::{OhosCoreFacade, parse_sidebar_filter, parse_sidebar_sort, parse_string_vec_json};
use crate::dto::OhosThreadPrefSeed;
use orbit_client_core::app_core::ClientThreadPref;
use orbit_client_core::contract::FleetSidebarRequest;

impl OhosCoreFacade {
    pub fn discover_local_daemon_uri(&self) -> Option<String> {
        self.inner.discover_local_daemon_uri()
    }

    pub fn fleet_overview_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.fleet_overview()).map_err(|e| e.to_string())
    }

    pub fn bridge_info_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.bridge_info()).map_err(|e| e.to_string())
    }

    pub fn search_messages_json(&self, query: &str, limit: Option<u32>) -> Result<String, String> {
        serde_json::to_string(&self.inner.search_messages(query, limit.unwrap_or(20)))
            .map_err(|e| e.to_string())
    }

    pub fn fleet_sidebar_by_request_json(&self, req_json: &str) -> Result<String, String> {
        let req: FleetSidebarRequest = serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        serde_json::to_string(&self.inner.fleet_sidebar_by_request(&req)).map_err(|e| e.to_string())
    }

    pub fn host_detail_json(&self, host_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.host_detail(host_id)).map_err(|e| e.to_string())
    }

    pub fn fleet_sidebar_json(
        &self,
        sort: Option<&str>,
        filter: Option<&str>,
        query: Option<&str>,
        active_host_id: Option<&str>,
        active_agent_id: Option<&str>,
        active_cwd: Option<&str>,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.fleet_sidebar(
            parse_sidebar_sort(sort),
            parse_sidebar_filter(filter),
            query.unwrap_or(""),
            active_host_id,
            active_agent_id,
            active_cwd,
        ))
        .map_err(|e| e.to_string())
    }

    pub fn mark_agent_seen(&self, host_id: &str, agent_id: &str, seq: u64) {
        self.inner.mark_agent_seen(host_id, agent_id, seq);
    }

    pub fn mark_agent_unread(&self, host_id: &str, agent_id: &str) {
        self.inner.mark_agent_unread(host_id, agent_id);
    }

    pub fn set_agent_pinned(&self, host_id: &str, agent_id: &str, pinned: bool) {
        self.inner.set_agent_pinned(host_id, agent_id, pinned);
    }

    pub fn toggle_pin_agent(&self, host_id: &str, agent_id: &str) -> bool {
        self.inner.toggle_pin_agent(host_id, agent_id)
    }

    pub fn seed_thread_prefs_json(&self, prefs_json: &str) -> Result<(), String> {
        let seeds: Vec<OhosThreadPrefSeed> =
            serde_json::from_str(prefs_json).map_err(|e| e.to_string())?;
        let prefs: Vec<ClientThreadPref> = seeds
            .into_iter()
            .map(|s| ClientThreadPref {
                host_id: s.host_id,
                agent_id: s.agent_id,
                seen_seq: s.seen_seq,
                is_pinned: s.is_pinned,
                pinned_order: s.pinned_order,
                updated_at_ms: s.updated_at_ms,
            })
            .collect();
        self.inner.seed_thread_prefs(prefs);
        Ok(())
    }

    pub fn upsert_project_json(
        &self,
        project_id: Option<&str>,
        host_id: &str,
        name: &str,
        directories_json: &str,
    ) -> Result<String, String> {
        let dirs = parse_string_vec_json(Some(directories_json))?;
        let proj = self
            .inner
            .upsert_project(project_id, host_id, name, dirs)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&proj).map_err(|e| e.to_string())
    }

    pub fn remove_project(&self, project_id: &str) -> Result<(), String> {
        self.inner
            .remove_project(project_id)
            .map_err(|e| e.to_string())
    }

    pub fn connect_all_hosts(&self) {
        self.inner.connect_all_hosts();
    }

    pub fn connect_host(&self, host_id: &str) {
        self.inner.connect_host_async(host_id.to_string());
    }

    pub fn refresh_host(&self, host_id: &str) {
        self.inner.refresh_host(host_id);
    }

    pub fn refresh_quotas(&self, host_id: &str) {
        self.inner.refresh_quotas(host_id);
    }

    pub fn reorder_projects_json(&self, ordered_keys_json: &str) -> Result<String, String> {
        let keys = parse_string_vec_json(Some(ordered_keys_json))?;
        let saved = self
            .inner
            .reorder_projects(keys)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&saved).map_err(|e| e.to_string())
    }

    pub fn project_order_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.project_order()).map_err(|e| e.to_string())
    }

    pub fn search_messages_remote_json(&self, req_json: String) -> Result<String, String> {
        serde_json::to_string(&self.inner.search_messages_remote_blocking(
            serde_json::from_str(&req_json).map_err(|e| e.to_string())?,
        ))
        .map_err(|e| e.to_string())
    }

    pub fn discover_fleet_router_hosts_json(&self) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.discover_fleet_router_hosts())?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn get_fleet_mesh_snapshot_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.get_fleet_mesh_snapshot()?).map_err(|e| e.to_string())
    }

    pub fn get_session_usage_json(
        &self,
        host_id: String,
        agent_id: String,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.get_session_usage(&host_id, &agent_id))?,
        )
        .map_err(|e| e.to_string())
    }
}
