use orbit_client_core::app_core::{CoreEventSubscriber, CoreNotification};
use serde::Serialize;
use std::sync::Arc;

use crate::OhosCoreFacade;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(not(any(target_env = "ohos", test)), allow(clippy::enum_variant_names))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum OhosCoreNotification {
    #[cfg(any(target_env = "ohos", test))]
    ResyncRequired,
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

/// One instance per subscription. A coalescer can invoke the callback from
/// different threads, so the caller serializes the marker and payload enqueue.
#[cfg(any(target_env = "ohos", test))]
#[derive(Default)]
pub(crate) struct DeliveryState {
    pending_resync: bool,
}

#[cfg(any(target_env = "ohos", test))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeliveryStatus {
    Ok,
    Dropped,
}

#[cfg(any(target_env = "ohos", test))]
impl DeliveryState {
    fn note_delivery(&mut self, status: DeliveryStatus) {
        self.pending_resync |= status == DeliveryStatus::Dropped;
    }

    fn take_pending_resync(&mut self) -> bool {
        std::mem::take(&mut self.pending_resync)
    }

    pub(crate) fn deliver(
        &mut self,
        payload: String,
        resync_json: &str,
        mut enqueue: impl FnMut(String) -> DeliveryStatus,
    ) {
        if self.take_pending_resync() && enqueue(resync_json.to_owned()) == DeliveryStatus::Dropped
        {
            self.note_delivery(DeliveryStatus::Dropped);
            return;
        }
        self.note_delivery(enqueue(payload));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resync_emitted_after_queue_failure() {
        let marker = serde_json::to_string(&OhosCoreNotification::ResyncRequired).expect("marker");
        assert_eq!(marker, r#"{"kind":"resync_required"}"#);
        let mut state = DeliveryState::default();
        let mut received = Vec::new();
        for (event, statuses) in [
            ("first", vec![DeliveryStatus::Ok]),
            ("lost", vec![DeliveryStatus::Dropped]),
            ("still full", vec![DeliveryStatus::Dropped]),
            ("next", vec![DeliveryStatus::Ok, DeliveryStatus::Ok]),
            ("ordinary", vec![DeliveryStatus::Ok]),
            ("lost again", vec![DeliveryStatus::Dropped]),
            ("last", vec![DeliveryStatus::Ok, DeliveryStatus::Ok]),
        ] {
            let mut results = statuses.into_iter();
            state.deliver(event.into(), &marker, |payload| {
                let status = results.next().expect("unexpected enqueue");
                if status == DeliveryStatus::Ok {
                    received.push(payload);
                }
                status
            });
            assert!(results.next().is_none());
        }
        assert_eq!(
            received,
            ["first", &marker, "next", "ordinary", &marker, "last"]
        );
    }
}
