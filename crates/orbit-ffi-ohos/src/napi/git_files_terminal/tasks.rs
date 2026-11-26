use crate::OhosCoreFacade;
use napi_ohos::bindgen_prelude::*;

pub struct StageAttachmentTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) filename: String,
    pub(super) mime_type: Option<String>,
    pub(super) bytes: Vec<u8>,
}

impl Task for StageAttachmentTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        let bytes = std::mem::take(&mut self.bytes);
        self.facade
            .stage_attachment_json(
                &self.host_id,
                &self.filename,
                self.mime_type.as_deref(),
                bytes,
            )
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ReadWorkspaceFileTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) cwd: String,
    pub(super) path: String,
}

impl Task for ReadWorkspaceFileTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .read_workspace_file_json(&self.host_id, &self.cwd, &self.path)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct WriteWorkspaceFileTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for WriteWorkspaceFileTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .write_workspace_file_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct CreateWorkspaceEntryTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for CreateWorkspaceEntryTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .create_workspace_entry_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct RenameWorkspaceEntryTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for RenameWorkspaceEntryTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .rename_workspace_entry_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct DeleteWorkspaceEntryTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for DeleteWorkspaceEntryTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .delete_workspace_entry_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ListArtifactsTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) workspace_id: Option<String>,
    pub(super) agent_id: Option<String>,
}

impl Task for ListArtifactsTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .list_artifacts_json(
                &self.host_id,
                self.workspace_id.clone(),
                self.agent_id.clone(),
            )
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct GetArtifactTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) artifact_id: String,
    pub(super) version: Option<u32>,
}

impl Task for GetArtifactTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .get_artifact_json(&self.host_id, &self.artifact_id, self.version)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct UpsertArtifactTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for UpsertArtifactTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .upsert_artifact_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct DeleteArtifactTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) artifact_id: String,
}

impl Task for DeleteArtifactTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .delete_artifact(&self.host_id, &self.artifact_id)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}
