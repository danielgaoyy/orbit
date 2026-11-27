use super::core::OrbitHarmonyCore;
use super::{MobileRpcTask, run_mobile_job};
use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::*;

#[napi]
impl OrbitHarmonyCore {
    #[napi]
    pub fn git_push_json(&self, req_json: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| facade.git_push_json(&req_json))
    }

    #[napi]
    pub fn git_discard_files_json(&self, req_json: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.git_discard_files_json(&req_json)
        })
    }

    #[napi]
    pub fn highlight_workspace_code_json(&self, req_json: String) -> Result<String> {
        self.facade
            .highlight_workspace_code_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn terminal_screen_delta_slot_json(
        &self,
        host_id: String,
        slot: u32,
        since_revision: i64,
    ) -> Result<String> {
        let normalized_slot = u8::try_from(slot.clamp(1, u32::from(u8::MAX))).unwrap_or(1);
        let since_rev = u64::try_from(since_revision).unwrap_or(0);
        self.facade
            .terminal_screen_delta_slot_json(&host_id, normalized_slot, since_rev)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn create_workspace_v2_json(&self, req_json: String) -> Result<()> {
        self.facade
            .create_workspace_v2_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn archive_workspace_v2_json(
        &self,
        host_id: String,
        workspace_id: String,
        keep_agents_unclassified: bool,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.archive_workspace_v2_json(&host_id, &workspace_id, keep_agents_unclassified)
        })
    }

    #[napi]
    pub fn unarchive_workspace_json(
        &self,
        host_id: String,
        workspace_id: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.unarchive_workspace_json(&host_id, &workspace_id)
        })
    }

    #[napi]
    pub fn rename_workspace_confirmed_json(
        &self,
        host_id: String,
        workspace_id: String,
        title: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.rename_workspace_confirmed_json(&host_id, &workspace_id, &title)
        })
    }

    #[napi]
    pub fn request_workspace_scripts(&self, host_id: String, workspace_id: String) -> Result<()> {
        self.facade
            .request_workspace_scripts(&host_id, &workspace_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn start_workspace_script_json(
        &self,
        host_id: String,
        workspace_id: String,
        script_name: String,
    ) -> Result<()> {
        self.facade
            .start_workspace_script_json(&host_id, &workspace_id, &script_name)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn stop_workspace_script_json(
        &self,
        host_id: String,
        workspace_id: String,
        script_name: String,
    ) -> Result<()> {
        self.facade
            .stop_workspace_script_json(&host_id, &workspace_id, &script_name)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn update_workspace_board_cas_json(
        &self,
        req_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.update_workspace_board_cas_json(&req_json)
        })
    }

    #[napi]
    pub fn start_terminal_slot(
        &self,
        host_id: String,
        cwd: String,
        cols: u32,
        rows: u32,
        slot: u32,
    ) {
        let normalized_cols = u16::try_from(cols.clamp(20, 500)).unwrap_or(80);
        let normalized_rows = u16::try_from(rows.clamp(4, 200)).unwrap_or(24);
        let normalized_slot = u8::try_from(slot.clamp(1, 250)).unwrap_or(1);
        self.facade.start_terminal_slot(
            &host_id,
            &cwd,
            normalized_cols,
            normalized_rows,
            normalized_slot,
        );
    }
}
