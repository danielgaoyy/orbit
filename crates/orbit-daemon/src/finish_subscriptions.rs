//! Durable Finish & Permission-Checkpoint Subscriptions (`FinishSubscriptionManager`).
//!
//! Persists every `(caller_agent_id, child_agent_id, target_host_id)` watch in SQLite BEFORE
//! acknowledging `create_agent` / `send_agent_prompt`. Survives daemon restarts, emits actionable
//! permission checkpoints without ending the watch, retries notification delivery with exponential
//! backoff until acknowledged, and supports cross-host observation over `ConnectedHostsManager`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageKind, AgentStatus, AgentSummary, PermissionCheckpointRef,
    TimelinePayload, WireMessage,
};
use orbit_store::{OrbitStore, StoredFinishSubscription};
use tokio::sync::{Mutex, broadcast};
use tokio::task::AbortHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::connected_hosts::{ConnectedHostsManager, RemoteHostEvent};
use crate::manager::{AgentOrchestrator, DaemonBroadcast};
use crate::messaging::{AgentMessageDelivery, AgentPromptSource, QueuedAgentMessage};

const FINISH_NOTIFICATION_MSG_LIMIT: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WatchKey {
    caller_agent_id: String,
    child_agent_id: String,
    target_host_id: Option<String>,
}

pub struct FinishSubscriptionManager {
    self_host_id: String,
    store: Arc<OrbitStore>,
    orchestrator: Arc<AgentOrchestrator>,
    delivery: Arc<AgentMessageDelivery>,
    connected_hosts: Arc<ConnectedHostsManager>,
    active_watches: Mutex<HashMap<WatchKey, AbortHandle>>,
}

impl FinishSubscriptionManager {
    pub fn new(
        self_host_id: String,
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        delivery: Arc<AgentMessageDelivery>,
        connected_hosts: Arc<ConnectedHostsManager>,
    ) -> Arc<Self> {
        Arc::new(Self {
            self_host_id,
            store,
            orchestrator,
            delivery,
            connected_hosts,
            active_watches: Mutex::new(HashMap::new()),
        })
    }

    /// Restore all persisted finish subscriptions from SQLite after daemon startup & shim recovery.
    pub async fn start_recovery(self: &Arc<Self>) {
        let Ok(rows) = self.store.list_finish_subscriptions() else {
            return;
        };
        // Coalesce duplicate persisted rows for the same (caller, child, host)
        let mut grouped: HashMap<WatchKey, Vec<StoredFinishSubscription>> = HashMap::new();
        for row in rows {
            let key = WatchKey {
                caller_agent_id: row.caller_agent_id.clone(),
                child_agent_id: row.child_agent_id.clone(),
                target_host_id: row.target_host_id.clone(),
            };
            grouped.entry(key).or_default().push(row);
        }
        for (_, mut group) in grouped {
            if let Some(primary) = group.pop() {
                for extra in group {
                    let _ = self.store.delete_finish_subscription(&extra.id);
                }
                self.spawn_watch(primary, true).await;
            }
        }
    }

    /// Commit a durable finish subscription to SQLite and begin observing immediately.
    pub async fn register(
        self: &Arc<Self>,
        caller_agent_id: String,
        child_agent_id: String,
        target_host_id: Option<String>,
        require_parent_owner: bool,
    ) -> anyhow::Result<StoredFinishSubscription> {
        let normalized_host = target_host_id.filter(|h| h != &self.self_host_id);
        let sub = StoredFinishSubscription {
            id: format!("fs_{}", Uuid::new_v4().simple()),
            caller_agent_id,
            child_agent_id,
            target_host_id: normalized_host,
            require_parent_owner,
            delivered_perm_ids: Vec::new(),
            created_at: Utc::now(),
        };
        self.store.upsert_finish_subscription(&sub)?;
        self.spawn_watch(sub.clone(), false).await;
        Ok(sub)
    }

    pub fn has_pending_for_caller(&self, caller_agent_id: &str) -> bool {
        self.store
            .has_finish_subscriptions_for_caller(caller_agent_id)
            .unwrap_or(false)
    }

    /// Cancel any active watches owned by a caller that was archived or deleted.
    pub async fn cancel_for_caller(&self, caller_agent_id: &str) {
        let _ = self
            .store
            .delete_finish_subscriptions_for_caller(caller_agent_id);
        let mut guard = self.active_watches.lock().await;
        guard.retain(|k, abort| {
            if k.caller_agent_id == caller_agent_id {
                abort.abort();
                false
            } else {
                true
            }
        });
    }

    async fn spawn_watch(self: &Arc<Self>, sub: StoredFinishSubscription, is_recovery: bool) {
        let key = WatchKey {
            caller_agent_id: sub.caller_agent_id.clone(),
            child_agent_id: sub.child_agent_id.clone(),
            target_host_id: sub.target_host_id.clone(),
        };
        let this = Arc::clone(self);
        let key_bg = key.clone();
        let handle = tokio::spawn(async move {
            if sub.target_host_id.is_some() {
                this.run_remote_watch(sub, is_recovery).await;
            } else {
                this.run_local_watch(sub, is_recovery).await;
            }
            this.active_watches.lock().await.remove(&key_bg);
        });
        let mut guard = self.active_watches.lock().await;
        if let Some(prev) = guard.insert(key, handle.abort_handle()) {
            prev.abort();
        }
    }

    fn has_completed_latest_turn(&self, agent_id: &str) -> bool {
        let Ok((events, _)) = self.store.fetch_timeline_since(agent_id, 0, 500) else {
            return false;
        };
        timeline_has_completed_latest_turn(&events)
    }

    async fn caller_should_receive(
        &self,
        sub: &StoredFinishSubscription,
        child: Option<&AgentSummary>,
    ) -> bool {
        let Ok(Some(caller)) = self.store.get_agent(&sub.caller_agent_id) else {
            return false;
        };
        if caller.archived_at.is_some() {
            return false;
        }
        if sub.require_parent_owner
            && let Some(c) = child
        {
            if c.parent_agent_id.as_deref() != Some(sub.caller_agent_id.as_str()) {
                return false;
            }
            if let Some(ref ph) = c.parent_host_id
                && ph != &self.self_host_id
            {
                return false;
            }
        }
        true
    }

    async fn run_local_watch(&self, mut sub: StoredFinishSubscription, is_recovery: bool) {
        let mut rx = self.orchestrator.broadcast_tx.subscribe();
        let mut saw_running_or_perm = false;

        // Inspect immediate persisted snapshot first (handles startup recovery or ultra-fast child completion).
        // NOTE: Never treat `Initializing` -> init-`Idle` (before `SendPrompt` begins) as finished!
        if let Ok(Some(cur)) = self.store.get_agent(&sub.child_agent_id) {
            if !self.caller_should_receive(&sub, Some(&cur)).await {
                let _ = self.store.delete_finish_subscription(&sub.id);
                return;
            }
            match &cur.status {
                AgentStatus::Initializing => {}
                AgentStatus::Running { .. } | AgentStatus::AutonomousWakeup { .. } => {
                    saw_running_or_perm = true;
                }
                AgentStatus::AwaitingPermission { request_id, .. } => {
                    saw_running_or_perm = true;
                    self.maybe_deliver_local_permission_checkpoint(&mut sub, &cur, request_id)
                        .await;
                }
                AgentStatus::Idle | AgentStatus::Error { .. } | AgentStatus::Closed => {
                    // Only settle immediately on snapshot if:
                    // - child was closed/archived/errored, OR
                    // - this is a brand-new child (`require_parent_owner`) or startup recovery AND its
                    //   latest prompt already has a completed assistant reply (never an unanswered prompt).
                    let already_finished = cur.archived_at.is_some()
                        || matches!(cur.status, AgentStatus::Error { .. } | AgentStatus::Closed)
                        || ((is_recovery || sub.require_parent_owner)
                            && self.has_completed_latest_turn(&cur.agent_id));
                    if already_finished {
                        self.deliver_terminal_and_delete(&sub, &cur, None).await;
                        return;
                    }
                }
            }
        }

        // Wait on live daemon broadcast stream
        loop {
            match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
                Ok(Ok(DaemonBroadcast::AgentUpdated(ag))) if ag.agent_id == sub.child_agent_id => {
                    if !self.caller_should_receive(&sub, Some(&ag)).await {
                        let _ = self.store.delete_finish_subscription(&sub.id);
                        return;
                    }
                    if ag.archived_at.is_some() {
                        self.deliver_terminal_and_delete(&sub, &ag, Some("was closed"))
                            .await;
                        return;
                    }
                    match &ag.status {
                        AgentStatus::Initializing => {}
                        AgentStatus::Running { .. } | AgentStatus::AutonomousWakeup { .. } => {
                            saw_running_or_perm = true;
                        }
                        AgentStatus::AwaitingPermission { request_id, .. } => {
                            saw_running_or_perm = true;
                            self.maybe_deliver_local_permission_checkpoint(
                                &mut sub, &ag, request_id,
                            )
                            .await;
                        }
                        AgentStatus::Idle => {
                            if saw_running_or_perm
                                || ((is_recovery || sub.require_parent_owner)
                                    && self.has_completed_latest_turn(&ag.agent_id))
                            {
                                self.deliver_terminal_and_delete(&sub, &ag, Some("finished"))
                                    .await;
                                return;
                            }
                        }
                        AgentStatus::Error { .. } => {
                            self.deliver_terminal_and_delete(&sub, &ag, Some("errored"))
                                .await;
                            return;
                        }
                        AgentStatus::Closed => {
                            self.deliver_terminal_and_delete(&sub, &ag, Some("was closed"))
                                .await;
                            return;
                        }
                    }
                }
                Ok(Ok(DaemonBroadcast::AgentDeleted(deleted_id)))
                    if deleted_id == sub.child_agent_id =>
                {
                    self.deliver_unavailable_and_delete(&sub, "is unavailable", None)
                        .await;
                    return;
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) | Err(_) => {
                    // Periodic resync from SQLite in case a broadcast lagged
                    match self.store.get_agent(&sub.child_agent_id) {
                        Ok(Some(ag)) => {
                            if !self.caller_should_receive(&sub, Some(&ag)).await {
                                let _ = self.store.delete_finish_subscription(&sub.id);
                                return;
                            }
                            if ag.archived_at.is_some() {
                                self.deliver_terminal_and_delete(&sub, &ag, Some("was closed"))
                                    .await;
                                return;
                            }
                            match &ag.status {
                                AgentStatus::Idle
                                    if saw_running_or_perm
                                        || self.has_completed_latest_turn(&ag.agent_id) =>
                                {
                                    self.deliver_terminal_and_delete(&sub, &ag, Some("finished"))
                                        .await;
                                    return;
                                }
                                AgentStatus::Error { .. } => {
                                    self.deliver_terminal_and_delete(&sub, &ag, Some("errored"))
                                        .await;
                                    return;
                                }
                                AgentStatus::Closed => {
                                    self.deliver_terminal_and_delete(&sub, &ag, Some("was closed"))
                                        .await;
                                    return;
                                }
                                _ => {}
                            }
                        }
                        Ok(None) => {
                            self.deliver_unavailable_and_delete(&sub, "is unavailable", None)
                                .await;
                            return;
                        }
                        Err(_) => {}
                    }
                }
                Ok(Ok(_)) => {}
                Ok(Err(broadcast::error::RecvError::Closed)) => return,
            }
        }
    }

    async fn run_remote_watch(&self, mut sub: StoredFinishSubscription, is_recovery: bool) {
        let Some(ref target_host_id) = sub.target_host_id.clone() else {
            return;
        };
        let mut remote_rx = self.connected_hosts.remote_events_tx.subscribe();
        let mut consecutive_failures: u32 = 0;
        let mut saw_running = false;

        loop {
            if !self.caller_should_receive(&sub, None).await {
                let _ = self.store.delete_finish_subscription(&sub.id);
                return;
            }
            if !self
                .connected_hosts
                .is_host_registered(target_host_id)
                .await
            {
                self.deliver_unavailable_and_delete(
                    &sub,
                    "host removed",
                    Some(target_host_id.as_str()),
                )
                .await;
                return;
            }

            // Probe remote agent status over Fleet Mesh
            let req_id = Uuid::new_v4();
            let probe_res = self
                .connected_hosts
                .send_rpc(
                    target_host_id,
                    req_id,
                    WireMessage::ListAgents {
                        request_id: req_id,
                        workspace_id: None,
                        include_archived: true,
                        include_background: true,
                        parent_agent_id: None,
                    },
                    Duration::from_secs(10),
                )
                .await;

            match probe_res {
                Ok(WireMessage::AgentsSnapshot { agents, .. }) => {
                    consecutive_failures = 0;
                    if let Some(child) = agents
                        .into_iter()
                        .find(|a| a.agent_id == sub.child_agent_id)
                    {
                        if !self.caller_should_receive(&sub, Some(&child)).await {
                            let _ = self.store.delete_finish_subscription(&sub.id);
                            return;
                        }
                        if child.archived_at.is_some() {
                            self.deliver_remote_terminal(
                                &sub,
                                &child,
                                target_host_id,
                                "was closed",
                            )
                            .await;
                            return;
                        }
                        match &child.status {
                            AgentStatus::Initializing => {}
                            AgentStatus::Running { .. } | AgentStatus::AutonomousWakeup { .. } => {
                                saw_running = true;
                            }
                            AgentStatus::AwaitingPermission { request_id, .. } => {
                                saw_running = true;
                                self.maybe_deliver_remote_permission_checkpoint(
                                    &mut sub,
                                    &child,
                                    target_host_id,
                                    request_id,
                                )
                                .await;
                            }
                            AgentStatus::Idle => {
                                let settled = if saw_running {
                                    true
                                } else if is_recovery || sub.require_parent_owner {
                                    let tl_req = Uuid::new_v4();
                                    match self
                                        .connected_hosts
                                        .send_rpc(
                                            target_host_id,
                                            tl_req,
                                            WireMessage::SubscribeTimeline {
                                                request_id: tl_req,
                                                agent_id: child.agent_id.clone(),
                                                since_seq: 0,
                                                tail_limit: None,
                                            },
                                            Duration::from_secs(10),
                                        )
                                        .await
                                    {
                                        Ok(WireMessage::TimelineCatchup { events, .. }) => {
                                            timeline_has_completed_latest_turn(&events)
                                        }
                                        _ => false,
                                    }
                                } else {
                                    false
                                };
                                if settled {
                                    self.deliver_remote_terminal(
                                        &sub,
                                        &child,
                                        target_host_id,
                                        "finished",
                                    )
                                    .await;
                                    return;
                                }
                            }
                            AgentStatus::Error { .. } => {
                                self.deliver_remote_terminal(
                                    &sub,
                                    &child,
                                    target_host_id,
                                    "errored",
                                )
                                .await;
                                return;
                            }
                            AgentStatus::Closed => {
                                self.deliver_remote_terminal(
                                    &sub,
                                    &child,
                                    target_host_id,
                                    "was closed",
                                )
                                .await;
                                return;
                            }
                        }
                    } else {
                        self.deliver_unavailable_and_delete(
                            &sub,
                            "is unavailable",
                            Some(target_host_id.as_str()),
                        )
                        .await;
                        return;
                    }
                }
                _ => {
                    consecutive_failures += 1;
                    if consecutive_failures >= 3 {
                        self.deliver_unavailable_and_delete(
                            &sub,
                            "host unreachable",
                            Some(target_host_id.as_str()),
                        )
                        .await;
                        return;
                    }
                    let delay = Duration::from_secs((1u64 << consecutive_failures).min(5));
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }

            // Wait for live push event from remote host (or re-poll every 3s)
            match tokio::time::timeout(Duration::from_secs(3), remote_rx.recv()).await {
                Ok(Ok(RemoteHostEvent::AgentUpdated { host_id, agent }))
                    if host_id == *target_host_id && agent.agent_id == sub.child_agent_id =>
                {
                    if matches!(
                        agent.status,
                        AgentStatus::Running { .. }
                            | AgentStatus::AutonomousWakeup { .. }
                            | AgentStatus::AwaitingPermission { .. }
                    ) {
                        saw_running = true;
                    }
                    // Loop around to process cleanly
                }
                Ok(Ok(RemoteHostEvent::HostRemoved { host_id })) if host_id == *target_host_id => {
                    self.deliver_unavailable_and_delete(
                        &sub,
                        "host removed",
                        Some(target_host_id.as_str()),
                    )
                    .await;
                    return;
                }
                _ => {}
            }
        }
    }

    async fn maybe_deliver_local_permission_checkpoint(
        &self,
        sub: &mut StoredFinishSubscription,
        child: &AgentSummary,
        request_id: &str,
    ) {
        if sub.delivered_perm_ids.iter().any(|id| id == request_id) {
            return;
        }
        // Wait briefly in case `StatusChanged(AwaitingPermission)` arrived just ahead of
        // `PermissionRequest` ingestion on another task.
        let mut perm = None;
        for attempt in 0..8 {
            if let Some(found) = self
                .orchestrator
                .get_pending_permission(&child.agent_id, request_id)
                .await
            {
                perm = Some(found);
                break;
            }
            if attempt < 7 {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
        let Some(perm) = perm else {
            return;
        };
        let perm_ref = PermissionCheckpointRef {
            agent_id: child.agent_id.clone(),
            host_id: None,
            request_id: perm.request_id.clone(),
            title: perm.title.clone(),
            detail: perm.detail.clone(),
            allowed_decisions: perm.allowed_decisions.clone(),
        };
        let body = format_permission_notification_body(child, None, &perm_ref);
        let msg = QueuedAgentMessage {
            recipient_id: sub.caller_agent_id.clone(),
            message_id: format!("perm_chk_{}", Uuid::new_v4().simple()),
            source: AgentPromptSource {
                kind: AgentMessageKind::FinishNotification,
                source_agent_id: Some(child.agent_id.clone()),
                source_host_id: Some(self.self_host_id.clone()),
                source_title: Some(child.title.clone()),
                event: Some("permission_required".into()),
                permission_context: Some(perm_ref),
            },
            body,
            active_turn_behavior: ActiveTurnBehavior::Queue,
            session_mode: None,
            unarchive: false,
        };
        if self.enqueue_with_retry(msg).await {
            sub.delivered_perm_ids.push(request_id.to_string());
            let _ = self
                .store
                .mark_finish_subscription_perm_delivered(&sub.id, request_id);
        }
    }

    async fn maybe_deliver_remote_permission_checkpoint(
        &self,
        sub: &mut StoredFinishSubscription,
        child: &AgentSummary,
        target_host_id: &str,
        request_id: &str,
    ) {
        if sub.delivered_perm_ids.iter().any(|id| id == request_id) {
            return;
        }
        let req_id = Uuid::new_v4();
        let perm_resp = self
            .connected_hosts
            .send_rpc(
                target_host_id,
                req_id,
                WireMessage::ListPendingPermissions {
                    request_id: req_id,
                    agent_id: Some(child.agent_id.clone()),
                },
                Duration::from_secs(10),
            )
            .await;
        let Ok(WireMessage::PendingPermissionsSnapshot { permissions, .. }) = perm_resp else {
            return;
        };
        let Some(perm) = permissions.into_iter().find(|p| p.request_id == request_id) else {
            return;
        };
        let perm_ref = PermissionCheckpointRef {
            agent_id: child.agent_id.clone(),
            host_id: Some(target_host_id.to_string()),
            request_id: perm.request_id.clone(),
            title: perm.title.clone(),
            detail: perm.detail.clone(),
            allowed_decisions: perm.allowed_decisions.clone(),
        };
        let body = format_permission_notification_body(child, Some(target_host_id), &perm_ref);
        let msg = QueuedAgentMessage {
            recipient_id: sub.caller_agent_id.clone(),
            message_id: format!("rperm_chk_{}", Uuid::new_v4().simple()),
            source: AgentPromptSource {
                kind: AgentMessageKind::FinishNotification,
                source_agent_id: Some(child.agent_id.clone()),
                source_host_id: Some(target_host_id.to_string()),
                source_title: Some(child.title.clone()),
                event: Some("permission_required".into()),
                permission_context: Some(perm_ref),
            },
            body,
            active_turn_behavior: ActiveTurnBehavior::Queue,
            session_mode: None,
            unarchive: false,
        };
        if self.enqueue_with_retry(msg).await {
            sub.delivered_perm_ids.push(request_id.to_string());
            let _ = self
                .store
                .mark_finish_subscription_perm_delivered(&sub.id, request_id);
        }
    }

    async fn deliver_terminal_and_delete(
        &self,
        sub: &StoredFinishSubscription,
        child: &AgentSummary,
        reason_override: Option<&str>,
    ) {
        let reason = reason_override.unwrap_or(match child.status {
            AgentStatus::Error { .. } => "errored",
            AgentStatus::Closed => "was closed",
            _ => "finished",
        });
        let event_tag = match reason {
            "finished" => "finished",
            "errored" => "errored",
            _ => "closed",
        };
        let last_msg = self
            .store
            .last_assistant_message_text(&child.agent_id)
            .ok()
            .flatten();
        let body = format_terminal_notification_body(child, None, reason, last_msg.as_deref());
        let msg = QueuedAgentMessage {
            recipient_id: sub.caller_agent_id.clone(),
            message_id: format!("fin_{}", Uuid::new_v4().simple()),
            source: AgentPromptSource {
                kind: AgentMessageKind::FinishNotification,
                source_agent_id: Some(child.agent_id.clone()),
                source_host_id: Some(self.self_host_id.clone()),
                source_title: Some(child.title.clone()),
                event: Some(event_tag.into()),
                permission_context: None,
            },
            body,
            active_turn_behavior: ActiveTurnBehavior::Queue,
            session_mode: None,
            unarchive: false,
        };
        if self.enqueue_with_retry(msg).await {
            let _ = self.store.delete_finish_subscription(&sub.id);
            info!(
                caller = %sub.caller_agent_id,
                child = %sub.child_agent_id,
                reason = %reason,
                "Delivered durable finish notification to parent agent"
            );
        }
    }

    async fn deliver_remote_terminal(
        &self,
        sub: &StoredFinishSubscription,
        child: &AgentSummary,
        target_host_id: &str,
        reason: &str,
    ) {
        // Fetch remote child's last assistant message cleanly over Fleet Mesh
        let req_id = Uuid::new_v4();
        let last_msg = match self
            .connected_hosts
            .send_rpc(
                target_host_id,
                req_id,
                WireMessage::SubscribeTimeline {
                    request_id: req_id,
                    agent_id: child.agent_id.clone(),
                    since_seq: 0,
                    tail_limit: None,
                },
                Duration::from_secs(10),
            )
            .await
        {
            Ok(WireMessage::TimelineCatchup { events, .. }) => {
                events.into_iter().rev().find_map(|ev| match ev.payload {
                    orbit_protocol::TimelinePayload::AssistantMessage { text, .. } => Some(text),
                    _ => None,
                })
            }
            _ => None,
        };
        let event_tag = match reason {
            "finished" => "finished",
            "errored" => "errored",
            _ => "closed",
        };
        let body = format_terminal_notification_body(
            child,
            Some(target_host_id),
            reason,
            last_msg.as_deref(),
        );
        let msg = QueuedAgentMessage {
            recipient_id: sub.caller_agent_id.clone(),
            message_id: format!("rfin_{}", Uuid::new_v4().simple()),
            source: AgentPromptSource {
                kind: AgentMessageKind::FinishNotification,
                source_agent_id: Some(child.agent_id.clone()),
                source_host_id: Some(target_host_id.to_string()),
                source_title: Some(child.title.clone()),
                event: Some(event_tag.into()),
                permission_context: None,
            },
            body,
            active_turn_behavior: ActiveTurnBehavior::Queue,
            session_mode: None,
            unarchive: false,
        };
        if self.enqueue_with_retry(msg).await {
            let _ = self.store.delete_finish_subscription(&sub.id);
        }
    }

    async fn deliver_unavailable_and_delete(
        &self,
        sub: &StoredFinishSubscription,
        reason: &str,
        target_host_id: Option<&str>,
    ) {
        let body = if reason == "host unreachable" {
            let host = target_host_id.unwrap_or("unknown");
            format!(
                "Could not observe child agent {} on host {host} after three failed attempts.\n\nStatus of child agent {} is unknown. This finish subscription has ended.",
                sub.child_agent_id, sub.child_agent_id
            )
        } else if reason == "host removed" {
            let host = target_host_id.unwrap_or("unknown");
            format!(
                "Host {host} was removed or disabled.\n\nStatus of child agent {} is unknown. This finish subscription has ended.",
                sub.child_agent_id
            )
        } else {
            format!("Agent {} {reason}.", sub.child_agent_id)
        };
        let msg = QueuedAgentMessage {
            recipient_id: sub.caller_agent_id.clone(),
            message_id: format!("ufin_{}", Uuid::new_v4().simple()),
            source: AgentPromptSource {
                kind: AgentMessageKind::FinishNotification,
                source_agent_id: Some(sub.child_agent_id.clone()),
                source_host_id: target_host_id.map(ToOwned::to_owned),
                source_title: None,
                event: Some("errored".into()),
                permission_context: None,
            },
            body,
            active_turn_behavior: ActiveTurnBehavior::Queue,
            session_mode: None,
            unarchive: false,
        };
        let _ = self.enqueue_with_retry(msg).await;
        let _ = self.store.delete_finish_subscription(&sub.id);
    }

    async fn enqueue_with_retry(&self, msg: QueuedAgentMessage) -> bool {
        for attempt in 0..4u32 {
            if let Ok((_, ack_rx)) = self.delivery.enqueue(msg.clone()).await
                && matches!(ack_rx.await, Ok(Ok(())))
            {
                return true;
            }
            if !self
                .store
                .get_agent(&msg.recipient_id)
                .ok()
                .flatten()
                .is_some_and(|a| a.archived_at.is_none())
            {
                return false;
            }
            let delay = Duration::from_secs((1u64 << attempt).min(5));
            warn!(
                recipient = %msg.recipient_id,
                attempt = attempt + 1,
                "Finish notification dispatch failed; retrying"
            );
            tokio::time::sleep(delay).await;
        }
        false
    }
}

fn format_permission_notification_body(
    child: &AgentSummary,
    host_id: Option<&str>,
    perm: &PermissionCheckpointRef,
) -> String {
    let mut sections = vec![format!(
        "Agent {} ({}) needs permission.",
        child.agent_id, child.title
    )];
    if let Some(h) = host_id {
        sections.push(format!("Host: {h}"));
    }
    sections.push(
        "Respond with `respond_to_permission` using the `agent_id` and `request_id` below."
            .to_string(),
    );
    let payload = serde_json::to_string_pretty(perm).unwrap_or_default();
    sections.push(format!(
        "<permission-request>\n{payload}\n</permission-request>"
    ));
    sections.join("\n\n")
}

fn format_terminal_notification_body(
    child: &AgentSummary,
    host_id: Option<&str>,
    reason: &str,
    last_assistant: Option<&str>,
) -> String {
    let mut sections = vec![format!(
        "Agent {} ({}) {reason}.",
        child.agent_id, child.title
    )];
    if let Some(h) = host_id {
        sections.push(format!("Host: {h}"));
    }
    if let AgentStatus::Error { ref message, .. } = child.status {
        sections.push(format!("Error: {message}"));
    }
    if let Some(raw) = last_assistant.map(str::trim).filter(|s| !s.is_empty()) {
        let total = raw.chars().count();
        let clipped = if total > FINISH_NOTIFICATION_MSG_LIMIT {
            let head: String = raw.chars().take(FINISH_NOTIFICATION_MSG_LIMIT).collect();
            let omitted = total - FINISH_NOTIFICATION_MSG_LIMIT;
            format!(
                "{head}\n[truncated {omitted} chars; use get_agent_activity for the full response]"
            )
        } else {
            raw.to_string()
        };
        sections.push(format!("<agent-response>\n{clipped}\n</agent-response>"));
    }
    sections.join("\n\n")
}

fn timeline_has_completed_latest_turn(events: &[orbit_protocol::TimelineEvent]) -> bool {
    let last_prompt_seq = events.iter().rev().find_map(|e| match &e.payload {
        TimelinePayload::UserPrompt { .. } | TimelinePayload::AgentMessage { .. } => Some(e.seq),
        _ => None,
    });
    let last_reply_seq = events.iter().rev().find_map(|e| match &e.payload {
        TimelinePayload::AssistantMessage {
            streaming: false, ..
        } => Some(e.seq),
        _ => None,
    });
    match (last_prompt_seq, last_reply_seq) {
        (Some(p), Some(r)) => r > p,
        (None, Some(_)) => true,
        _ => false,
    }
}
