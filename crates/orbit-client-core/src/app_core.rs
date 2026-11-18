//! Headless Multi-Host MVU Application Core (`OrbitAppCore`) shared across:
//! - Desktop: Tauri v2 (macOS & Linux)
//! - iOS 16.4+: Swift + SwiftUI (via `orbit-ffi-uniffi`)
//! - Android (minSdk 29 / targetSdk 36): Kotlin + Jetpack Compose (via `orbit-ffi-uniffi`)
//! - HarmonyOS 7+ (API 26): ArkTS + ArkUI V2 (via `orbit-ffi-ohos`)

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use orbit_crypto::PairingOffer;
use orbit_protocol::{
    AgentStatus, AgentSummary, DirectoryEntry, DiscoveredProject, GitStatusSnapshot, ModelOption,
    ProviderKind, ProviderQuotaSnapshot, QuestionItem, SessionMode, TerminalBinaryFrame,
    TerminalSummary, TimelineEvent, TimelinePayload, ToolCallDetail, ToolCallStatus, WireMessage,
    WorkspaceSummary,
};
use orbit_store::{OrbitStore, StoredClientProject, StoredHostMeta, StoredHostProfile};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::desktop_ops::{
    bootstrap_ssh_host_and_pair, discover_local_daemon_pairing_uri, normalize_relay_input,
};
use crate::render::{
    DiffFileView, RichBlockView, TerminalGridEmulator, TerminalScreenView, parse_markdown_blocks,
    parse_unified_diff,
};
use crate::{
    ClientCoreError, ClientInboundEvent, HostConnectionState, HostRouteMode, HostSessionHandle,
    SavedHostProfile, connect_smart,
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
pub struct ClientProjectView {
    pub project_id: String,
    pub host_id: String,
    pub name: String,
    pub directories: Vec<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryListingView {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub entries: Vec<DirectoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostCardView {
    pub host_id: String,
    pub display_name: String,
    pub hostname: String,
    pub custom_name: Option<String>,
    pub ssh_alias: Option<String>,
    pub route_mode: HostRouteMode,
    pub remote_daemon_port: u16,
    pub custom_direct_endpoint: Option<String>,
    pub via_ssh_tunnel: bool,
    pub last_ping_ms: Option<u32>,
    pub daemon_version: Option<String>,
    pub os_arch: Option<String>,
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
    pub projects: Vec<ClientProjectView>,
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
    pub discovered_projects: Vec<DiscoveredProject>,
    pub directory_listing: Option<DirectoryListingView>,
    pub models_by_provider: HashMap<ProviderKind, Vec<ModelOption>>,
    pub workspaces: Vec<WorkspaceSummary>,
    pub agents: Vec<AgentCardView>,
    pub last_created_agent_id: Option<String>,
    pub replaced_optimistic_agents: HashMap<String, String>,
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
    via_ssh_tunnel: bool,
    last_ping_ms: Option<u32>,
    daemon_version: Option<String>,
    os_arch: Option<String>,
    reconnect_attempt: u32,
    connecting_in_flight: bool,
    available_providers: Vec<ProviderKind>,
    session: Option<HostSessionHandle>,
    pending_outbox: Vec<WireMessage>,
    optimistic_by_req_id: HashMap<Uuid, String>,
    replaced_optimistic_agents: HashMap<String, String>,
    last_created_agent_id: Option<String>,
    discovered_projects: Vec<DiscoveredProject>,
    directory_listing: Option<DirectoryListingView>,
    models_by_provider: HashMap<ProviderKind, Vec<ModelOption>>,
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
            via_ssh_tunnel: false,
            last_ping_ms: None,
            daemon_version: None,
            os_arch: None,
            reconnect_attempt: 0,
            connecting_in_flight: false,
            available_providers: vec![
                ProviderKind::Antigravity,
                ProviderKind::Codex,
                ProviderKind::ClaudeCode,
            ],
            session: None,
            pending_outbox: Vec::new(),
            optimistic_by_req_id: HashMap::new(),
            replaced_optimistic_agents: HashMap::new(),
            last_created_agent_id: None,
            discovered_projects: Vec::new(),
            directory_listing: None,
            models_by_provider: default_client_models_map(),
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

    fn to_stored_host(&self) -> StoredHostProfile {
        StoredHostProfile {
            host_id: self.profile.host_id.clone(),
            display_name: self.profile.hostname.clone(),
            daemon_pubkey_b64: self.profile.daemon_pubkey_b64.clone(),
            direct_endpoints: self.profile.direct_endpoints.clone(),
            relay_url: self.profile.relay_url.clone(),
            meta: StoredHostMeta {
                custom_name: self.profile.custom_name.clone(),
                ssh_alias: self.profile.ssh_alias.clone(),
                route_mode: Some(self.profile.route_mode.as_str().to_string()),
                remote_daemon_port: Some(self.profile.remote_daemon_port),
                custom_direct_endpoint: self.profile.custom_direct_endpoint.clone(),
            },
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
            display_name: self.profile.effective_display_name(),
            hostname: self.profile.hostname.clone(),
            custom_name: self.profile.custom_name.clone(),
            ssh_alias: self.profile.ssh_alias.clone(),
            route_mode: self.profile.route_mode,
            remote_daemon_port: self.profile.remote_daemon_port,
            custom_direct_endpoint: self.profile.custom_direct_endpoint.clone(),
            via_ssh_tunnel: self.via_ssh_tunnel,
            last_ping_ms: self.last_ping_ms,
            daemon_version: self.daemon_version.clone(),
            os_arch: self.os_arch.clone(),
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
    projects: Arc<RwLock<HashMap<String, ClientProjectView>>>,
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
                    hostname: h.display_name,
                    custom_name: h.meta.custom_name,
                    ssh_alias: h.meta.ssh_alias,
                    route_mode: HostRouteMode::from_opt_str(h.meta.route_mode.as_deref()),
                    remote_daemon_port: h.meta.remote_daemon_port.unwrap_or(6868),
                    custom_direct_endpoint: h.meta.custom_direct_endpoint,
                    daemon_pubkey_b64: h.daemon_pubkey_b64,
                    direct_endpoints: h.direct_endpoints,
                    relay_url: h.relay_url,
                    state: HostConnectionState::Disconnected,
                };
                let mut mem = HostMemoryState::new(profile);
                if let Ok(ws_list) = store.list_replica_workspaces(&h.host_id) {
                    mem.workspaces = ws_list;
                }
                if let Ok(ag_list) = store.list_replica_agents(&h.host_id) {
                    for ag in ag_list {
                        mem.agents.insert(ag.agent_id.clone(), ag);
                    }
                }
                initial_hosts.insert(h.host_id, mem);
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
            let mem = HostMemoryState::new(SavedHostProfile {
                host_id: offer.host_id.clone(),
                hostname: offer.hostname,
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 6868,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: offer.daemon_pubkey_b64,
                direct_endpoints: endpoints,
                relay_url: offer.relay_url,
                state: HostConnectionState::Disconnected,
            });
            let _ = store.upsert_client_host(&mem.to_stored_host());
            initial_hosts.insert(offer.host_id, mem);
        }

        let mut initial_projects = HashMap::new();
        if let Ok(saved_projs) = store.list_client_projects() {
            for p in saved_projs {
                initial_projects.insert(
                    p.project_id.clone(),
                    ClientProjectView {
                        project_id: p.project_id,
                        host_id: p.host_id,
                        name: p.name,
                        directories: p.directories,
                        updated_at_ms: p.updated_at_ms,
                    },
                );
            }
        }

        Ok(Arc::new(Self {
            runtime,
            store,
            hosts: Arc::new(RwLock::new(initial_hosts)),
            projects: Arc::new(RwLock::new(initial_projects)),
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
        let cards = self
            .hosts
            .read()
            .map(|guard| {
                let mut v: Vec<HostCardView> =
                    guard.values().map(HostMemoryState::to_card).collect();
                v.sort_by(|a, b| a.display_name.cmp(&b.display_name));
                v
            })
            .unwrap_or_default();
        let total_pending_approvals = cards.iter().map(|c| c.pending_approval_count).sum();

        let projects = self
            .projects
            .read()
            .map(|guard| {
                let mut v: Vec<ClientProjectView> = guard.values().cloned().collect();
                v.sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
                v
            })
            .unwrap_or_default();

        FleetOverviewView {
            revision: rev,
            total_pending_approvals,
            hosts: cards,
            projects,
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
            discovered_projects: state.discovered_projects.clone(),
            directory_listing: state.directory_listing.clone(),
            models_by_provider: state.models_by_provider.clone(),
            workspaces: state.workspaces.clone(),
            agents,
            last_created_agent_id: state.last_created_agent_id.clone(),
            replaced_optimistic_agents: state.replaced_optimistic_agents.clone(),
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
            && !agent_id.starts_with("opt_")
            && let Ok(cached) = self.store.fetch_replica_timeline(agent_id, 500)
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

    // --- Paseo-Style Multi-Directory Project Management ---

    pub fn upsert_project(
        &self,
        project_id: Option<&str>,
        host_id: &str,
        name: &str,
        directories: Vec<String>,
    ) -> Result<ClientProjectView, ClientCoreError> {
        let clean_dirs: Vec<String> = directories
            .into_iter()
            .map(|d| d.trim().to_string())
            .filter(|d| !d.is_empty())
            .collect();
        let primary = clean_dirs
            .first()
            .cloned()
            .unwrap_or_else(|| "~".to_string());
        let id = project_id
            .filter(|s| !s.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("{host_id}::{}", primary.trim_end_matches('/')));
        let clean_name = if name.trim().is_empty() {
            folder_basename(&primary)
        } else {
            name.trim().to_string()
        };
        let now_ms = unix_ms_now() as i64;

        let stored = StoredClientProject {
            project_id: id.clone(),
            host_id: host_id.to_string(),
            name: clean_name.clone(),
            directories: if clean_dirs.is_empty() {
                vec![primary]
            } else {
                clean_dirs
            },
            updated_at_ms: now_ms,
        };
        self.store.upsert_client_project(&stored)?;

        let view = ClientProjectView {
            project_id: stored.project_id,
            host_id: stored.host_id,
            name: stored.name,
            directories: stored.directories,
            updated_at_ms: stored.updated_at_ms,
        };
        if let Ok(mut guard) = self.projects.write() {
            guard.insert(id, view.clone());
        }
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(view)
    }

    pub fn remove_project(&self, project_id: &str) -> Result<(), ClientCoreError> {
        self.store.delete_client_project(project_id)?;
        if let Ok(mut guard) = self.projects.write() {
            guard.remove(project_id);
        }
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(())
    }

    pub fn list_directory(self: &Arc<Self>, host_id: &str, path: Option<String>) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ListDirectory {
                request_id: Uuid::new_v4(),
                path,
            },
        );
    }

    // --- Host Pairing, 1-Click SSH & Configuration ---

    pub fn pair_host(self: &Arc<Self>, pairing_uri: &str) -> Result<HostCardView, ClientCoreError> {
        self.pair_host_internal(pairing_uri, None)
    }

    pub fn pair_ssh_host_blocking(
        self: &Arc<Self>,
        alias: &str,
        remote_port: u16,
    ) -> Result<HostCardView, ClientCoreError> {
        let uri =
            bootstrap_ssh_host_and_pair(alias, remote_port).map_err(ClientCoreError::Handshake)?;
        self.pair_host_internal(&uri, Some((alias.to_string(), remote_port)))
    }

    fn pair_host_internal(
        self: &Arc<Self>,
        pairing_uri: &str,
        ssh_info: Option<(String, u16)>,
    ) -> Result<HostCardView, ClientCoreError> {
        let offer = PairingOffer::from_qr_uri(pairing_uri)?;
        let (card, stored) = {
            let mut guard = self
                .hosts
                .write()
                .map_err(|_| ClientCoreError::LockPoisoned)?;
            let prev = guard.get(&offer.host_id);
            let custom_name = prev.and_then(|h| h.profile.custom_name.clone());
            let ssh_alias = ssh_info
                .as_ref()
                .map(|(a, _)| a.clone())
                .or_else(|| prev.and_then(|h| h.profile.ssh_alias.clone()));
            let remote_port = ssh_info
                .as_ref()
                .map(|(_, p)| *p)
                .or_else(|| prev.map(|h| h.profile.remote_daemon_port))
                .unwrap_or(6868);
            let route_mode = prev
                .map(|h| h.profile.route_mode)
                .unwrap_or(HostRouteMode::Auto);
            let custom_direct_endpoint =
                prev.and_then(|h| h.profile.custom_direct_endpoint.clone());

            let profile = SavedHostProfile {
                host_id: offer.host_id.clone(),
                hostname: offer.hostname,
                custom_name,
                ssh_alias,
                route_mode,
                remote_daemon_port: remote_port,
                custom_direct_endpoint,
                daemon_pubkey_b64: offer.daemon_pubkey_b64,
                direct_endpoints: offer.direct_endpoints,
                relay_url: offer.relay_url,
                state: HostConnectionState::Disconnected,
            };
            let entry = guard
                .entry(offer.host_id.clone())
                .or_insert_with(|| HostMemoryState::new(profile.clone()));
            entry.profile = profile;
            (entry.to_card(), entry.to_stored_host())
        };

        self.store.upsert_client_host(&stored)?;
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
        if let Ok(mut projs) = self.projects.write() {
            projs.retain(|_, p| p.host_id != host_id);
        }
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_host_config(
        self: &Arc<Self>,
        host_id: &str,
        custom_name: Option<String>,
        route_mode: Option<HostRouteMode>,
        ssh_alias: Option<String>,
        remote_daemon_port: Option<u16>,
        custom_direct_endpoint: Option<String>,
        raw_relay_url: Option<String>,
        reconnect: bool,
    ) -> Result<HostCardView, ClientCoreError> {
        let (card, stored) = {
            let mut guard = self
                .hosts
                .write()
                .map_err(|_| ClientCoreError::LockPoisoned)?;
            let h = guard.get_mut(host_id).ok_or(ClientCoreError::Unreachable)?;

            if let Some(cn) = custom_name {
                let t = cn.trim();
                h.profile.custom_name = if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                };
            }
            if let Some(rm) = route_mode {
                h.profile.route_mode = rm;
            }
            if let Some(sa) = ssh_alias {
                let t = sa.trim();
                h.profile.ssh_alias = if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                };
            }
            if let Some(port) = remote_daemon_port.filter(|p| *p > 0) {
                h.profile.remote_daemon_port = port;
            }
            if let Some(ep) = custom_direct_endpoint {
                let t = ep.trim();
                h.profile.custom_direct_endpoint = if t.is_empty() {
                    None
                } else if t.starts_with("ws://") || t.starts_with("wss://") {
                    Some(t.to_string())
                } else {
                    Some(format!("ws://{t}"))
                };
            }
            if let Some(ref relay) = raw_relay_url {
                h.profile.relay_url = normalize_relay_input(Some(relay));
            }
            (h.to_card(), h.to_stored_host())
        };

        self.store.upsert_client_host(&stored)?;
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
        if reconnect {
            self.connect_host_async(host_id.to_string());
        }
        Ok(card)
    }

    pub fn update_host_relay(
        self: &Arc<Self>,
        host_id: &str,
        raw_relay_url: Option<&str>,
    ) -> Result<(), ClientCoreError> {
        let _ = self.update_host_config(
            host_id,
            None,
            None,
            None,
            None,
            None,
            Some(raw_relay_url.unwrap_or("").to_string()),
            true,
        )?;
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
            if h.connecting_in_flight {
                return;
            }
            h.connecting_in_flight = true;
            h.profile.state = if matches!(h.profile.route_mode, HostRouteMode::RelayOnly)
                || (h.profile.direct_endpoints.is_empty() && h.profile.ssh_alias.is_none())
            {
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
                let (queued_outbox, resub_timelines) = {
                    let Ok(mut guard) = self.hosts.write() else {
                        return;
                    };
                    let Some(h) = guard.get_mut(host_id) else {
                        return;
                    };
                    h.connecting_in_flight = false;
                    h.reconnect_attempt = 0;
                    h.profile.state = handle.mode;
                    h.via_ssh_tunnel = handle.via_ssh_tunnel;
                    h.last_error = None;
                    h.session = Some(handle.clone());

                    let outbox = std::mem::take(&mut h.pending_outbox);
                    let resubs: Vec<(String, u64)> = h
                        .timelines
                        .iter()
                        .filter(|(aid, _)| !aid.starts_with("opt_"))
                        .map(|(aid, evs)| {
                            let max_seq = evs.iter().map(|e| e.seq).max().unwrap_or(0);
                            (aid.clone(), max_seq)
                        })
                        .collect();
                    (outbox, resubs)
                };
                self.emit(|revision| CoreNotification::FleetChanged { revision });
                self.emit(|revision| CoreNotification::HostChanged {
                    revision,
                    host_id: host_id.to_string(),
                });

                // Initial state refresh + live models + quotas
                let _ = handle
                    .send_wire(WireMessage::DiscoverProjects {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListModels {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
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
                let _ = handle
                    .send_wire(WireMessage::RefreshQuotas {
                        request_id: Uuid::new_v4(),
                    })
                    .await;

                for (agent_id, since_seq) in resub_timelines {
                    let _ = handle
                        .send_wire(WireMessage::SubscribeTimeline {
                            request_id: Uuid::new_v4(),
                            agent_id,
                            since_seq,
                        })
                        .await;
                }

                // Flush any prompts / agent launches queued while reconnecting!
                for queued_msg in queued_outbox {
                    let _ = handle.send_wire(queued_msg).await;
                }

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
                                loop_core.handle_unexpected_disconnect(&owned_host_id);
                                break;
                            }
                        }
                    }
                });
            }
            Err(err) => {
                let mut backoff_ms = None;
                if let Ok(mut guard) = self.hosts.write()
                    && let Some(h) = guard.get_mut(host_id)
                {
                    h.connecting_in_flight = false;
                    h.profile.state = HostConnectionState::Disconnected;
                    h.via_ssh_tunnel = false;
                    h.last_error = Some(err.to_string());
                    h.reconnect_attempt = h.reconnect_attempt.saturating_add(1);
                    backoff_ms = Some(match h.reconnect_attempt {
                        1 => 2000,
                        2 => 4000,
                        3 => 7000,
                        _ => 12000,
                    });
                }
                self.emit(|revision| CoreNotification::FleetChanged { revision });
                self.emit(|revision| CoreNotification::HostChanged {
                    revision,
                    host_id: host_id.to_string(),
                });
                if let Some(delay) = backoff_ms {
                    self.schedule_reconnect(host_id.to_string(), delay);
                }
            }
        }
    }

    fn handle_unexpected_disconnect(self: &Arc<Self>, host_id: &str) {
        let mut should_reconnect = false;
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            // Non-destructive disconnect: keep ALL workspaces, agents & timelines intact!
            h.profile.state = HostConnectionState::Disconnected;
            h.via_ssh_tunnel = false;
            h.session = None;
            should_reconnect = true;
        }
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
        if should_reconnect {
            self.schedule_reconnect(host_id.to_string(), 1500);
        }
    }

    fn schedule_reconnect(self: &Arc<Self>, host_id: String, delay_ms: u64) {
        let core = Arc::clone(self);
        self.runtime.spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            let still_disconnected = core
                .hosts
                .read()
                .ok()
                .and_then(|g| {
                    g.get(&host_id).map(|h| {
                        h.profile.state == HostConnectionState::Disconnected
                            && !h.connecting_in_flight
                    })
                })
                .unwrap_or(false);
            if still_disconnected {
                core.connect_host_task(&host_id).await;
            }
        });
    }

    fn handle_inbound_wire(self: &Arc<Self>, host_id: &str, msg: WireMessage) {
        let mut notify_fleet = false;
        let mut notify_host = false;
        let mut notify_timeline_agent: Option<String> = None;
        let mut notify_git_cwd: Option<String> = None;
        let mut auto_subscribe_agent: Option<String> = None;
        let mut host_to_persist: Option<StoredHostProfile> = None;

        if let Ok(mut guard) = self.hosts.write()
            && let Some(state) = guard.get_mut(host_id)
        {
            match msg {
                WireMessage::Welcome { host } => {
                    state.profile.hostname = host.hostname;
                    state.daemon_version = Some(host.daemon_version);
                    state.os_arch = Some(format!("{} / {}", host.os, host.arch));
                    if !host.available_providers.is_empty() {
                        state.available_providers = host.available_providers;
                    }
                    host_to_persist = Some(state.to_stored_host());
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::Pong { ts_ms } => {
                    let now = unix_ms_now();
                    if now >= ts_ms && (now - ts_ms) < 60_000 {
                        state.last_ping_ms = Some((now - ts_ms) as u32);
                        notify_fleet = true;
                    }
                }
                WireMessage::DiscoveredProjectsSnapshot { projects, .. } => {
                    state.discovered_projects = projects;
                    notify_host = true;
                }
                WireMessage::DirectoryEntriesResponse {
                    current_path,
                    parent_path,
                    entries,
                    ..
                } => {
                    state.directory_listing = Some(DirectoryListingView {
                        current_path,
                        parent_path,
                        entries,
                    });
                    notify_host = true;
                }
                WireMessage::ModelsSnapshot { models, .. } => {
                    for (prov, list) in models {
                        if !list.is_empty() {
                            state.models_by_provider.insert(prov, list);
                        }
                    }
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
                    // Keep any 0ms optimistic `opt_*` sessions while refreshing server-confirmed ones
                    state.agents.retain(|k, _| k.starts_with("opt_"));
                    for a in agents {
                        state.agents.insert(a.agent_id.clone(), a);
                    }
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::AgentCreated { request_id, agent } => {
                    let ag_id = agent.agent_id.clone();
                    if let Some(opt_id) = state.optimistic_by_req_id.remove(&request_id) {
                        state.agents.remove(&opt_id);
                        if let Some(opt_tl) = state.timelines.remove(&opt_id)
                            && !state.timelines.contains_key(&ag_id)
                        {
                            state.timelines.insert(ag_id.clone(), opt_tl);
                        }
                        state
                            .replaced_optimistic_agents
                            .insert(opt_id, ag_id.clone());
                    }
                    state.last_created_agent_id = Some(ag_id.clone());
                    state.agents.insert(ag_id.clone(), agent);
                    auto_subscribe_agent = Some(ag_id.clone());
                    notify_fleet = true;
                    notify_host = true;
                    notify_timeline_agent = Some(ag_id);
                }
                WireMessage::AgentUpdated { agent } => {
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
                    let mut ordered = events;
                    ordered.sort_by_key(|e| e.seq);
                    for ev in ordered {
                        coalesce_or_push_timeline(list, ev);
                    }
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
                WireMessage::RpcError {
                    request_id,
                    code,
                    message,
                } => {
                    state.last_error = Some(format!("[{code}] {message}"));
                    if let Some(req_id) = request_id
                        && let Some(opt_id) = state.optimistic_by_req_id.remove(&req_id)
                    {
                        if let Some(opt_ag) = state.agents.get_mut(&opt_id) {
                            opt_ag.status = AgentStatus::Error {
                                message: message.clone(),
                                recoverable: true,
                            };
                        }
                        state
                            .timelines
                            .entry(opt_id.clone())
                            .or_default()
                            .push(TimelineEvent {
                                seq: 0,
                                epoch: 1,
                                agent_id: opt_id.clone(),
                                turn_id: None,
                                timestamp: Utc::now(),
                                payload: TimelinePayload::Notice {
                                    level: "error".into(),
                                    message,
                                },
                            });
                        notify_timeline_agent = Some(opt_id);
                    }
                    notify_host = true;
                }
                _ => {}
            }
        }

        if let Some(stored) = host_to_persist {
            let _ = self.store.upsert_client_host(&stored);
        }
        if let Some(new_agent_id) = auto_subscribe_agent {
            self.open_agent_timeline(host_id, &new_agent_id);
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

    /// Sends immediately if connected, or queues into `pending_outbox` and triggers background reconnect so user messages are NEVER dropped!
    fn dispatch_wire_queued(self: &Arc<Self>, host_id: &str, msg: WireMessage) {
        let mut live_handle = None;
        let mut needs_reconnect = false;

        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ref handle) = h.session {
                live_handle = Some(handle.clone());
            } else {
                h.pending_outbox.push(msg.clone());
                needs_reconnect = !h.connecting_in_flight;
            }
        }

        if let Some(handle) = live_handle {
            let fallback_core = Arc::clone(self);
            let owned_host = host_id.to_string();
            self.runtime.spawn(async move {
                if handle.send_wire(msg.clone()).await.is_err() {
                    if let Ok(mut guard) = fallback_core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_outbox.push(msg);
                    }
                    fallback_core.connect_host_async(owned_host);
                }
            });
        } else if needs_reconnect {
            self.connect_host_async(host_id.to_string());
        }
    }

    pub fn refresh_host(&self, host_id: &str) {
        self.dispatch_wire(
            host_id,
            WireMessage::DiscoverProjects {
                request_id: Uuid::new_v4(),
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::ListModels {
                request_id: Uuid::new_v4(),
            },
        );
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

    /// Legacy FFI-compatible wrapper (`orbit-ffi-uniffi` & `orbit-ffi-ohos`).
    pub fn create_agent(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
        provider: ProviderKind,
        mode: SessionMode,
        initial_prompt: &str,
        worktree_branch: Option<&str>,
    ) {
        let _ = self.create_agent_ext(
            host_id,
            cwd,
            Vec::new(),
            provider,
            None,
            mode,
            initial_prompt,
            worktree_branch,
        );
    }

    /// Full Paseo-style multi-directory + model-selectable `0ms` optimistic session launcher.
    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_ext(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
        extra_dirs: Vec<String>,
        provider: ProviderKind,
        model: Option<String>,
        mode: SessionMode,
        initial_prompt: &str,
        worktree_branch: Option<&str>,
    ) -> String {
        let clean_branch = worktree_branch.map(str::trim).filter(|b| !b.is_empty());
        let use_auto_worktree = clean_branch == Some("auto");
        if let Some(branch) = clean_branch.filter(|b| *b != "auto") {
            self.dispatch_wire_queued(
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

        let req_id = Uuid::new_v4();
        let opt_id = format!("opt_{req_id}");
        let now = Utc::now();
        let first_line = initial_prompt.lines().next().unwrap_or("").trim();
        let title: String = {
            let chars: Vec<char> = first_line.chars().collect();
            if chars.is_empty() {
                "新工作区会话".to_string()
            } else if chars.len() > 48 {
                format!("{}…", chars[..48].iter().collect::<String>())
            } else {
                first_line.to_string()
            }
        };

        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            let opt_agent = AgentSummary {
                agent_id: opt_id.clone(),
                workspace_id: format!("ws_{opt_id}"),
                cwd: cwd.to_string(),
                title,
                provider,
                model: model.clone(),
                mode: mode.clone(),
                status: AgentStatus::Initializing,
                parent_agent_id: None,
                latest_seq: 0,
                created_at: now,
                updated_at: now,
            };
            h.optimistic_by_req_id.insert(req_id, opt_id.clone());
            h.last_created_agent_id = Some(opt_id.clone());
            h.agents.insert(opt_id.clone(), opt_agent);
            h.timelines.insert(
                opt_id.clone(),
                vec![TimelineEvent {
                    seq: 0,
                    epoch: 1,
                    agent_id: opt_id.clone(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::UserPrompt {
                        message_id: format!("opt_msg_{req_id}"),
                        text: initial_prompt.to_string(),
                    },
                }],
            );
        }

        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
        self.emit(|revision| CoreNotification::TimelineChanged {
            revision,
            host_id: host_id.to_string(),
            agent_id: opt_id.clone(),
        });

        self.dispatch_wire_queued(
            host_id,
            WireMessage::CreateAgent {
                request_id: req_id,
                cwd: cwd.to_string(),
                extra_dirs,
                title: None,
                provider,
                model,
                mode,
                initial_prompt: initial_prompt.to_string(),
                worktree: use_auto_worktree,
            },
        );

        opt_id
    }

    pub fn open_agent_timeline(&self, host_id: &str, agent_id: &str) {
        if agent_id.starts_with("opt_") {
            return;
        }
        let since_seq = self
            .hosts
            .read()
            .ok()
            .and_then(|g| {
                g.get(host_id)
                    .and_then(|h| h.timelines.get(agent_id))
                    .and_then(|list| list.iter().map(|e| e.seq).max())
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

    pub fn send_prompt(self: &Arc<Self>, host_id: &str, agent_id: &str, prompt: &str) {
        let now = Utc::now();
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ag) = h.agents.get_mut(agent_id) {
                ag.status = AgentStatus::Running {
                    turn_id: "pending".into(),
                };
                ag.updated_at = now;
            }
            h.timelines
                .entry(agent_id.to_string())
                .or_default()
                .push(TimelineEvent {
                    seq: 0,
                    epoch: 1,
                    agent_id: agent_id.to_string(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::UserPrompt {
                        message_id: format!("opt_{}", Uuid::new_v4()),
                        text: prompt.to_string(),
                    },
                });
        }

        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
        self.emit(|revision| CoreNotification::TimelineChanged {
            revision,
            host_id: host_id.to_string(),
            agent_id: agent_id.to_string(),
        });

        self.dispatch_wire_queued(
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

    pub fn set_session_mode(self: &Arc<Self>, host_id: &str, agent_id: &str, mode: SessionMode) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            ag.mode = mode.clone();
        }
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::SetSessionMode {
                    request_id: Uuid::new_v4(),
                    agent_id: agent_id.to_string(),
                    mode,
                },
            );
        }
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn set_agent_model(self: &Arc<Self>, host_id: &str, agent_id: &str, model: Option<String>) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            ag.model = model.clone();
        }
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::SetAgentModel {
                    request_id: Uuid::new_v4(),
                    agent_id: agent_id.to_string(),
                    model,
                },
            );
        }
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
        self.emit(|revision| CoreNotification::TimelineChanged {
            revision,
            host_id: host_id.to_string(),
            agent_id: agent_id.to_string(),
        });
    }

    pub fn switch_agent_provider(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        provider: ProviderKind,
    ) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            ag.provider = provider;
            ag.model = None;
        }
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::SwitchAgentProvider {
                    request_id: Uuid::new_v4(),
                    agent_id: agent_id.to_string(),
                    provider,
                },
            );
        }
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn archive_agent(self: &Arc<Self>, host_id: &str, agent_id: &str) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.agents.remove(agent_id);
            h.timelines.remove(agent_id);
        }
        let _ = self.store.delete_replica_agent(host_id, agent_id);
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::ArchiveAgent {
                    request_id: Uuid::new_v4(),
                    agent_id: agent_id.to_string(),
                },
            );
        }
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn resolve_permission(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        permission_request_id: &str,
        decision: &str,
    ) {
        self.dispatch_wire_queued(
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
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        question_request_id: &str,
        answers: Vec<Vec<String>>,
    ) {
        self.dispatch_wire_queued(
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

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn folder_basename(path: &str) -> String {
    let clean = path.trim_end_matches('/');
    if clean.is_empty() || clean == "~" {
        return "主目录".to_string();
    }
    clean
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(clean)
        .to_string()
}

fn coalesce_or_push_timeline(list: &mut Vec<TimelineEvent>, incoming: TimelineEvent) {
    match &incoming.payload {
        TimelinePayload::UserPrompt {
            text: inc_text,
            message_id: inc_mid,
        } => {
            if incoming.seq > 0 {
                // Replace optimistic `seq == 0` UserPrompt with matching text
                if let Some(opt_slot) = list.iter_mut().find(|ev| {
                    ev.seq == 0
                        && matches!(
                            &ev.payload,
                            TimelinePayload::UserPrompt { text, .. } if text.trim() == inc_text.trim()
                        )
                }) {
                    *opt_slot = incoming;
                    return;
                }
            }
            if !inc_mid.is_empty()
                && let Some(dup_slot) = list.iter_mut().find(|ev| {
                    matches!(
                        &ev.payload,
                        TimelinePayload::UserPrompt { message_id, .. } if message_id == inc_mid
                    )
                })
            {
                *dup_slot = incoming;
                return;
            }
        }
        TimelinePayload::AssistantMessage { item_id, .. } if !item_id.is_empty() => {
            if let Some(prev) = list.iter_mut().rev().find(|prev_ev| {
                matches!(
                    &prev_ev.payload,
                    TimelinePayload::AssistantMessage { item_id: existing_id, .. } if existing_id == item_id
                )
            }) {
                *prev = incoming;
                return;
            }
        }
        TimelinePayload::AssistantThinking { item_id, .. } if !item_id.is_empty() => {
            if let Some(prev) = list.iter_mut().rev().find(|prev_ev| {
                matches!(
                    &prev_ev.payload,
                    TimelinePayload::AssistantThinking { item_id: existing_id, .. } if existing_id == item_id
                )
            }) {
                *prev = incoming;
                return;
            }
        }
        TimelinePayload::ToolCall { call_id, .. } if !call_id.is_empty() => {
            if let Some(prev) = list.iter_mut().rev().find(|prev_ev| {
                matches!(
                    &prev_ev.payload,
                    TimelinePayload::ToolCall { call_id: existing_id, .. } if existing_id == call_id
                )
            }) {
                *prev = incoming;
                return;
            }
        }
        _ => {}
    }

    if incoming.seq > 0 {
        if let Some(same_seq) = list.iter_mut().find(|e| e.seq == incoming.seq) {
            *same_seq = incoming;
            return;
        }
        if let Some(first_opt_idx) = list.iter().position(|e| e.seq == 0) {
            list.insert(first_opt_idx, incoming);
            return;
        }
    }
    list.push(incoming);
}

fn agent_to_card_view(a: &AgentSummary) -> AgentCardView {
    let (status_label, is_running, is_awaiting_permission) = match &a.status {
        AgentStatus::Initializing => ("启动中".to_string(), true, false),
        AgentStatus::Idle => ("就绪".to_string(), false, false),
        AgentStatus::Running { .. } => ("思考执行中".to_string(), true, false),
        AgentStatus::AutonomousWakeup { source, .. } => {
            (format!("后台唤醒 ({source})"), true, false)
        }
        AgentStatus::AwaitingPermission { .. } => ("等待确认".to_string(), false, true),
        AgentStatus::Error { .. } => ("异常".to_string(), false, false),
        AgentStatus::Closed => ("已结束".to_string(), false, false),
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
        ToolCallDetail::FileRead { path, .. } => (format!("读取文件 {path}"), Vec::new()),
        ToolCallDetail::FileEdit { path, unified_diff } => {
            let diffs = unified_diff
                .as_deref()
                .map(parse_unified_diff)
                .unwrap_or_default();
            (format!("编辑文件 {path}"), diffs)
        }
        ToolCallDetail::Search { query, .. } => (format!("检索代码 \"{query}\""), Vec::new()),
        ToolCallDetail::Subagent { role, .. } => (format!("子代理协作 · {role}"), Vec::new()),
        ToolCallDetail::Generic { tool_name, .. } => (tool_name.clone(), Vec::new()),
    }
}

fn default_client_models_map() -> HashMap<ProviderKind, Vec<ModelOption>> {
    let mut m = HashMap::new();
    m.insert(
        ProviderKind::Antigravity,
        vec![
            ModelOption {
                id: "gemini-4-carbon-high".into(),
                label: "Gemini 4 Carbon (High)".into(),
                description: Some("最新旗舰深度推理与多步编排模型".into()),
                is_default: true,
            },
            ModelOption {
                id: "gemini-4-carbon-medium".into(),
                label: "Gemini 4 Carbon (Medium)".into(),
                description: Some("速度与深度均衡主力".into()),
                is_default: false,
            },
            ModelOption {
                id: "gemini-4-argon-high".into(),
                label: "Gemini 4 Argon (High)".into(),
                description: Some("高强度复杂代码推理".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-opus-5-5-high".into(),
                label: "Claude Opus 5.5 (High)".into(),
                description: Some("Antigravity 内置 Opus 深度模式".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-sonnet-5-5-high".into(),
                label: "Claude Sonnet 5.5 (High)".into(),
                description: Some("高响应速度编码模式".into()),
                is_default: false,
            },
            ModelOption {
                id: "gemini-3.8-flash-high".into(),
                label: "Gemini 3.8 Flash (High)".into(),
                description: Some("极速轻量任务响应".into()),
                is_default: false,
            },
        ],
    );
    m.insert(
        ProviderKind::Codex,
        vec![
            ModelOption {
                id: "gpt-6.1-sol".into(),
                label: "GPT-6.1-Sol".into(),
                description: Some("最新日常编码与重构主力模型".into()),
                is_default: true,
            },
            ModelOption {
                id: "gpt-6-astra".into(),
                label: "GPT-6-Astra".into(),
                description: Some("高复杂度架构与攻坚模型".into()),
                is_default: false,
            },
            ModelOption {
                id: "gpt-6.1-terra".into(),
                label: "GPT-6.1-Terra".into(),
                description: Some("均衡稳定编码模型".into()),
                is_default: false,
            },
            ModelOption {
                id: "gpt-5.4".into(),
                label: "GPT-5.4".into(),
                description: Some("通用高兼容 Codex 模型".into()),
                is_default: false,
            },
        ],
    );
    m.insert(
        ProviderKind::ClaudeCode,
        vec![
            ModelOption {
                id: "claude-opus-5-5".into(),
                label: "Opus 5.5".into(),
                description: Some("最新旗舰深度架构与推理模型".into()),
                is_default: true,
            },
            ModelOption {
                id: "claude-sonnet-5-5".into(),
                label: "Sonnet 5.5".into(),
                description: Some("高速高质主力编码模型".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-opus-4-6".into(),
                label: "Opus 4.6".into(),
                description: Some("经典稳定复杂工程模型".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-sonnet-4-6".into(),
                label: "Sonnet 4.6".into(),
                description: Some("轻量快速日常开发".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-haiku-5-5".into(),
                label: "Haiku 5.5".into(),
                description: Some("瞬时响应问答与检索".into()),
                is_default: false,
            },
        ],
    );
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_core_persists_paired_host_projects_rename_and_optimistic_outbox() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = PairingOffer {
            host_id: "host_devbox".into(),
            hostname: "ip-172-31-33-74.ap-southeast-1.compute.internal".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "test_secret".into(),
        };
        let uri = offer.to_qr_uri().expect("qr uri");
        let card = core.pair_host(&uri).expect("pair");
        assert_eq!(card.host_id, "host_devbox");

        // Configure SSH alias + rename -> overrides ugly EC2 hostname
        let updated = core
            .update_host_config(
                "host_devbox",
                Some("新加坡 DevBox".into()),
                Some(HostRouteMode::SshTunnel),
                Some("devbox".into()),
                Some(6868),
                None,
                None,
                false,
            )
            .expect("update config");
        assert_eq!(updated.display_name, "新加坡 DevBox");
        assert_eq!(updated.route_mode, HostRouteMode::SshTunnel);

        // Multi-directory Project creation
        let proj = core
            .upsert_project(
                None,
                "host_devbox",
                "mono_repo",
                vec![
                    "/home/ec2-user/go/src/mono_repo".into(),
                    "/home/ec2-user/go/src/shared_proto".into(),
                ],
            )
            .expect("upsert project");
        assert_eq!(proj.directories.len(), 2);

        // Optimistic 0ms create_agent while offline -> creates `opt_*` agent + queues in outbox!
        let opt_id = core.create_agent_ext(
            "host_devbox",
            "/home/ec2-user/go/src/mono_repo",
            vec!["/home/ec2-user/go/src/shared_proto".into()],
            ProviderKind::Antigravity,
            Some("claude-opus-5-5-high".into()),
            SessionMode::AcceptEdits,
            "帮我检查 mono_repo 编译状态",
            None,
        );
        assert!(opt_id.starts_with("opt_"));

        let tl = core.agent_timeline("host_devbox", &opt_id);
        assert_eq!(tl.items.len(), 1);
        assert!(tl.agent.expect("opt agent").is_running);

        let overview = core.fleet_overview();
        assert_eq!(overview.hosts.len(), 1);
        assert_eq!(overview.projects.len(), 1);
    }
}
