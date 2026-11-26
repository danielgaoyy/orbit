use super::OhosCoreFacade;
use orbit_client_core::app_core::RegisterPushDeviceRequest;

impl OhosCoreFacade {
    // --- Zero-Server E2EE Cloud Sync ---

    pub fn cloud_sync_status_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.cloud_sync_status()).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_generate_passphrase(&self) -> String {
        self.inner.cloud_sync_generate_passphrase()
    }

    pub fn cloud_sync_set_github_client_id_json(
        &self,
        client_id: Option<String>,
    ) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_set_github_client_id(client_id)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_set_auto_sync_json(&self, enabled: bool) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_set_auto_sync(enabled)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_start_github_device_flow_json(
        &self,
        client_id_override: Option<String>,
    ) -> Result<String, String> {
        let started = self
            .inner
            .cloud_sync_start_github_device_flow(client_id_override)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&started).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_poll_github_device_flow_json(
        &self,
        device_code: &str,
        client_id_override: Option<String>,
    ) -> Result<String, String> {
        let polled = self
            .inner
            .cloud_sync_poll_github_device_flow(device_code, client_id_override)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&polled).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_login_with_token_json(&self, token: &str) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_login_with_token(token)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_unlock_with_passphrase_json(
        &self,
        passphrase: &str,
    ) -> Result<String, String> {
        let res = self
            .inner
            .cloud_sync_unlock_with_passphrase(passphrase)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_now_json(&self) -> Result<String, String> {
        let res = self.inner.cloud_sync_now().map_err(|e| e.to_string())?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_export_device_link_uri(&self) -> Result<String, String> {
        self.inner
            .cloud_sync_export_device_link_uri()
            .map_err(|e| e.to_string())
    }

    pub fn cloud_sync_import_device_link_uri_json(&self, uri: &str) -> Result<String, String> {
        let res = self
            .inner
            .cloud_sync_import_device_link_uri(uri)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_enable_manual_passphrase_json(
        &self,
        passphrase: &str,
    ) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_enable_manual_passphrase(passphrase)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_set_manual_passphrase_mode_json(
        &self,
        require_manual: bool,
    ) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_set_manual_passphrase_mode(require_manual)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_logout_json(&self, clear_local_hosts: bool) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_logout(clear_local_hosts)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_get_preferences_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.cloud_sync_get_preferences()).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_update_preferences_json(&self, patch_json: &str) -> Result<(), String> {
        let patch: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(patch_json).map_err(|e| e.to_string())?;
        self.inner
            .cloud_sync_update_preferences(patch)
            .map_err(|e| e.to_string())
    }

    // --- Native Mobile Background Push (APNs / FCM / HarmonyOS Push Kit) ---

    pub fn register_push_device_json(
        &self,
        host_id: &str,
        req_json: &str,
    ) -> Result<String, String> {
        let req: RegisterPushDeviceRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let snap = self.inner.register_push_device_blocking(host_id, req)?;
        serde_json::to_string(&snap).map_err(|e| e.to_string())
    }

    pub fn unregister_push_device_json(
        &self,
        host_id: &str,
        device_id: &str,
    ) -> Result<String, String> {
        let snap = self
            .inner
            .unregister_push_device_blocking(host_id, device_id)?;
        serde_json::to_string(&snap).map_err(|e| e.to_string())
    }

    pub fn list_push_devices_json(&self, host_id: &str) -> Result<String, String> {
        let snap = self.inner.list_push_devices_blocking(host_id)?;
        serde_json::to_string(&snap).map_err(|e| e.to_string())
    }

    pub fn register_push_token_json(
        &self,
        platform: String,
        token: String,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.register_push_token(platform, token)?)
            .map_err(|e| e.to_string())
    }

    pub fn unregister_push_token(&self, platform: String) -> Result<(), String> {
        self.inner.unregister_push_token(platform)
    }

    pub fn push_registration_status_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.push_registration_status()).map_err(|e| e.to_string())
    }

    pub fn update_push_preferences_json(
        &self,
        host_id: String,
        req_json: String,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.run_mobile_rpc(
            self.inner.update_push_preferences_on_host(
                &host_id,
                serde_json::from_str(&req_json).map_err(|e| e.to_string())?,
            ),
        )?)
        .map_err(|e| e.to_string())
    }

    pub fn test_push_notification_json(
        &self,
        host_id: String,
        device_id: Option<String>,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self.inner.run_mobile_rpc(
                self.inner
                    .test_push_notification_on_host(&host_id, device_id),
            )?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn register_live_activity_token_json(
        &self,
        host_id: String,
        device_id: String,
        live_activity_token: Option<String>,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .run_mobile_rpc(self.inner.register_live_activity_token(
                    &host_id,
                    device_id,
                    live_activity_token,
                ))?,
        )
        .map_err(|e| e.to_string())
    }
}
