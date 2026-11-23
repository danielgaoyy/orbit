//! Idle Runtime Cleanup (`IdleRuntimeCleanup`) — reclaims inactive resumable provider processes
//! while strictly protecting agents with active descendants, pending messages, finish subscriptions,
//! heartbeats, or pending permissions.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use orbit_protocol::{AgentStatus, AgentSummary, OrchestrationConfig};
use orbit_store::OrbitStore;
use tokio::sync::RwLock;
use tracing::info;

use crate::finish_subscriptions::FinishSubscriptionManager;
use crate::manager::AgentOrchestrator;
use crate::messaging::AgentMessageDelivery;

pub struct IdleRuntimeCleanup {
    store: Arc<OrbitStore>,
    orchestrator: Arc<AgentOrchestrator>,
    delivery: Arc<AgentMessageDelivery>,
    finish_subs: Arc<FinishSubscriptionManager>,
    config: Arc<RwLock<OrchestrationConfig>>,
}

impl IdleRuntimeCleanup {
    pub fn new(
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        delivery: Arc<AgentMessageDelivery>,
        finish_subs: Arc<FinishSubscriptionManager>,
        config: Arc<RwLock<OrchestrationConfig>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            orchestrator,
            delivery,
            finish_subs,
            config,
        })
    }

    pub fn spawn_background(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                let _ = this.sweep_once(Utc::now()).await;
            }
        })
    }

    /// Execute one deterministic cleanup sweep and return IDs of collected runtimes.
    pub async fn sweep_once(&self, now: DateTime<Utc>) -> Vec<String> {
        let cfg = self.config.read().await.clone();
        if !cfg.idle_cleanup_enabled || cfg.idle_timeout_secs == 0 {
            return Vec::new();
        }
        let live_ids = self.orchestrator.list_live_agent_ids().await;
        if live_ids.is_empty() {
            return Vec::new();
        }
        let all_agents = self
            .store
            .list_agents_filtered(None, false, true, None)
            .unwrap_or_default();
        let by_id: HashMap<String, AgentSummary> = all_agents
            .into_iter()
            .map(|a| (a.agent_id.clone(), a))
            .collect();

        let timeout = chrono::Duration::seconds(cfg.idle_timeout_secs as i64);
        let mut collected = Vec::new();

        for agent_id in live_ids {
            let Some(agent) = by_id.get(&agent_id) else {
                continue;
            };
            // Must have a durable provider persistence handle so next prompt resumes transparently
            if self
                .store
                .get_persistence_ref(&agent_id)
                .ok()
                .flatten()
                .is_none()
            {
                continue;
            }
            let last_act = agent.last_activity_at.unwrap_or(agent.updated_at);
            if now - last_act < timeout {
                continue;
            }
            if !self.is_family_eligible(&agent_id, &by_id).await {
                continue;
            }
            // Final atomic revalidation before releasing runtime
            if self.orchestrator.close_idle_runtime(&agent_id).await {
                info!(agent_id = %agent_id, "Collected idle resumable agent runtime");
                collected.push(agent_id);
            }
        }
        collected
    }

    async fn is_family_eligible(
        &self,
        root_id: &str,
        by_id: &HashMap<String, AgentSummary>,
    ) -> bool {
        // BFS over root and all managed descendants
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue = vec![root_id.to_string()];
        while let Some(cur_id) = queue.pop() {
            if !visited.insert(cur_id.clone()) {
                continue;
            }
            if let Some(ag) = by_id.get(&cur_id) {
                // Canonical Personal Router sessions stay warm and are never idle-reclaimed.
                if ag.is_personal_router() {
                    return false;
                }
                match ag.status {
                    AgentStatus::Idle | AgentStatus::Error { .. } | AgentStatus::Closed => {}
                    AgentStatus::Initializing
                    | AgentStatus::Running { .. }
                    | AgentStatus::AutonomousWakeup { .. }
                    | AgentStatus::AwaitingPermission { .. } => return false,
                }
            }
            if self.delivery.has_pending(&cur_id).await {
                return false;
            }
            if self.finish_subs.has_pending_for_caller(&cur_id) {
                return false;
            }
            if self
                .store
                .has_active_heartbeats_for_agent(&cur_id)
                .unwrap_or(false)
            {
                return false;
            }
            if self.orchestrator.has_pending_permissions(&cur_id).await {
                return false;
            }
            for child in by_id.values() {
                if child.parent_agent_id.as_deref() == Some(cur_id.as_str()) {
                    queue.push(child.agent_id.clone());
                }
            }
        }
        true
    }
}
