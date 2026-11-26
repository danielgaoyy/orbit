use crate::OhosCoreFacade;
use napi_ohos::bindgen_prelude::*;

pub struct RewindAgentTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) host_id: String,
    pub(super) agent_id: String,
    pub(super) to_before_seq: u64,
    pub(super) restore_files: bool,
}

impl Task for RewindAgentTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .rewind_agent_json(
                &self.host_id,
                &self.agent_id,
                self.to_before_seq,
                self.restore_files,
            )
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct ForkAgentTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for ForkAgentTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .fork_agent_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}

pub struct RenderDiagramTask {
    pub(super) facade: OhosCoreFacade,
    pub(super) req_json: String,
}

impl Task for RenderDiagramTask {
    type Output = String;
    type JsValue = String;

    fn compute(&mut self) -> Result<Self::Output> {
        self.facade
            .render_diagram_json(&self.req_json)
            .map_err(Error::from_reason)
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
        Ok(output)
    }
}
