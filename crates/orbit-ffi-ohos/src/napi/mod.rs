pub mod composer;
pub mod core;
pub mod fleet_stage;
pub mod git_files_terminal;
pub mod pr5_tools;
pub mod settings_sync;
pub mod state;
pub mod workspace_auto;

use crate::OhosCoreFacade;
use napi_ohos::bindgen_prelude::*;

type RpcJob<T> = Box<dyn FnOnce() -> std::result::Result<T, String> + Send>;
pub struct MobileRpcTask<T>(Option<RpcJob<T>>);
fn run_mobile_job<T: Send + TypeName + ToNapiValue + 'static>(
    facade: &OhosCoreFacade,
    job: impl FnOnce(OhosCoreFacade) -> std::result::Result<T, String> + Send + 'static,
) -> AsyncTask<MobileRpcTask<T>> {
    let facade = facade.clone();
    AsyncTask::new(MobileRpcTask(Some(Box::new(move || job(facade)))))
}

impl<T: Send + TypeName + ToNapiValue + 'static> Task for MobileRpcTask<T> {
    type Output = T;
    type JsValue = T;
    fn compute(&mut self) -> Result<T> {
        let job = self
            .0
            .take()
            .ok_or_else(|| Error::from_reason("mobile RPC already completed"))?;
        job().map_err(Error::from_reason)
    }
    fn resolve(&mut self, _env: Env, output: T) -> Result<T> {
        Ok(output)
    }
}
