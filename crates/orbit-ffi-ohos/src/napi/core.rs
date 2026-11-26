use crate::OhosCoreFacade;
use crate::events::{DeliveryState, DeliveryStatus, OhosCoreNotification};
use napi_derive_ohos::napi;
use napi_ohos::bindgen_prelude::*;
use napi_ohos::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use std::sync::Mutex;

#[napi]
pub struct OrbitHarmonyCore {
    pub(super) facade: OhosCoreFacade,
}

// Both public class names refer to the same JS constructor and prototype.
#[napi(module_exports)]
fn export_mobile_core_alias(mut exports: Object) -> Result<()> {
    let constructor: Unknown = exports.get_named_property("OrbitHarmonyCore")?;
    exports.set_named_property("OhosOrbitCore", constructor)
}

#[napi]
impl OrbitHarmonyCore {
    #[napi(constructor)]
    pub fn new(db_path: String, auto_discover_local: Option<bool>) -> Result<Self> {
        let facade =
            OhosCoreFacade::open_with_options(&db_path, auto_discover_local.unwrap_or(false))
                .map_err(Error::from_reason)?;
        Ok(Self { facade })
    }

    #[napi(factory)]
    pub fn open_with_options(db_path: String, auto_discover_local: bool) -> Result<Self> {
        let facade = OhosCoreFacade::open_with_options(&db_path, auto_discover_local)
            .map_err(Error::from_reason)?;
        Ok(Self { facade })
    }

    #[napi]
    pub fn subscribe_events(&self, callback: ThreadsafeFunction<String, ()>) -> Result<()> {
        let resync = serde_json::to_string(&OhosCoreNotification::ResyncRequired)
            .map_err(|err| Error::from_reason(err.to_string()))?;
        let delivery = Mutex::new(DeliveryState::default());
        self.facade.subscribe_coalesced(24, move |payload_json| {
            let mut delivery = delivery.lock().unwrap_or_else(|err| err.into_inner());
            delivery.deliver(payload_json, &resync, |payload| {
                match callback.call(Ok(payload), ThreadsafeFunctionCallMode::NonBlocking) {
                    Status::Ok => DeliveryStatus::Ok,
                    _ => DeliveryStatus::Dropped,
                }
            });
        });
        Ok(())
    }

    #[napi]
    pub fn on_app_foreground(&self) {
        self.facade.on_app_foreground();
    }

    #[napi]
    pub fn on_app_background(&self) {
        self.facade.on_app_background();
    }

    #[napi]
    pub fn on_network_changed(&self) {
        self.facade.on_network_changed();
    }

    #[napi(factory)]
    pub fn new_with_master_key(db_path: String, master_key_b64: String) -> Result<Self> {
        Ok(Self {
            facade: OhosCoreFacade::new_with_master_key(db_path, master_key_b64)
                .map_err(Error::from_reason)?,
        })
    }

    #[napi(factory)]
    pub fn open(db_path: String) -> Result<Self> {
        Ok(Self {
            facade: OhosCoreFacade::open(&db_path).map_err(Error::from_reason)?,
        })
    }

    #[napi(factory)]
    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            facade: OhosCoreFacade::open_in_memory().map_err(Error::from_reason)?,
        })
    }

    #[napi]
    pub fn subscribe(&self, callback: ThreadsafeFunction<String, ()>) {
        self.facade.subscribe(move |event| {
            let _ = callback.call(Ok(event), ThreadsafeFunctionCallMode::NonBlocking);
        });
    }

    #[napi]
    pub fn subscribe_coalesced(
        &self,
        min_interval_ms: u32,
        callback: ThreadsafeFunction<String, ()>,
    ) {
        self.facade
            .subscribe_coalesced(min_interval_ms.into(), move |event| {
                let _ = callback.call(Ok(event), ThreadsafeFunctionCallMode::NonBlocking);
            });
    }
}
