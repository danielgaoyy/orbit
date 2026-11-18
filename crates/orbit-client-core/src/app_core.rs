//! Headless Multi-Host MVU Application Core (`OrbitAppCore`) shared across:
//! - Desktop: Tauri v2 (macOS & Linux)
//! - iOS 16.4+: Swift + SwiftUI (via `orbit-ffi-uniffi`)
//! - Android (minSdk 29 / targetSdk 36): Kotlin + Jetpack Compose (via `orbit-ffi-uniffi`)
//! - HarmonyOS 7+ (API 26): ArkTS + ArkUI V2 (via `orbit-ffi-ohos`)

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use orbit_crypto::PairingOffer;
use orbit_protocol::{
    AgentStatus, AgentSummary, GitStatusSnapshot, ProviderKind, ProviderQuotaSnapshot,
    QuestionItem, SessionMode, TerminalBinaryFrame, TerminalSummary, TimelineEvent,
    TimelinePayload, ToolCallDetail, ToolCallStatus, WireMessage, WorkspaceSummary,
};
use orbit_store::{OrbitStore, StoredHostProfile};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::desktop_ops::{discover_local_daemon_pairing_uri, normalize_relay_input};
use crate::render::{
    DiffFileView, RichBlockView, TerminalGridEmulator, TerminalScreenView, parse_markdown_blocks,
    parse_unified_diff,
};
use crate::{
    ClientCoreError, ClientInboundEvent, HostConnectionState, HostSessionHandle, SavedHostProfile,
    connect_smart,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreNotification {
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
        raw_bytes: Vec<u8>,
    },
}

pub trait CoreEventSubscriber: Send + Sync {
    fn on_notification(&self, event: CoreNotification);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostCardView {
    pub host_id: String,
    pub display_name: String,
    pub daemon_pubkey_b64: String,
    pub direct_endpoints: Vec<String>,
    pub relay_url: Option<String>,
    pub connection_state: HostConnectionState,
    pub last_error: Option<String>,
    pub available_providers: Vec<ProviderKind>,
    pub workspace_count: u32,
    pub agent_count: u32,
    pub pending_approval_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetOverviewView {
    pub revision: u64,
    pub total_pending_approvals: u32,
    pub hosts: Vec<HostCardView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCardView {
    pub agent_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub title: String,
    pub provider: ProviderKind,
    pub provider_label: String,
    pub model: Option<String>,
    pub mode: SessionMode,
    pub status: AgentStatus,
    pub status_label: String,
    pub is_running: bool,
    pub is_awaiting_permission: bool,
    pub latest_seq: u64,
    pub updated_at_rfc3339: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostDetailView {
    pub revision: u64,
    pub host: HostCardView,
    pub workspaces: Vec<WorkspaceSummary>,
    pub agents: Vec<AgentCardView>,
    pub quotas: Vec<ProviderQuotaSnapshot>,
    pub terminals: Vec<TerminalSummary>,
    pub active_terminal_id: Option<String>,
    pub active_terminal_slot: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "item_type", rename_all = "snake_case")]
pub enum TimelineItemView {
    UserPrompt {
        seq: u64,
        message_id: String,
        text: String,
        blocks: Vec<RichBlockView>,
    },
    AssistantThinking {
        seq: u64,
        item_id: String,
        text: String,
    },
    AssistantMessage {
        seq: u64,
        item_id: String,
        text: String,
        streaming: bool,
        blocks: Vec<RichBlockView>,
    },
    ToolCall {
        seq: u64,
        call_id: String,
        status: ToolCallStatus,
        summary_title: String,
        detail: ToolCallDetail,
        diff_files: Vec<DiffFileView>,
    },
    PermissionRequest {
        seq: u64,
        request_id: String,
        title: String,
        detail: ToolCallDetail,
        allowed_decisions: Vec<String>,
        resolved_decision: Option<String>,
    },
    QuestionRequest {
        seq: u64,
        request_id: String,
        questions: Vec<QuestionItem>,
        resolved_answers: Option<Vec<Vec<String>>>,
    },
    Notice {
        seq: u64,
        level: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTimelineView {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<AgentCardView>,
    pub items: Vec<TimelineItemView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitReviewView {
    pub revision: u64,
    pub host_id: String,
    pub cwd: String,
    pub status: Option<GitStatusSnapshot>,
    pub selected_path: Option<String>,
    pub raw_diff: String,
    pub diff_files: Vec<DiffFileView>,
}

struct HostMemoryState {
    profile: SavedHostProfile,
    last_error: Option<String>,
    available_providers: Vec<ProviderKind>,
    session: Option<HostSessionHandle>,
    workspaces: Vec<WorkspaceSummary>,
    agents: HashMap<String, AgentSummary>,
    timelines: HashMap<String, Vec<TimelineEvent>>,
    markdown_cache: HashMap<(u64, usize, bool), Vec<RichBlockView>>,
    quotas: Vec<ProviderQuotaSnapshot>,
    terminals: Vec<TerminalSummary>,
    active_terminal_id: Option<String>,
    active_terminal_slot: u8,
    terminal_emulator: TerminalGridEmulator,
    git_statuses: HashMap<String, GitStatusSnapshot>,
    git_diffs: HashMap<String, String>,
}

impl HostMemoryState {
    fn new(profile: SavedHostProfile) -> Self {
        Self {
            profile,
            last_error: None,
            available_providers: vec![
                ProviderKind::Antigravity,
                ProviderKind::Codex,
                ProviderKind::ClaudeCode,
            ],
            session: None,
            workspaces: Vec::new(),
            agents: HashMap::new(),
            timelines: HashMap::new(),
            markdown_cache: HashMap::new(),
            quotas: Vec::new(),
            terminals: Vec::new(),
            active_terminal_id: None,
            active_terminal_slot: 1,
            terminal_emulator: TerminalGridEmulator::new(100, 30),
            git_statuses: HashMap::new(),
            git_diffs: HashMap::new(),
        }
    }

    fn to_card(&self) -> HostCardView {
        let pending_approval_count = self
            .agents
            .values()
            .filter(|a| matches!(a.status, AgentStatus::AwaitingPermission { .. }))
            .count() as u32;
        HostCardView {
            host_id: self.profile.host_id.clone(),
            display_name: self.profile.display_name.clone(),
            daemon_pubkey_b64: self.profile.daemon_pubkey_b64.clone(),
            direct_endpoints: self.profile.direct_endpoints.clone(),
            relay_url: self.profile.relay_url.clone(),
            connection_state: self.profile.state,
            last_error: self.last_error.clone(),
            available_providers: self.available_providers.clone(),
            workspace_count: self.workspaces.len() as u32,
            agent_count: self.agents.len() as u32,
            pending_approval_count,
        }
    }
}

/// Thread-safe, self-contained Rust Application Core with its own background Tokio runtime.
pub struct OrbitAppCore {
    runtime: tokio::runtime::Runtime,
    store: Arc<OrbitStore>,
    hosts: Arc<RwLock<HashMap<String, HostMemoryState>>>,
    subscribers: Arc<Mutex<Vec<Arc<dyn CoreEventSubscriber>>>>,
    revision: Arc<AtomicU64>,
}

impl OrbitAppCore {
    pub fn open(
        local_db_path: impl AsRef<Path>,
        auto_discover_local_daemon: bool,
    ) -> Result<Arc<Self>, ClientCoreError> {
        let store = Arc::new(OrbitStore::open(local_db_path)?);
        Self::from_store(store, auto_discover_local_daemon)
    }

    pub fn open_in_memory() -> Result<Arc<Self>, ClientCoreError> {
        let store = Arc::new(OrbitStore::open_in_memory()?);
        Self::from_store(store, false)
    }

    fn from_store(
        store: Arc<OrbitStore>,
        auto_discover_local_daemon: bool,
    ) -> Result<Arc<Self>, ClientCoreError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| ClientCoreError::Handshake(format!("tokio init failed: {e}")))?;

        let mut initial_hosts = HashMap::new();
        if let Ok(saved) = store.list_client_hosts() {
            for h in saved {
                let profile = SavedHostProfile {
                    host_id: h.host_id.clone(),
                    display_name: h.display_name,
                    daemon_pubkey_b64: h.daemon_pubkey_b64,
                    direct_endpoints: h.direct_endpoints,
                    relay_url: h.relay_url,
                    state: HostConnectionState::Disconnected,
                };
                initial_hosts.insert(h.host_id, HostMemoryState::new(profile));
            }
        }

        if auto_discover_local_daemon
            && let Some(uri) = discover_local_daemon_pairing_uri()
            && let Ok(offer) = PairingOffer::from_qr_uri(&uri)
            && !initial_hosts.contains_key(&offer.host_id)
        {
            let mut endpoints = offer.direct_endpoints;
            let loopback = "ws://127.0.0.1:6868/ws".to_string();
            if !endpoints.contains(&loopback) {
                endpoints.insert(0, loopback);
            }
            let stored = StoredHostProfile {
                host_id: offer.host_id.clone(),
                display_name: offer.hostname.clone(),
                daemon_pubkey_b64: offer.daemon_pubkey_b64.clone(),
                direct_endpoints: endpoints.clone(),
                relay_url: offer.relay_url.clone(),
            };
            let _ = store.upsert_client_host(&stored);
            initial_hosts.insert(
                offer.host_id.clone(),
                HostMemoryState::new(SavedHostProfile {
                    host_id: offer.host_id,
                    display_name: offer.hostname,
                    daemon_pubkey_b64: offer.daemon_pubkey_b64,
                    direct_endpoints: endpoints,
                    relay_url: offer.relay_url,
                    state: HostConnectionState::Disconnected,
                }),
            );
        }

        Ok(Arc::new(Self {
            runtime,
            store,
            hosts: Arc::new(RwLock::new(initial_hosts)),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            revision: Arc::new(AtomicU64::new(1)),
        }))
    }

    pub fn subscribe(&self, sub: Arc<dyn CoreEventSubscriber>) {
        if let Ok(mut guard) = self.subscribers.lock() {
            guard.push(sub);
        }
    }

    fn emit(&self, make_event: impl FnOnce(u64) -> CoreNotification) {
        let rev = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
        let ev = make_event(rev);
        let subs = self
            .subscribers
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        for s in subs {
            s.on_notification(ev.clone());
        }
    }

    pub fn fleet_overview(&self) -> FleetOverviewView {
        let rev = self.revision.load(Ordering::SeqCst);
        let Ok(guard) = self.hosts.read() else {
            return FleetOverviewView {
                revision: rev,
                total_pending_approvals: 0,
                hosts: Vec::new(),
            };
        };
        let mut cards: Vec<HostCardView> = guard.values().map(HostMemoryState::to_card).collect();
        cards.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        let total_pending_approvals = cards.iter().map(|c| c.pending_approval_count).sum();
        FleetOverviewView {
            revision: rev,
            total_pending_approvals,
            hosts: cards,
        }
    }

    pub fn host_detail(&self, host_id: &str) -> Option<HostDetailView> {
        let rev = self.revision.load(Ordering::SeqCst);
        let guard = self.hosts.read().ok()?;
        let state = guard.get(host_id)?;
        let mut agents: Vec<AgentCardView> =
            state.agents.values().map(agent_to_card_view).collect();
        agents.sort_by(|a, b| b.updated_at_rfc3339.cmp(&a.updated_at_rfc3339));
        Some(HostDetailView {
            revision: rev,
            host: state.to_card(),
            workspaces: state.workspaces.clone(),
            agents,
            quotas: state.quotas.clone(),
            terminals: state.terminals.clone(),
            active_terminal_id: state.active_terminal_id.clone(),
            active_terminal_slot: state.active_terminal_slot,
        })
    }

    pub fn agent_timeline(&self, host_id: &str, agent_id: &str) -> AgentTimelineView {
        let rev = self.revision.load(Ordering::SeqCst);
        let Ok(mut guard) = self.hosts.write() else {
            return AgentTimelineView {
                revision: rev,
                host_id: host_id.to_string(),
                agent: None,
                items: Vec::new(),
            };
        };
        let Some(host_state) = guard.get_mut(host_id) else {
            return AgentTimelineView {
                revision: rev,
                host_id: host_id.to_string(),
                agent: None,
                items: Vec::new(),
            };
        };

        // Hydrate from local SQLite replica on first open for 0ms offline-first rendering
        if !host_state.timelines.contains_key(agent_id)
            && let Ok((cached, _)) = self.store.fetch_timeline_since(agent_id, 0, 500)
            && !cached.is_empty()
        {
            host_state.timelines.insert(agent_id.to_string(), cached);
        }

        let agent_card = host_state.agents.get(agent_id).map(agent_to_card_view);
        let events = host_state
            .timelines
            .get(agent_id)
            .cloned()
            .unwrap_or_default();

        // Collect resolved permission & question decisions so cards render their final state
        let mut resolved_perms: HashMap<String, String> = HashMap::new();
        let mut resolved_questions: HashMap<String, Vec<Vec<String>>> = HashMap::new();
        for ev in &events {
            match &ev.payload {
                TimelinePayload::PermissionResolved {
                    request_id,
                    decision,
                } => {
                    resolved_perms.insert(request_id.clone(), decision.clone());
                }
                TimelinePayload::QuestionResolved {
                    request_id,
                    answers,
                } => {
                    resolved_questions.insert(request_id.clone(), answers.clone());
                }
                _ => {}
            }
        }

        let mut items = Vec::with_capacity(events.len());
        for ev in &events {
            match &ev.payload {
                TimelinePayload::UserPrompt { message_id, text } => {
                    let key = (ev.seq, text.len(), false);
                    let blocks = host_state
                        .markdown_cache
                        .entry(key)
                        .or_insert_with(|| parse_markdown_blocks(text, false))
                        .clone();
                    items.push(TimelineItemView::UserPrompt {
                        seq: ev.seq,
                        message_id: message_id.clone(),
                        text: text.clone(),
                        blocks,
                    });
                }
                TimelinePayload::AssistantThinking { item_id, text } => {
                    items.push(TimelineItemView::AssistantThinking {
                        seq: ev.seq,
                        item_id: item_id.clone(),
                        text: text.clone(),
                    });
                }
                TimelinePayload::AssistantMessage {
                    item_id,
                    text,
                    streaming,
                } => {
                    let key = (ev.seq, text.len(), *streaming);
                    let blocks = host_state
                        .markdown_cache
                        .entry(key)
                        .or_insert_with(|| parse_markdown_blocks(text, *streaming))
                        .clone();
                    items.push(TimelineItemView::AssistantMessage {
                        seq: ev.seq,
                        item_id: item_id.clone(),
                        text: text.clone(),
                        streaming: *streaming,
                        blocks,
                    });
                }
                TimelinePayload::ToolCall {
                    call_id,
                    status,
                    detail,
                } => {
                    let (summary_title, diff_files) = summarize_tool_detail(detail);
                    items.push(TimelineItemView::ToolCall {
                        seq: ev.seq,
                        call_id: call_id.clone(),
                        status: *status,
                        summary_title,
                        detail: detail.clone(),
                        diff_files,
                    });
                }
                TimelinePayload::PermissionRequest {
                    request_id,
                    title,
                    detail,
                    allowed_decisions,
                } => {
                    items.push(TimelineItemView::PermissionRequest {
                        seq: ev.seq,
                        request_id: request_id.clone(),
                        title: title.clone(),
                        detail: detail.clone(),
                        allowed_decisions: allowed_decisions.clone(),
                        resolved_decision: resolved_perms.get(request_id).cloned(),
                    });
                }
                TimelinePayload::QuestionRequest {
                    request_id,
                    questions,
                } => {
                    items.push(TimelineItemView::QuestionRequest {
                        seq: ev.seq,
                        request_id: request_id.clone(),
                        questions: questions.clone(),
                        resolved_answers: resolved_questions.get(request_id).cloned(),
                    });
                }
                TimelinePayload::Notice { level, message } => {
                    items.push(TimelineItemView::Notice {
                        seq: ev.seq,
                        level: level.clone(),
                        message: message.clone(),
                    });
                }
                TimelinePayload::PermissionResolved { .. }
                | TimelinePayload::QuestionResolved { .. }
                | TimelinePayload::StatusChanged { .. } => {}
            }
        }

        AgentTimelineView {
            revision: rev,
            host_id: host_id.to_string(),
            agent: agent_card,
            items,
        }
    }

    pub fn git_review(&self, host_id: &str, cwd: &str, path: Option<&str>) -> GitReviewView {
        let rev = self.revision.load(Ordering::SeqCst);
        let Ok(guard) = self.hosts.read() else {
            return GitReviewView {
                revision: rev,
                host_id: host_id.to_string(),
                cwd: cwd.to_string(),
                status: None,
                selected_path: path.map(ToOwned::to_owned),
                raw_diff: String::new(),
                diff_files: Vec::new(),
            };
        };
        let Some(host_state) = guard.get(host_id) else {
            return GitReviewView {
                revision: rev,
                host_id: host_id.to_string(),
                cwd: cwd.to_string(),
                status: None,
                selected_path: path.map(ToOwned::to_owned),
                raw_diff: String::new(),
                diff_files: Vec::new(),
            };
        };

        let key = format!("{cwd}::{}", path.unwrap_or("__all__"));
        let raw_diff = host_state.git_diffs.get(&key).cloned().unwrap_or_default();
        let diff_files = parse_unified_diff(&raw_diff);
        GitReviewView {
            revision: rev,
            host_id: host_id.to_string(),
            cwd: cwd.to_string(),
            status: host_state.git_statuses.get(cwd).cloned(),
            selected_path: path.map(ToOwned::to_owned),
            raw_diff,
            diff_files,
        }
    }

    pub fn terminal_screen(&self, host_id: &str) -> Option<TerminalScreenView> {
        let guard = self.hosts.read().ok()?;
        let state = guard.get(host_id)?;
        Some(state.terminal_emulator.snapshot())
    }

    // --- Commands / Intents ---

    pub fn pair_host(self: &Arc<Self>, pairing_uri: &str) -> Result<HostCardView, ClientCoreError> {
        let offer = PairingOffer::from_qr_uri(pairing_uri)?;
        let stored = StoredHostProfile {
            host_id: offer.host_id.clone(),
            display_name: offer.hostname.clone(),
            daemon_pubkey_b64: offer.daemon_pubkey_b64.clone(),
            direct_endpoints: offer.direct_endpoints.clone(),
            relay_url: offer.relay_url.clone(),
        };
        self.store.upsert_client_host(&stored)?;

        let profile = SavedHostProfile {
            host_id: offer.host_id.clone(),
            display_name: offer.hostname,
            daemon_pubkey_b64: offer.daemon_pubkey_b64,
            direct_endpoints: offer.direct_endpoints,
            relay_url: offer.relay_url,
            state: HostConnectionState::Disconnected,
        };
        let card = {
            let mut guard = self
                .hosts
                .write()
                .map_err(|_| ClientCoreError::LockPoisoned)?;
            let entry = guard
                .entry(offer.host_id.clone())
                .or_insert_with(|| HostMemoryState::new(profile.clone()));
            entry.profile = profile;
            entry.to_card()
        };
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.connect_host_async(offer.host_id);
        Ok(card)
    }

    pub fn remove_host(&self, host_id: &str) -> Result<(), ClientCoreError> {
        let _ = self.store.delete_client_host(host_id);
        {
            let mut guard = self
                .hosts
                .write()
                .map_err(|_| ClientCoreError::LockPoisoned)?;
            guard.remove(host_id);
        }
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(())
    }

    pub fn update_host_relay(
        self: &Arc<Self>,
        host_id: &str,
        raw_relay_url: Option<&str>,
    ) -> Result<(), ClientCoreError> {
        let normalized = normalize_relay_input(raw_relay_url);
        let mut stored_opt = None;
        {
            let mut guard = self
                .hosts
                .write()
                .map_err(|_| ClientCoreError::LockPoisoned)?;
            if let Some(h) = guard.get_mut(host_id) {
                h.profile.relay_url = normalized;
                stored_opt = Some(StoredHostProfile {
                    host_id: h.profile.host_id.clone(),
                    display_name: h.profile.display_name.clone(),
                    daemon_pubkey_b64: h.profile.daemon_pubkey_b64.clone(),
                    direct_endpoints: h.profile.direct_endpoints.clone(),
                    relay_url: h.profile.relay_url.clone(),
                });
            }
        }
        if let Some(stored) = stored_opt {
            self.store.upsert_client_host(&stored)?;
            self.emit(|revision| CoreNotification::FleetChanged { revision });
            self.connect_host_async(host_id.to_string());
        }
        Ok(())
    }

    pub fn connect_all_hosts(self: &Arc<Self>) {
        let ids: Vec<String> = self
            .hosts
            .read()
            .map(|g| g.keys().cloned().collect())
            .unwrap_or_default();
        for id in ids {
            self.connect_host_async(id);
        }
    }

    pub fn connect_host_async(self: &Arc<Self>, host_id: String) {
        let core = Arc::clone(self);
        self.runtime.spawn(async move {
            core.connect_host_task(&host_id).await;
        });
    }

    async fn connect_host_task(self: &Arc<Self>, host_id: &str) {
        let profile = {
            let Ok(mut guard) = self.hosts.write() else {
                return;
            };
            let Some(h) = guard.get_mut(host_id) else {
                return;
            };
            h.profile.state = if h.profile.direct_endpoints.is_empty() {
                HostConnectionState::ConnectingRelay
            } else {
                HostConnectionState::ConnectingDirect
            };
            h.last_error = None;
            h.session = None;
            h.profile.clone()
        };
        self.emit(|revision| CoreNotification::FleetChanged { revision });

        match connect_smart(&profile, Some(self.store.clone())).await {
            Ok(handle) => {
                let mut rx = handle.subscribe();
                {
                    let Ok(mut guard) = self.hosts.write() else {
                        return;
                    };
                    if let Some(h) = guard.get_mut(host_id) {
                        h.profile.state = handle.mode;
                        h.last_error = None;
                        h.session = Some(handle.clone());
                    }
                }
                self.emit(|revision| CoreNotification::FleetChanged { revision });

                // Request initial snapshots immediately upon connecting
                let _ = handle
                    .send_wire(WireMessage::ListWorkspaces {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListAgents {
                        request_id: Uuid::new_v4(),
                        workspace_id: None,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListTerminals {
                        request_id: Uuid::new_v4(),
                    })
                    .await;

                let loop_core = Arc::clone(self);
                let owned_host_id = host_id.to_string();
                tokio::spawn(async move {
                    while let Ok(ev) = rx.recv().await {
                        match ev {
                            ClientInboundEvent::Wire(wire) => {
                                loop_core.handle_inbound_wire(&owned_host_id, wire);
                            }
                            ClientInboundEvent::Terminal(term) => {
                                loop_core.handle_inbound_terminal(&owned_host_id, term);
                            }
                            ClientInboundEvent::Disconnected => {
                                if let Ok(mut guard) = loop_core.hosts.write()
                                    && let Some(h) = guard.get_mut(&owned_host_id)
                                {
                                    h.profile.state = HostConnectionState::Disconnected;
                                    h.session = None;
                                }
                                loop_core
                                    .emit(|revision| CoreNotification::FleetChanged { revision });
                                break;
                            }
                        }
                    }
                });
            }
            Err(err) => {
                if let Ok(mut guard) = self.hosts.write()
                    && let Some(h) = guard.get_mut(host_id)
                {
                    h.profile.state = HostConnectionState::Disconnected;
                    h.last_error = Some(err.to_string());
                }
                self.emit(|revision| CoreNotification::FleetChanged { revision });
            }
        }
    }

    fn handle_inbound_wire(&self, host_id: &str, msg: WireMessage) {
        let mut notify_fleet = false;
        let mut notify_host = false;
        let mut notify_timeline_agent: Option<String> = None;
        let mut notify_git_cwd: Option<String> = None;

        if let Ok(mut guard) = self.hosts.write()
            && let Some(state) = guard.get_mut(host_id)
        {
            match msg {
                WireMessage::Welcome { host } => {
                    state.profile.display_name = host.hostname;
                    if !host.available_providers.is_empty() {
                        state.available_providers = host.available_providers;
                    }
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::WorkspacesSnapshot { workspaces, .. } => {
                    state.workspaces = workspaces;
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::WorkspaceCreated { workspace, .. } => {
                    state
                        .workspaces
                        .retain(|w| w.workspace_id != workspace.workspace_id);
                    state.workspaces.insert(0, workspace);
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::AgentsSnapshot { agents, .. } => {
                    state.agents.clear();
                    for a in agents {
                        state.agents.insert(a.agent_id.clone(), a);
                    }
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::AgentCreated { agent, .. } | WireMessage::AgentUpdated { agent } => {
                    let ag_id = agent.agent_id.clone();
                    state.agents.insert(ag_id.clone(), agent);
                    notify_fleet = true;
                    notify_host = true;
                    notify_timeline_agent = Some(ag_id);
                }
                WireMessage::TimelineCatchup {
                    agent_id, events, ..
                } => {
                    let list = state.timelines.entry(agent_id.clone()).or_default();
                    let existing: HashSet<u64> = list.iter().map(|e| e.seq).collect();
                    for ev in events {
                        if !existing.contains(&ev.seq) {
                            coalesce_or_push_timeline(list, ev);
                        }
                    }
                    list.sort_by_key(|e| e.seq);
                    notify_timeline_agent = Some(agent_id);
                }
                WireMessage::TimelineLive { event } => {
                    let agent_id = event.agent_id.clone();
                    if let TimelinePayload::StatusChanged { ref status } = event.payload
                        && let Some(ag) = state.agents.get_mut(&agent_id)
                    {
                        ag.status = status.clone();
                        notify_fleet = true;
                        notify_host = true;
                    }
                    let list = state.timelines.entry(agent_id.clone()).or_default();
                    coalesce_or_push_timeline(list, event);
                    notify_timeline_agent = Some(agent_id);
                }
                WireMessage::GitStatusResponse { status, .. } => {
                    let cwd = status.cwd.clone();
                    state.git_statuses.insert(cwd.clone(), status);
                    notify_git_cwd = Some(cwd);
                }
                WireMessage::GitDiffResponse {
                    cwd,
                    path,
                    unified_diff,
                    ..
                } => {
                    let key = format!("{cwd}::{}", path.as_deref().unwrap_or("__all__"));
                    state.git_diffs.insert(key, unified_diff);
                    notify_git_cwd = Some(cwd);
                }
                WireMessage::TerminalsSnapshot { terminals, .. } => {
                    state.terminals = terminals;
                    notify_host = true;
                }
                WireMessage::TerminalAttached { terminal, slot, .. } => {
                    state.active_terminal_id = Some(terminal.terminal_id.clone());
                    state.active_terminal_slot = slot;
                    state.terminal_emulator.resize(terminal.cols, terminal.rows);
                    state
                        .terminals
                        .retain(|t| t.terminal_id != terminal.terminal_id);
                    state.terminals.insert(0, terminal);
                    notify_host = true;
                }
                WireMessage::QuotaReport { snapshots, .. } => {
                    state.quotas = snapshots;
                    notify_host = true;
                }
                WireMessage::RpcError { code, message, .. } => {
                    state.last_error = Some(format!("[{code}] {message}"));
                    notify_host = true;
                }
                _ => {}
            }
        }

        if notify_fleet {
            self.emit(|revision| CoreNotification::FleetChanged { revision });
        }
        if notify_host {
            self.emit(|revision| CoreNotification::HostChanged {
                revision,
                host_id: host_id.to_string(),
            });
        }
        if let Some(agent_id) = notify_timeline_agent {
            self.emit(|revision| CoreNotification::TimelineChanged {
                revision,
                host_id: host_id.to_string(),
                agent_id,
            });
        }
        if let Some(cwd) = notify_git_cwd {
            self.emit(|revision| CoreNotification::GitChanged {
                revision,
                host_id: host_id.to_string(),
                cwd,
            });
        }
    }

    fn handle_inbound_terminal(&self, host_id: &str, frame: TerminalBinaryFrame) {
        let mut raw_out: Option<(u8, Vec<u8>)> = None;
        if let Ok(mut guard) = self.hosts.write()
            && let Some(state) = guard.get_mut(host_id)
        {
            match frame {
                TerminalBinaryFrame::Output { slot, data } => {
                    state.terminal_emulator.process(&data);
                    raw_out = Some((slot, data));
                }
                TerminalBinaryFrame::Snapshot { slot, data } => {
                    state.terminal_emulator.clear();
                    state.terminal_emulator.process(&data);
                    raw_out = Some((slot, data));
                }
                TerminalBinaryFrame::Resize { cols, rows, .. } => {
                    state.terminal_emulator.resize(cols, rows);
                }
                TerminalBinaryFrame::Input { .. } => {}
            }
        }

        if let Some((slot, raw_bytes)) = raw_out {
            self.emit(|revision| CoreNotification::TerminalChanged {
                revision,
                host_id: host_id.to_string(),
                slot,
                raw_bytes,
            });
        }
    }

    fn dispatch_wire(&self, host_id: &str, msg: WireMessage) {
        let session = self
            .hosts
            .read()
            .ok()
            .and_then(|g| g.get(host_id).and_then(|h| h.session.clone()));
        if let Some(handle) = session {
            self.runtime.spawn(async move {
                let _ = handle.send_wire(msg).await;
            });
        }
    }

    pub fn refresh_host(&self, host_id: &str) {
        self.dispatch_wire(
            host_id,
            WireMessage::ListWorkspaces {
                request_id: Uuid::new_v4(),
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::ListAgents {
                request_id: Uuid::new_v4(),
                workspace_id: None,
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::ListTerminals {
                request_id: Uuid::new_v4(),
            },
        );
    }

    pub fn refresh_quotas(&self, host_id: &str) {
        self.dispatch_wire(
            host_id,
            WireMessage::RefreshQuotas {
                request_id: Uuid::new_v4(),
            },
        );
    }

    pub fn create_agent(
        &self,
        host_id: &str,
        cwd: &str,
        provider: ProviderKind,
        mode: SessionMode,
        initial_prompt: &str,
        worktree_branch: Option<&str>,
    ) {
        if let Some(branch) = worktree_branch.map(str::trim).filter(|b| !b.is_empty()) {
            self.dispatch_wire(
                host_id,
                WireMessage::CreateWorkspace {
                    request_id: Uuid::new_v4(),
                    source_cwd: cwd.to_string(),
                    title: Some(branch.to_string()),
                    worktree_branch: Some(branch.to_string()),
                    base_ref: None,
                },
            );
        }
        self.dispatch_wire(
            host_id,
            WireMessage::CreateAgent {
                request_id: Uuid::new_v4(),
                cwd: cwd.to_string(),
                title: None,
                provider,
                model: None,
                mode,
                initial_prompt: initial_prompt.to_string(),
            },
        );
    }

    pub fn open_agent_timeline(&self, host_id: &str, agent_id: &str) {
        let since_seq = self
            .hosts
            .read()
            .ok()
            .and_then(|g| {
                g.get(host_id)
                    .and_then(|h| h.timelines.get(agent_id))
                    .and_then(|list| list.last().map(|e| e.seq))
            })
            .unwrap_or(0);
        self.dispatch_wire(
            host_id,
            WireMessage::SubscribeTimeline {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
                since_seq,
            },
        );
    }

    pub fn send_prompt(&self, host_id: &str, agent_id: &str, prompt: &str) {
        self.dispatch_wire(
            host_id,
            WireMessage::SendPrompt {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
                prompt: prompt.to_string(),
            },
        );
    }

    pub fn interrupt_agent(&self, host_id: &str, agent_id: &str) {
        self.dispatch_wire(
            host_id,
            WireMessage::InterruptTurn {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
            },
        );
    }

    pub fn set_session_mode(&self, host_id: &str, agent_id: &str, mode: SessionMode) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            ag.mode = mode.clone();
        }
        self.dispatch_wire(
            host_id,
            WireMessage::SetSessionMode {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
                mode,
            },
        );
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn archive_agent(&self, host_id: &str, agent_id: &str) {
        self.dispatch_wire(
            host_id,
            WireMessage::ArchiveAgent {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
            },
        );
    }

    pub fn resolve_permission(
        &self,
        host_id: &str,
        agent_id: &str,
        permission_request_id: &str,
        decision: &str,
    ) {
        self.dispatch_wire(
            host_id,
            WireMessage::ResolvePermission {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
                permission_request_id: permission_request_id.to_string(),
                decision: decision.to_string(),
            },
        );
    }

    pub fn answer_question(
        &self,
        host_id: &str,
        agent_id: &str,
        question_request_id: &str,
        answers: Vec<Vec<String>>,
    ) {
        self.dispatch_wire(
            host_id,
            WireMessage::AnswerQuestion {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
                question_request_id: question_request_id.to_string(),
                answers,
            },
        );
    }

    pub fn fetch_git_status(&self, host_id: &str, cwd: &str) {
        self.dispatch_wire(
            host_id,
            WireMessage::GetGitStatus {
                request_id: Uuid::new_v4(),
                cwd: cwd.to_string(),
            },
        );
    }

    pub fn fetch_git_diff(&self, host_id: &str, cwd: &str, path: Option<&str>, staged: bool) {
        self.dispatch_wire(
            host_id,
            WireMessage::GetGitDiff {
                request_id: Uuid::new_v4(),
                cwd: cwd.to_string(),
                path: path.map(ToOwned::to_owned),
                staged,
            },
        );
    }

    pub fn start_terminal(&self, host_id: &str, cwd: &str, cols: u16, rows: u16) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.terminal_emulator.clear();
            h.terminal_emulator.resize(cols, rows);
        }
        self.dispatch_wire(
            host_id,
            WireMessage::CreateTerminal {
                request_id: Uuid::new_v4(),
                cwd: cwd.to_string(),
                title: Some("Orbit PTY".to_string()),
                cols,
                rows,
            },
        );
    }

    pub fn attach_terminal(&self, host_id: &str, terminal_id: &str, slot: u8) {
        self.dispatch_wire(
            host_id,
            WireMessage::AttachTerminal {
                request_id: Uuid::new_v4(),
                terminal_id: terminal_id.to_string(),
                slot,
            },
        );
    }

    pub fn send_terminal_input(&self, host_id: &str, input_bytes: Vec<u8>) {
        let pair = self.hosts.read().ok().and_then(|g| {
            g.get(host_id)
                .and_then(|h| h.session.clone().map(|s| (h.active_terminal_slot, s)))
        });
        if let Some((slot, handle)) = pair {
            self.runtime.spawn(async move {
                let _ = handle
                    .send_terminal(TerminalBinaryFrame::Input {
                        slot,
                        data: input_bytes,
                    })
                    .await;
            });
        }
    }

    pub fn resize_terminal(&self, host_id: &str, cols: u16, rows: u16) {
        let mut pair = None;
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.terminal_emulator.resize(cols, rows);
            if let Some(ref s) = h.session {
                pair = Some((h.active_terminal_slot, s.clone()));
            }
        }
        if let Some((slot, handle)) = pair {
            self.runtime.spawn(async move {
                let _ = handle
                    .send_terminal(TerminalBinaryFrame::Resize { slot, cols, rows })
                    .await;
            });
        }
    }
}

fn coalesce_or_push_timeline(list: &mut Vec<TimelineEvent>, incoming: TimelineEvent) {
    if let TimelinePayload::AssistantMessage { ref item_id, .. } = incoming.payload
        && let Some(prev) = list.iter_mut().rev().find(|prev_ev| {
            matches!(
                &prev_ev.payload,
                TimelinePayload::AssistantMessage {
                    item_id: existing_id,
                    ..
                } if existing_id == item_id
            )
        })
    {
        *prev = incoming;
        return;
    }
    list.push(incoming);
}

fn agent_to_card_view(a: &AgentSummary) -> AgentCardView {
    let (status_label, is_running, is_awaiting_permission) = match &a.status {
        AgentStatus::Initializing => ("Initializing".to_string(), true, false),
        AgentStatus::Idle => ("Idle".to_string(), false, false),
        AgentStatus::Running { .. } => ("Running".to_string(), true, false),
        AgentStatus::AutonomousWakeup { source, .. } => (format!("Wakeup ({source})"), true, false),
        AgentStatus::AwaitingPermission { .. } => ("Needs Approval".to_string(), false, true),
        AgentStatus::Error { .. } => ("Error".to_string(), false, false),
        AgentStatus::Closed => ("Closed".to_string(), false, false),
    };
    AgentCardView {
        agent_id: a.agent_id.clone(),
        workspace_id: a.workspace_id.clone(),
        cwd: a.cwd.clone(),
        title: a.title.clone(),
        provider: a.provider,
        provider_label: a.provider.display_name().to_string(),
        model: a.model.clone(),
        mode: a.mode.clone(),
        status: a.status.clone(),
        status_label,
        is_running,
        is_awaiting_permission,
        latest_seq: a.latest_seq,
        updated_at_rfc3339: a.updated_at.to_rfc3339(),
    }
}

fn summarize_tool_detail(detail: &ToolCallDetail) -> (String, Vec<DiffFileView>) {
    match detail {
        ToolCallDetail::Shell { command, .. } => (format!("$ {command}"), Vec::new()),
        ToolCallDetail::FileRead { path, .. } => (format!("Read {path}"), Vec::new()),
        ToolCallDetail::FileEdit { path, unified_diff } => {
            let diffs = unified_diff
                .as_deref()
                .map(parse_unified_diff)
                .unwrap_or_default();
            (format!("Edit {path}"), diffs)
        }
        ToolCallDetail::Search { query, .. } => (format!("Search \"{query}\""), Vec::new()),
        ToolCallDetail::Subagent { role, .. } => (format!("Subagent · {role}"), Vec::new()),
        ToolCallDetail::Generic { tool_name, .. } => (tool_name.clone(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_core_persists_paired_host_and_projects_overview() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = PairingOffer {
            host_id: "host_devbox".into(),
            hostname: "cloud-devbox".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "test_secret".into(),
        };
        let uri = offer.to_qr_uri().expect("qr uri");
        let card = core.pair_host(&uri).expect("pair");
        assert_eq!(card.host_id, "host_devbox");
        assert_eq!(card.display_name, "cloud-devbox");

        let overview = core.fleet_overview();
        assert_eq!(overview.hosts.len(), 1);
    }
}
