use orbit_client_core::app_core::{CoreEventSubscriber, CoreNotification};
use serde::Serialize;
use std::sync::Arc;

use crate::OhosCoreFacade;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum OhosCoreNotification {
    FleetChanged {
        revision: u64,
    },
    HostChanged {
        revision: u64,
        host_id: String,
    },
    TimelineChanged {
        revision: u64,
        host_id: String,
        agent_id: String,
    },
    GitChanged {
        revision: u64,
        host_id: String,
        cwd: String,
    },
    TerminalChanged {
        revision: u64,
        host_id: String,
        slot: u8,
    },
}

impl From<CoreNotification> for OhosCoreNotification {
    fn from(ev: CoreNotification) -> Self {
        match ev {
            CoreNotification::FleetChanged { revision } => Self::FleetChanged { revision },
            CoreNotification::HostChanged { revision, host_id } => {
                Self::HostChanged { revision, host_id }
            }
            CoreNotification::TimelineChanged {
                revision,
                host_id,
                agent_id,
            } => Self::TimelineChanged {
                revision,
                host_id,
                agent_id,
            },
            CoreNotification::GitChanged {
                revision,
                host_id,
                cwd,
            } => Self::GitChanged {
                revision,
                host_id,
                cwd,
            },
            CoreNotification::TerminalChanged {
                revision,
                host_id,
                slot,
                ..
            } => Self::TerminalChanged {
                revision,
                host_id,
                slot,
            },
        }
    }
}

impl OhosCoreFacade {
    pub fn subscribe(&self, callback: impl Fn(String) + Send + Sync + 'static) {
        self.subscribe_coalesced(0, callback);
    }

    pub fn subscribe_coalesced(
        &self,
        min_interval_ms: u64,
        callback: impl Fn(String) + Send + Sync + 'static,
    ) {
        struct FnSub<F>(F);
        impl<F: Fn(String) + Send + Sync> CoreEventSubscriber for FnSub<F> {
            fn on_notification(&self, event: CoreNotification) {
                let compact: OhosCoreNotification = event.into();
                if let Ok(json) = serde_json::to_string(&compact) {
                    (self.0)(json);
                }
            }
        }
        let subscriber: Arc<dyn CoreEventSubscriber> = Arc::new(FnSub(callback));
        #[cfg(feature = "perf-hooks")]
        if let Ok(mut replay) = self.replay.lock() {
            replay.subscribe(&subscriber);
        }
        self.inner.subscribe_coalesced(min_interval_ms, subscriber);
    }
}
