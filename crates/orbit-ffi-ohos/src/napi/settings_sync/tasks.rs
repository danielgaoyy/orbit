use crate::OhosCoreFacade;
use napi_ohos::bindgen_prelude::*;

pub struct RegisterPushDeviceTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) req_json: String,
}

impl Task for RegisterPushDeviceTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .register_push_device_json(&self.host_id, &self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct UnregisterPushDeviceTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) device_id: String,
}

impl Task for UnregisterPushDeviceTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .unregister_push_device_json(&self.host_id, &self.device_id)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ListPushDevicesTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
}

impl Task for ListPushDevicesTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .list_push_devices_json(&self.host_id)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CloudSyncStartGithubDeviceFlowTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) client_id_override: Option<String>,
}

impl Task for CloudSyncStartGithubDeviceFlowTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cloud_sync_start_github_device_flow_json(self.client_id_override.clone())
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CloudSyncPollGithubDeviceFlowTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) device_code: String,
    pub(super) client_id_override: Option<String>,
}

impl Task for CloudSyncPollGithubDeviceFlowTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cloud_sync_poll_github_device_flow_json(
                &self.device_code,
                self.client_id_override.clone(),
            )
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CloudSyncLoginWithTokenTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) token: String,
}

impl Task for CloudSyncLoginWithTokenTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cloud_sync_login_with_token_json(&self.token)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CloudSyncUnlockWithPassphraseTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) passphrase: String,
}

impl Task for CloudSyncUnlockWithPassphraseTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cloud_sync_unlock_with_passphrase_json(&self.passphrase)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CloudSyncNowTask {
    pub(super) facade: OhosCoreFacade,
}

impl Task for CloudSyncNowTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cloud_sync_now_json()
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CloudSyncImportDeviceLinkUriTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) uri: String,
}

impl Task for CloudSyncImportDeviceLinkUriTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cloud_sync_import_device_link_uri_json(&self.uri)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CloudSyncEnableManualPassphraseTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) passphrase: String,
}

impl Task for CloudSyncEnableManualPassphraseTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cloud_sync_enable_manual_passphrase_json(&self.passphrase)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CloudSyncSetManualPassphraseModeTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) require_manual: bool,
}

impl Task for CloudSyncSetManualPassphraseModeTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .cloud_sync_set_manual_passphrase_mode_json(self.require_manual)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}
