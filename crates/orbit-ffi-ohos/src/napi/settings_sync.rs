use super::core::OrbitHarmonyCore;
use super::{MobileRpcTask, run_mobile_job};
use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::*;
mod tasks;
use tasks::{
    CloudSyncEnableManualPassphraseTask, CloudSyncImportDeviceLinkUriTask,
    CloudSyncLoginWithTokenTask, CloudSyncNowTask, CloudSyncPollGithubDeviceFlowTask,
    CloudSyncSetManualPassphraseModeTask, CloudSyncStartGithubDeviceFlowTask,
    CloudSyncUnlockWithPassphraseTask, ListPushDevicesTask, RegisterPushDeviceTask,
    UnregisterPushDeviceTask,
};

#[napi]
impl OrbitHarmonyCore {
    #[napi]
    pub fn update_host_config_request_json(&self, req_json: String) -> Result<String> {
        self.facade
            .update_host_config_request_json(&req_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn pair_host_json(&self, pairing_uri: String) -> Result<String> {
        self.facade
            .pair_host_json(&pairing_uri)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn remove_host(&self, host_id: String) -> Result<()> {
        self.facade
            .remove_host(&host_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn update_host_relay(&self, host_id: String, relay_url: Option<String>) -> Result<()> {
        self.facade
            .update_host_relay(&host_id, relay_url.as_deref())
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn apply_relay_to_fleet(&self, raw_relay_url: String) -> Result<u32> {
        self.facade
            .apply_relay_to_fleet(&raw_relay_url)
            .map_err(Error::from_reason)
    }

    #[napi]
    #[allow(clippy::too_many_arguments)]
    pub fn update_host_config_json(
        &self,
        host_id: String,
        custom_name: Option<String>,
        route_mode: Option<String>,
        ssh_alias: Option<String>,
        remote_daemon_port: Option<u32>,
        custom_direct_endpoint: Option<String>,
        relay_url: Option<String>,
        reconnect: Option<bool>,
    ) -> Result<String> {
        self.facade
            .update_host_config_json(
                &host_id,
                custom_name,
                route_mode.as_deref(),
                ssh_alias,
                remote_daemon_port.map(|p| p as u16),
                custom_direct_endpoint,
                relay_url,
                reconnect.unwrap_or(true),
            )
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn refresh_provider_models(&self, host_id: String, force_refresh: bool) {
        self.facade.refresh_provider_models(&host_id, force_refresh);
    }

    #[napi]
    pub fn set_session_mode(&self, host_id: String, agent_id: String, mode: String) {
        self.facade.set_session_mode(&host_id, &agent_id, &mode);
    }

    #[napi]
    pub fn set_agent_config(
        &self,
        host_id: String,
        agent_id: String,
        model: Option<String>,
        effort: Option<String>,
    ) {
        self.facade
            .set_agent_config(&host_id, &agent_id, model, effort.as_deref());
    }

    #[napi]
    pub fn set_agent_fast_mode(&self, host_id: String, agent_id: String, fast_mode: bool) {
        self.facade
            .set_agent_fast_mode(&host_id, &agent_id, fast_mode);
    }

    #[napi]
    pub fn switch_agent_provider(
        &self,
        host_id: String,
        agent_id: String,
        provider: String,
        model: Option<String>,
        effort: Option<String>,
        acp_agent_id: Option<String>,
    ) {
        self.facade.switch_agent_provider(
            &host_id,
            &agent_id,
            &provider,
            model,
            effort.as_deref(),
            acp_agent_id,
        );
    }

    #[napi]
    pub fn resolve_permission(
        &self,
        host_id: String,
        agent_id: String,
        permission_request_id: String,
        decision: String,
    ) {
        self.facade
            .resolve_permission(&host_id, &agent_id, &permission_request_id, &decision);
    }

    #[napi]
    pub fn answer_question(
        &self,
        host_id: String,
        agent_id: String,
        question_request_id: String,
        answers_json: String,
    ) -> Result<()> {
        self.facade
            .answer_question(&host_id, &agent_id, &question_request_id, &answers_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn cloud_sync_status_json(&self) -> Result<String> {
        self.facade
            .cloud_sync_status_json()
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn cloud_sync_generate_passphrase(&self) -> String {
        self.facade.cloud_sync_generate_passphrase()
    }

    #[napi]
    pub fn cloud_sync_set_github_client_id_json(
        &self,
        client_id: Option<String>,
    ) -> Result<String> {
        self.facade
            .cloud_sync_set_github_client_id_json(client_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn cloud_sync_set_auto_sync_json(&self, enabled: bool) -> Result<String> {
        self.facade
            .cloud_sync_set_auto_sync_json(enabled)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn cloud_sync_start_github_device_flow_json(
        &self,
        client_id_override: Option<String>,
    ) -> AsyncTask<CloudSyncStartGithubDeviceFlowTask> {
        AsyncTask::new(CloudSyncStartGithubDeviceFlowTask {
            facade: self.facade.clone(),
            client_id_override,
        })
    }

    #[napi]
    pub fn cloud_sync_poll_github_device_flow_json(
        &self,
        device_code: String,
        client_id_override: Option<String>,
    ) -> AsyncTask<CloudSyncPollGithubDeviceFlowTask> {
        AsyncTask::new(CloudSyncPollGithubDeviceFlowTask {
            facade: self.facade.clone(),
            device_code,
            client_id_override,
        })
    }

    #[napi]
    pub fn cloud_sync_login_with_token_json(
        &self,
        token: String,
    ) -> AsyncTask<CloudSyncLoginWithTokenTask> {
        AsyncTask::new(CloudSyncLoginWithTokenTask {
            facade: self.facade.clone(),
            token,
        })
    }

    #[napi]
    pub fn cloud_sync_unlock_with_passphrase_json(
        &self,
        passphrase: String,
    ) -> AsyncTask<CloudSyncUnlockWithPassphraseTask> {
        AsyncTask::new(CloudSyncUnlockWithPassphraseTask {
            facade: self.facade.clone(),
            passphrase,
        })
    }

    #[napi]
    pub fn cloud_sync_now_json(&self) -> AsyncTask<CloudSyncNowTask> {
        AsyncTask::new(CloudSyncNowTask {
            facade: self.facade.clone(),
        })
    }

    #[napi]
    pub fn cloud_sync_export_device_link_uri(&self) -> Result<String> {
        self.facade
            .cloud_sync_export_device_link_uri()
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn cloud_sync_import_device_link_uri_json(
        &self,
        uri: String,
    ) -> AsyncTask<CloudSyncImportDeviceLinkUriTask> {
        AsyncTask::new(CloudSyncImportDeviceLinkUriTask {
            facade: self.facade.clone(),
            uri,
        })
    }

    #[napi]
    pub fn cloud_sync_enable_manual_passphrase_json(
        &self,
        passphrase: String,
    ) -> AsyncTask<CloudSyncEnableManualPassphraseTask> {
        AsyncTask::new(CloudSyncEnableManualPassphraseTask {
            facade: self.facade.clone(),
            passphrase,
        })
    }

    #[napi]
    pub fn cloud_sync_set_manual_passphrase_mode_json(
        &self,
        require_manual: bool,
    ) -> AsyncTask<CloudSyncSetManualPassphraseModeTask> {
        AsyncTask::new(CloudSyncSetManualPassphraseModeTask {
            facade: self.facade.clone(),
            require_manual,
        })
    }

    #[napi]
    pub fn cloud_sync_logout_json(&self, clear_local_hosts: bool) -> Result<String> {
        self.facade
            .cloud_sync_logout_json(clear_local_hosts)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn cloud_sync_get_preferences_json(&self) -> Result<String> {
        self.facade
            .cloud_sync_get_preferences_json()
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn cloud_sync_update_preferences_json(&self, patch_json: String) -> Result<()> {
        self.facade
            .cloud_sync_update_preferences_json(&patch_json)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn register_push_device_json(
        &self,
        host_id: String,
        req_json: String,
    ) -> AsyncTask<RegisterPushDeviceTask> {
        AsyncTask::new(RegisterPushDeviceTask {
            facade: self.facade.clone(),
            host_id,
            req_json,
        })
    }

    #[napi]
    pub fn unregister_push_device_json(
        &self,
        host_id: String,
        device_id: String,
    ) -> AsyncTask<UnregisterPushDeviceTask> {
        AsyncTask::new(UnregisterPushDeviceTask {
            facade: self.facade.clone(),
            host_id,
            device_id,
        })
    }

    #[napi]
    pub fn list_push_devices_json(&self, host_id: String) -> AsyncTask<ListPushDevicesTask> {
        AsyncTask::new(ListPushDevicesTask {
            facade: self.facade.clone(),
            host_id,
        })
    }

    #[napi]
    pub fn delete_custom_acp_agent_json(
        &self,
        host_id: String,
        agent_id: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.delete_custom_acp_agent_json(&host_id, &agent_id)
        })
    }

    #[napi]
    pub fn delete_permission_rule_json(
        &self,
        host_id: String,
        rule_id: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.delete_permission_rule_json(host_id, rule_id)
        })
    }

    #[napi]
    pub fn get_guardian_config_json(&self, host_id: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.get_guardian_config_json(host_id)
        })
    }

    #[napi]
    pub fn get_router_config_json(&self, host_id: String) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.get_router_config_json(host_id)
        })
    }

    #[napi]
    pub fn list_acp_agents_json(&self, host_id: String) -> Result<String> {
        self.facade
            .list_acp_agents_json(&host_id)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn list_permission_rules_json(
        &self,
        host_id: String,
        project_cwd: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.list_permission_rules_json(host_id, project_cwd)
        })
    }

    #[napi]
    pub fn push_registration_status_json(&self) -> Result<String> {
        self.facade
            .push_registration_status_json()
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn refresh_acp_agents(&self, host_id: String) {
        self.facade.refresh_acp_agents(&host_id)
    }

    #[napi]
    pub fn refresh_acp_registry_json(
        &self,
        host_id: String,
        registry_url: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.refresh_acp_registry_json(&host_id, registry_url)
        })
    }

    #[napi]
    pub fn register_live_activity_token_json(
        &self,
        host_id: String,
        device_id: String,
        live_activity_token: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.register_live_activity_token_json(host_id, device_id, live_activity_token)
        })
    }

    #[napi]
    pub fn register_push_token_json(&self, platform: String, token: String) -> Result<String> {
        self.facade
            .register_push_token_json(platform, token)
            .map_err(Error::from_reason)
    }

    #[napi]
    pub fn resolve_permissions_batch_json(
        &self,
        host_id: String,
        items_json: String,
        decision: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.resolve_permissions_batch_json(host_id, items_json, decision)
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[napi]
    pub fn resolve_plan_approval_json(
        &self,
        host_id: String,
        agent_id: String,
        permission_request_id: String,
        action_json: String,
        edited_plan_markdown: Option<String>,
        feedback: Option<String>,
        target_mode_json: String,
    ) -> AsyncTask<MobileRpcTask<()>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.resolve_plan_approval_json(
                host_id,
                agent_id,
                permission_request_id,
                action_json,
                edited_plan_markdown,
                feedback,
                target_mode_json,
            )
        })
    }

    #[napi]
    pub fn test_acp_agent_json(
        &self,
        host_id: String,
        agent_id: String,
        cwd: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.test_acp_agent_json(&host_id, &agent_id, cwd)
        })
    }

    #[napi]
    pub fn test_push_notification_json(
        &self,
        host_id: String,
        device_id: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.test_push_notification_json(host_id, device_id)
        })
    }

    #[napi]
    pub fn unregister_push_token(&self, platform: String) -> Result<()> {
        self.facade
            .unregister_push_token(platform)
            .map_err(Error::from_reason)
    }

    #[allow(clippy::too_many_arguments)]
    #[napi]
    pub fn update_agent_identity(
        &self,
        host_id: String,
        agent_id: String,
        handle: Option<String>,
        title: Option<String>,
        task_summary: Option<String>,
        lock_handle: bool,
        lock_title: bool,
    ) {
        self.facade.update_agent_identity(
            host_id,
            agent_id,
            handle,
            title,
            task_summary,
            lock_handle,
            lock_title,
        )
    }

    #[napi]
    pub fn update_guardian_config_json(
        &self,
        host_id: String,
        config_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.update_guardian_config_json(host_id, config_json)
        })
    }

    #[napi]
    pub fn update_mesh_peer_json(
        &self,
        host_id: String,
        registration_id: String,
        label: Option<String>,
        enabled: Option<bool>,
    ) -> AsyncTask<MobileRpcTask<()>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.update_mesh_peer_json(host_id, registration_id, label, enabled)
        })
    }

    #[napi]
    pub fn update_push_preferences_json(
        &self,
        host_id: String,
        req_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.update_push_preferences_json(host_id, req_json)
        })
    }

    #[napi]
    pub fn update_router_config_json(
        &self,
        host_id: String,
        config_json: String,
        external_webhook_secret: Option<String>,
        activate: bool,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.update_router_config_json(
                host_id,
                config_json,
                external_webhook_secret,
                activate,
            )
        })
    }

    #[napi]
    pub fn upgrade_host_daemon_over_wire(
        &self,
        host_id: String,
        target_version: Option<String>,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.upgrade_host_daemon_over_wire(host_id, target_version)
        })
    }

    #[napi]
    pub fn upsert_custom_acp_agent_json(
        &self,
        host_id: String,
        spec_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.upsert_custom_acp_agent_json(&host_id, &spec_json)
        })
    }

    #[napi]
    pub fn upsert_permission_rule_json(
        &self,
        host_id: String,
        rule_json: String,
    ) -> AsyncTask<MobileRpcTask<String>> {
        run_mobile_job(&self.facade, move |facade| {
            facade.upsert_permission_rule_json(host_id, rule_json)
        })
    }
}
