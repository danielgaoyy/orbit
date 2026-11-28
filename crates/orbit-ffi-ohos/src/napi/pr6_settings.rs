use super::core::OrbitHarmonyCore;
use super::{MobileRpcTask, run_mobile_job};
use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::*;

#[napi]
impl OrbitHarmonyCore {
    #[napi]
    pub fn export_host_pairing_uri(&self, host_id: String) -> Result<String> {
        self.facade
            .export_host_pairing_uri(&host_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn get_usage_insights_json(
        &self,
        host_id: String,
        days: u32,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.get_usage_insights_json(&host_id, days)
        })
    }

    #[napi]
    pub fn list_external_mcp_servers_json(
        &self,
        host_id: String,
        cwd: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.list_external_mcp_servers_json(&host_id, cwd)
        })
    }

    #[napi]
    pub fn toggle_external_mcp_server_json(
        &self,
        host_id: String,
        server_id: String,
        enabled: bool,
        cwd: Option<String>,
    ) -> AsyncTask<MobileRpcTask<()>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.toggle_external_mcp_server_json(&host_id, &server_id, enabled, cwd)
        })
    }

    #[napi]
    pub fn list_managed_skills_json(
        &self,
        host_id: String,
        cwd: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.list_managed_skills_json(&host_id, cwd)
        })
    }

    #[napi]
    pub fn list_project_memories_json(
        &self,
        host_id: String,
        cwd: String,
        limit: u32,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.list_project_memories_json(&host_id, &cwd, limit)
        })
    }

    #[napi]
    pub fn confirm_project_memory_json(
        &self,
        host_id: String,
        memory_id: String,
        cwd: Option<String>,
    ) -> AsyncTask<MobileRpcTask<()>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.confirm_project_memory_json(&host_id, &memory_id, cwd)
        })
    }

    #[napi]
    pub fn delete_project_memory_json(
        &self,
        host_id: String,
        memory_id: String,
        cwd: Option<String>,
    ) -> AsyncTask<MobileRpcTask<()>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.delete_project_memory_json(&host_id, &memory_id, cwd)
        })
    }

    #[napi]
    pub fn list_plugins_json(&self, host_id: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.list_plugins_json(&host_id)
        })
    }

    #[napi]
    pub fn set_plugin_enabled_json(
        &self,
        host_id: String,
        plugin_id: String,
        enabled: bool,
    ) -> AsyncTask<MobileRpcTask<()>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.set_plugin_enabled_json(&host_id, &plugin_id, enabled)
        })
    }

    #[napi]
    pub fn get_sandbox_config_json(&self, host_id: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.get_sandbox_config_json(&host_id)
        })
    }
}
