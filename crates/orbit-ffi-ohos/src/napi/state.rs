use super::core::OrbitHarmonyCore;
use super::{MobileRpcTask, run_mobile_job};
use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::*;

#[napi]
impl OrbitHarmonyCore {
    #[napi]
    pub fn personal_agent_briefing_json(
        &self,
        primary_router_host_id: Option<String>,
    ) -> Result<String> {
        self.facade
            .personal_agent_briefing_json(primary_router_host_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn agent_stage_turns_slice_json(
        &self,
        host_id: String,
        agent_id: String,
        start_turn_index: u32,
        count: u32,
    ) -> Result<String> {
        self.facade
            .agent_stage_turns_slice_json(&host_id, &agent_id, start_turn_index, count)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn list_personal_agent_activity_json(
        &self,
        host_id: String,
        limit: u32,
        since_ms: Option<i64>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.list_personal_agent_activity_json(&host_id, limit, since_ms)
        })
    }

    #[napi]
    pub fn fleet_overview_json_async(&self) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, |facade| facade.fleet_overview_json())
    }

    #[napi]
    pub fn host_detail_json_async(&self, host_id: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.host_detail_json(&host_id)
        })
    }

    #[napi]
    pub fn fleet_sidebar_by_request_json_async(
        &self,
        req_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.fleet_sidebar_by_request_json(&req_json)
        })
    }

    #[napi]
    pub fn agent_stage_delta_by_request_json_async(
        &self,
        req_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.agent_stage_delta_by_request_json(&req_json)
        })
    }

    #[napi]
    pub fn get_composer_draft_json(&self, draft_key: String) -> Result<Option<String>> {
        self.facade
            .get_composer_draft_json(&draft_key)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn set_composer_draft(&self, draft_key: String, draft_json: String) -> Result<()> {
        self.facade
            .set_composer_draft(&draft_key, &draft_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn delete_composer_draft(&self, draft_key: String) -> Result<()> {
        self.facade
            .delete_composer_draft(&draft_key)
            .map_err(Error::from_reason)
    }
}
