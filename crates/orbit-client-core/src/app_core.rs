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

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Utc;
use orbit_crypto::PairingOffer;
use orbit_protocol::{
    AgentStatus, AgentSummary, DirectoryEntry, DiscoveredProject, GitStatusSnapshot, ModelOption,
    PromptBadge, ProviderKind, ProviderModelCatalog, ProviderQuotaSnapshot, QuestionItem,
    ReasoningEffort, SessionMode, TerminalBinaryFrame, TerminalSummary, TimelineEvent,
    TimelinePayload, ToolCallDetail, ToolCallStatus, WireMessage, WorkspaceSummary,
};
use orbit_store::{OrbitStore, StoredClientProject, StoredHostMeta, StoredHostProfile};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::desktop_ops::{
    StagedAttachmentInfo, bootstrap_ssh_host_and_pair, discover_local_daemon_pairing_uri,
    extract_ws_port, normalize_relay_input, read_local_workspace_file, stage_local_attachment,
};
use crate::render::{
    DiffFileView, HighlightedLineView, RichBlockView, TerminalGridEmulator, TerminalScreenView,
    detect_language_for_path, highlight_code_lines_capped, parse_markdown_blocks,
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
pub struct StagedAttachmentView {
    pub path: String,
    pub filename: String,
    pub size_bytes: u64,
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
    pub effort: Option<ReasoningEffort>,
    pub mode: SessionMode,
    pub status: AgentStatus,
    pub status_label: String,
    pub is_running: bool,
    pub is_awaiting_permission: bool,
    pub parent_agent_id: Option<String>,
    pub latest_seq: u64,
    pub updated_at_rfc3339: String,
}

pub struct CreateAgentRequest<'a> {
    pub host_id: &'a str,
    pub cwd: &'a str,
    pub extra_dirs: Vec<String>,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    pub mode: SessionMode,
    pub initial_prompt: &'a str,
    pub initial_display_prompt: Option<String>,
    pub initial_badges: Vec<PromptBadge>,
    pub worktree_branch: Option<&'a str>,
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
    #[serde(default)]
    pub provider_catalogs: Vec<ProviderModelCatalog>,
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
        #[serde(default)]
        badges: Vec<PromptBadge>,
        blocks: Vec<RichBlockView>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    AssistantThinking {
        seq: u64,
        item_id: String,
        text: String,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    AssistantMessage {
        seq: u64,
        item_id: String,
        text: String,
        streaming: bool,
        blocks: Vec<RichBlockView>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    ToolCall {
        seq: u64,
        call_id: String,
        status: ToolCallStatus,
        summary_title: String,
        detail: ToolCallDetail,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_descriptor: Option<crate::stage::ToolStepDescriptorView>,
        diff_files: Vec<DiffFileView>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    PermissionRequest {
        seq: u64,
        request_id: String,
        title: String,
        detail: ToolCallDetail,
        allowed_decisions: Vec<String>,
        resolved_decision: Option<String>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    QuestionRequest {
        seq: u64,
        request_id: String,
        questions: Vec<QuestionItem>,
        resolved_answers: Option<Vec<Vec<String>>>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    Notice {
        seq: u64,
        level: String,
        message: String,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTimelineView {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<AgentCardView>,
    pub items: Vec<TimelineItemView>,
}

/// Incremental `O(delta)` timeline projection so streaming tokens never re-serialize historical blocks over IPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTimelineDeltaView {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<AgentCardView>,
    pub latest_seq: u64,
    pub upserted_items: Vec<TimelineItemView>,
    pub resolved_permissions: HashMap<String, String>,
    pub resolved_questions: HashMap<String, Vec<Vec<String>>>,
    /// Set to `true` after an authoritative server-side rewind so clients replace their local list.
    #[serde(default)]
    pub reset: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindAgentResultView {
    pub agent: AgentCardView,
    pub restored_files: u32,
    pub restored_prompt: Option<String>,
    pub notice: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceFileView {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub resolved_path: String,
    pub language: String,
    /// `"text"` | `"image"` | `"binary"`
    pub content_kind: String,
    pub mime_type: Option<String>,
    pub content: String,
    pub size_bytes: u64,
    pub truncated: bool,
    pub highlighted_lines: Vec<HighlightedLineView>,
    pub markdown_blocks: Vec<RichBlockView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SideQuestionStreamEvent {
    Delta {
        request_id: String,
        text_delta: String,
    },
    Done {
        request_id: String,
        full_text: String,
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct AskSideQuestionRequest {
    pub host_id: String,
    pub agent_id: Option<String>,
    pub cwd: String,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub question: String,
    pub context_summary: String,
}

#[derive(Debug, Clone)]
struct RawFileReadResponse {
    resolved_path: String,
    content_kind: String,
    mime_type: Option<String>,
    content: String,
    size_bytes: u64,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct AttachmentStageAck {
    completed: bool,
    path: Option<String>,
    filename: Option<String>,
    size_bytes: Option<u64>,
}

struct HostMemoryState {
    profile: SavedHostProfile,
    is_local: bool,
    last_error: Option<String>,
    via_ssh_tunnel: bool,
    last_ping_ms: Option<u32>,
    daemon_version: Option<String>,
    daemon_protocol_version: Option<u32>,
    os_arch: Option<String>,
    reconnect_attempt: u32,
    ssh_heal_attempted: bool,
    connecting_in_flight: bool,
    available_providers: Vec<ProviderKind>,
    session: Option<HostSessionHandle>,
    pending_outbox: Vec<WireMessage>,
    pending_attachment_acks:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<AttachmentStageAck, String>>>,
    pending_file_reads:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<RawFileReadResponse, String>>>,
    pending_side_questions:
        HashMap<Uuid, tokio::sync::mpsc::UnboundedSender<SideQuestionStreamEvent>>,
    pending_rewinds:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<RewindAgentResultView, String>>>,
    rewound_agents: std::collections::HashSet<String>,
    optimistic_by_req_id: HashMap<Uuid, String>,
    agent_req_by_req_id: HashMap<Uuid, String>,
    replaced_optimistic_agents: HashMap<String, String>,
    last_created_agent_id: Option<String>,
    discovered_projects: Vec<DiscoveredProject>,
    directory_listing: Option<DirectoryListingView>,
    models_by_provider: HashMap<ProviderKind, Vec<ModelOption>>,
    workspaces: Vec<WorkspaceSummary>,
    agents: HashMap<String, AgentSummary>,
    timelines: HashMap<String, Vec<TimelineEvent>>,
    markdown_cache: HashMap<(String, u64, usize, bool), Vec<RichBlockView>>,
    quotas: Vec<ProviderQuotaSnapshot>,
    provider_catalogs: Vec<ProviderModelCatalog>,
    terminals: Vec<TerminalSummary>,
    active_terminal_id: Option<String>,
    active_terminal_slot: u8,
    terminal_emulator: TerminalGridEmulator,
    git_statuses: HashMap<String, GitStatusSnapshot>,
    git_diffs: HashMap<String, String>,
    workspace_files: HashMap<String, Vec<String>>,
}

impl HostMemoryState {
    fn new(profile: SavedHostProfile) -> Self {
        Self {
            profile,
            is_local: false,
            last_error: None,
            via_ssh_tunnel: false,
            last_ping_ms: None,
            daemon_version: None,
            daemon_protocol_version: None,
            os_arch: None,
            reconnect_attempt: 0,
            ssh_heal_attempted: false,
            connecting_in_flight: false,
            available_providers: vec![
                ProviderKind::Antigravity,
                ProviderKind::Codex,
                ProviderKind::ClaudeCode,
            ],
            session: None,
            pending_outbox: Vec::new(),
            pending_attachment_acks: HashMap::new(),
            pending_file_reads: HashMap::new(),
            pending_side_questions: HashMap::new(),
            pending_rewinds: HashMap::new(),
            rewound_agents: std::collections::HashSet::new(),
            optimistic_by_req_id: HashMap::new(),
            agent_req_by_req_id: HashMap::new(),
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
            provider_catalogs: Vec::new(),
            terminals: Vec::new(),
            active_terminal_id: None,
            active_terminal_slot: 1,
            terminal_emulator: TerminalGridEmulator::new(100, 30),
            git_statuses: HashMap::new(),
            git_diffs: HashMap::new(),
            workspace_files: HashMap::new(),
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
        {
            let local_port = extract_ws_port(&offer.direct_endpoints).unwrap_or(6868);
            let mut endpoints = offer.direct_endpoints;
            let loopback = format!("ws://127.0.0.1:{local_port}/ws");
            if !endpoints.contains(&loopback) {
                endpoints.insert(0, loopback);
            }
            if let Some(existing) = initial_hosts.get_mut(&offer.host_id) {
                if existing.profile.daemon_pubkey_b64 == offer.daemon_pubkey_b64 {
                    existing.is_local = true;
                    existing.profile.direct_endpoints = endpoints;
                    existing.profile.remote_daemon_port = local_port;
                    if offer.relay_url.is_some() {
                        existing.profile.relay_url = offer.relay_url;
                    }
                    let _ = store.upsert_client_host(&existing.to_stored_host());
                }
            } else {
                let mut mem = HostMemoryState::new(SavedHostProfile {
                    host_id: offer.host_id.clone(),
                    hostname: offer.hostname,
                    custom_name: None,
                    ssh_alias: None,
                    route_mode: HostRouteMode::Auto,
                    remote_daemon_port: local_port,
                    custom_direct_endpoint: None,
                    daemon_pubkey_b64: offer.daemon_pubkey_b64,
                    direct_endpoints: endpoints,
                    relay_url: offer.relay_url,
                    state: HostConnectionState::Disconnected,
                });
                mem.is_local = true;
                let _ = store.upsert_client_host(&mem.to_stored_host());
                initial_hosts.insert(offer.host_id, mem);
            }
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

    pub fn subscribe_coalesced(&self, min_interval_ms: u64, sub: Arc<dyn CoreEventSubscriber>) {
        if min_interval_ms == 0 {
            self.subscribe(sub);
            return;
        }
        let coalesced = Arc::new(CoalescingSubscriber::new(
            sub,
            self.runtime.handle().clone(),
            Duration::from_millis(min_interval_ms),
        ));
        self.subscribe(coalesced);
    }

    pub fn discover_local_daemon_uri(&self) -> Option<String> {
        discover_local_daemon_pairing_uri()
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
            provider_catalogs: crate::catalog::effective_provider_catalogs(
                &state.provider_catalogs,
            ),
            terminals: state.terminals.clone(),
            active_terminal_id: state.active_terminal_id.clone(),
            active_terminal_slot: state.active_terminal_slot,
        })
    }

    pub fn agent_stage(&self, host_id: &str, agent_id: &str) -> crate::stage::AgentStageView {
        let tl = self.agent_timeline(host_id, agent_id);
        let cwd = tl.agent.as_ref().map(|a| a.cwd.as_str());
        let latest_seq = tl
            .items
            .iter()
            .map(crate::stage::timeline_item_seq)
            .max()
            .unwrap_or_else(|| tl.agent.as_ref().map(|a| a.latest_seq).unwrap_or(0));
        let (turns, pending_permissions, pending_questions, session_turn_diffs) =
            crate::stage::process_timeline_for_stage(&tl.items, cwd);
        crate::stage::AgentStageView {
            revision: tl.revision,
            host_id: tl.host_id,
            agent: tl.agent,
            turns,
            pending_permissions,
            pending_questions,
            session_turn_diffs,
            latest_seq,
        }
    }

    pub fn agent_timeline(&self, host_id: &str, agent_id: &str) -> AgentTimelineView {
        let delta = self.agent_timeline_delta(host_id, agent_id, 0);
        AgentTimelineView {
            revision: delta.revision,
            host_id: delta.host_id,
            agent: delta.agent,
            items: delta.upserted_items,
        }
    }

    pub fn agent_timeline_delta(
        &self,
        host_id: &str,
        agent_id: &str,
        since_seq: u64,
    ) -> AgentTimelineDeltaView {
        let rev = self.revision.load(Ordering::SeqCst);
        let Ok(mut guard) = self.hosts.write() else {
            return AgentTimelineDeltaView {
                revision: rev,
                host_id: host_id.to_string(),
                agent: None,
                latest_seq: since_seq,
                upserted_items: Vec::new(),
                resolved_permissions: HashMap::new(),
                resolved_questions: HashMap::new(),
                reset: false,
            };
        };
        let Some(host_state) = guard.get_mut(host_id) else {
            return AgentTimelineDeltaView {
                revision: rev,
                host_id: host_id.to_string(),
                agent: None,
                latest_seq: since_seq,
                upserted_items: Vec::new(),
                resolved_permissions: HashMap::new(),
                resolved_questions: HashMap::new(),
                reset: false,
            };
        };

        // Hydrate from local SQLite replica on first open for 0ms offline-first rendering
        if !host_state.timelines.contains_key(agent_id)
            && !agent_id.starts_with("opt_")
            && let Ok(cached) = self.store.fetch_replica_timeline(agent_id, 500)
            && !cached.is_empty()
        {
            let mut coalesced = Vec::with_capacity(cached.len());
            for ev in cached {
                coalesce_or_push_timeline(&mut coalesced, ev);
            }
            host_state.timelines.insert(agent_id.to_string(), coalesced);
        }

        let agent_card = host_state.agents.get(agent_id).map(agent_to_card_view);
        let agent_cwd = agent_card.as_ref().map(|a| a.cwd.clone());
        let events = host_state
            .timelines
            .get(agent_id)
            .cloned()
            .unwrap_or_default();

        let actual_max_seq = events.iter().map(|e| e.seq).max().unwrap_or(0);
        let was_rewound = host_state.rewound_agents.remove(agent_id);
        let needs_reset = was_rewound || (since_seq > 0 && actual_max_seq < since_seq);
        let effective_since_seq = if needs_reset { 0 } else { since_seq };

        let mut all_resolved_perms: HashMap<String, String> = HashMap::new();
        let mut all_resolved_questions: HashMap<String, Vec<Vec<String>>> = HashMap::new();
        let mut delta_resolved_perms: HashMap<String, String> = HashMap::new();
        let mut delta_resolved_questions: HashMap<String, Vec<Vec<String>>> = HashMap::new();
        let mut latest_seq = if needs_reset {
            actual_max_seq
        } else {
            since_seq
        };
        for ev in &events {
            if ev.seq > latest_seq {
                latest_seq = ev.seq;
            }
            match &ev.payload {
                TimelinePayload::PermissionResolved {
                    request_id,
                    decision,
                } => {
                    all_resolved_perms.insert(request_id.clone(), decision.clone());
                    if effective_since_seq == 0 || ev.seq > effective_since_seq {
                        delta_resolved_perms.insert(request_id.clone(), decision.clone());
                    }
                }
                TimelinePayload::QuestionResolved {
                    request_id,
                    answers,
                } => {
                    all_resolved_questions.insert(request_id.clone(), answers.clone());
                    if effective_since_seq == 0 || ev.seq > effective_since_seq {
                        delta_resolved_questions.insert(request_id.clone(), answers.clone());
                    }
                }
                _ => {}
            }
        }

        let mut upserted_items = Vec::new();
        for ev in &events {
            let is_local_notice =
                ev.seq == 0 && matches!(ev.payload, TimelinePayload::Notice { .. });
            if effective_since_seq > 0 && ev.seq <= effective_since_seq && !is_local_notice {
                continue;
            }
            let ts_ms = Some(ev.timestamp.timestamp_millis());
            match &ev.payload {
                TimelinePayload::UserPrompt {
                    message_id,
                    text,
                    badges,
                } => {
                    let key = (agent_id.to_string(), ev.seq, text.len(), false);
                    if host_state.markdown_cache.len() > 2048 {
                        host_state.markdown_cache.clear();
                    }
                    let blocks = host_state
                        .markdown_cache
                        .entry(key)
                        .or_insert_with(|| parse_markdown_blocks(text, false))
                        .clone();
                    upserted_items.push(TimelineItemView::UserPrompt {
                        seq: ev.seq,
                        message_id: message_id.clone(),
                        text: text.clone(),
                        badges: badges.clone(),
                        blocks,
                        timestamp_ms: ts_ms,
                    });
                }
                TimelinePayload::AssistantThinking { item_id, text } => {
                    upserted_items.push(TimelineItemView::AssistantThinking {
                        seq: ev.seq,
                        item_id: item_id.clone(),
                        text: text.trim().to_string(),
                        timestamp_ms: ts_ms,
                    });
                }
                TimelinePayload::AssistantMessage {
                    item_id,
                    text,
                    streaming,
                } => {
                    let key = (agent_id.to_string(), ev.seq, text.len(), *streaming);
                    if host_state.markdown_cache.len() > 2048 {
                        host_state.markdown_cache.clear();
                    }
                    let blocks = host_state
                        .markdown_cache
                        .entry(key)
                        .or_insert_with(|| parse_markdown_blocks(text, *streaming))
                        .clone();
                    upserted_items.push(TimelineItemView::AssistantMessage {
                        seq: ev.seq,
                        item_id: item_id.clone(),
                        text: text.clone(),
                        streaming: *streaming,
                        blocks,
                        timestamp_ms: ts_ms,
                    });
                }
                TimelinePayload::ToolCall {
                    call_id,
                    status,
                    detail,
                } => {
                    let (summary_title, diff_files) = summarize_tool_detail(detail);
                    let step_descriptor = Some(crate::stage::describe_tool_detail(
                        *status,
                        &summary_title,
                        detail,
                        &diff_files,
                        agent_cwd.as_deref(),
                    ));
                    upserted_items.push(TimelineItemView::ToolCall {
                        seq: ev.seq,
                        call_id: call_id.clone(),
                        status: *status,
                        summary_title,
                        detail: detail.clone(),
                        step_descriptor,
                        diff_files,
                        timestamp_ms: ts_ms,
                    });
                }
                TimelinePayload::PermissionRequest {
                    request_id,
                    title,
                    detail,
                    allowed_decisions,
                } => {
                    upserted_items.push(TimelineItemView::PermissionRequest {
                        seq: ev.seq,
                        request_id: request_id.clone(),
                        title: title.clone(),
                        detail: detail.clone(),
                        allowed_decisions: allowed_decisions.clone(),
                        resolved_decision: all_resolved_perms.get(request_id).cloned(),
                        timestamp_ms: ts_ms,
                    });
                }
                TimelinePayload::QuestionRequest {
                    request_id,
                    questions,
                } => {
                    upserted_items.push(TimelineItemView::QuestionRequest {
                        seq: ev.seq,
                        request_id: request_id.clone(),
                        questions: questions.clone(),
                        resolved_answers: all_resolved_questions.get(request_id).cloned(),
                        timestamp_ms: ts_ms,
                    });
                }
                TimelinePayload::Notice { level, message } => {
                    upserted_items.push(TimelineItemView::Notice {
                        seq: ev.seq,
                        level: level.clone(),
                        message: message.clone(),
                        timestamp_ms: ts_ms,
                    });
                }
                TimelinePayload::StatusChanged {
                    status: AgentStatus::Error { message, .. },
                } => {
                    if !message.trim().is_empty() {
                        upserted_items.push(TimelineItemView::Notice {
                            seq: ev.seq,
                            level: "error".into(),
                            message: message.clone(),
                            timestamp_ms: ts_ms,
                        });
                    }
                }
                TimelinePayload::PermissionResolved { .. }
                | TimelinePayload::QuestionResolved { .. }
                | TimelinePayload::StatusChanged { .. } => {}
            }
        }

        AgentTimelineDeltaView {
            revision: rev,
            host_id: host_id.to_string(),
            agent: agent_card,
            latest_seq,
            upserted_items,
            resolved_permissions: delta_resolved_perms,
            resolved_questions: delta_resolved_questions,
            reset: needs_reset,
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

        let path_suffix = format!("::{}", path.unwrap_or("__all__"));
        let exact_key = format!("{cwd}{path_suffix}");
        let raw_diff = host_state
            .git_diffs
            .get(&exact_key)
            .or_else(|| {
                host_state.git_diffs.iter().find_map(|(k, v)| {
                    let stored_cwd = k.strip_suffix(&path_suffix)?;
                    cwd_paths_equivalent(stored_cwd, cwd).then_some(v)
                })
            })
            .cloned()
            .unwrap_or_default();
        let status = host_state
            .git_statuses
            .get(cwd)
            .or_else(|| {
                host_state
                    .git_statuses
                    .iter()
                    .find_map(|(k, v)| cwd_paths_equivalent(k, cwd).then_some(v))
            })
            .cloned();
        let diff_files = parse_unified_diff(&raw_diff);
        let capped_raw_diff = truncate_utf8_prefix(&raw_diff, 32_768);
        GitReviewView {
            revision: rev,
            host_id: host_id.to_string(),
            cwd: cwd.to_string(),
            status,
            selected_path: path.map(ToOwned::to_owned),
            raw_diff: capped_raw_diff,
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
        remote_port: Option<u16>,
    ) -> Result<HostCardView, ClientCoreError> {
        let uri =
            bootstrap_ssh_host_and_pair(alias, remote_port).map_err(ClientCoreError::Handshake)?;
        self.pair_host_internal(&uri, Some((alias.to_string(), remote_port)))
    }

    fn pair_host_internal(
        self: &Arc<Self>,
        pairing_uri: &str,
        ssh_info: Option<(String, Option<u16>)>,
    ) -> Result<HostCardView, ClientCoreError> {
        let offer = PairingOffer::from_qr_uri(pairing_uri)?;
        let detected_port = extract_ws_port(&offer.direct_endpoints);
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
                .and_then(|(_, p)| *p)
                .or(detected_port)
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
            entry.ssh_heal_attempted = false;
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
            if let Some(mut removed) = guard.remove(host_id) {
                fail_pending_host_requests(&mut removed, "Host removed before request completed");
            }
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
            h.ssh_heal_attempted = false;
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
            if h.pending_outbox.is_empty() {
                h.agent_req_by_req_id.clear();
            }
            fail_pending_host_requests(h, "Host reconnecting before request completed");
            h.profile.clone()
        };
        self.emit(|revision| CoreNotification::FleetChanged { revision });

        match connect_smart(&profile, Some(self.store.clone())).await {
            Ok(handle) => {
                let mut rx = handle.subscribe();
                let welcome_host = handle.welcome_host.clone();
                let (queued_outbox, resub_timelines) = {
                    let Ok(mut guard) = self.hosts.write() else {
                        return;
                    };
                    let Some(h) = guard.get_mut(host_id) else {
                        return;
                    };
                    h.connecting_in_flight = false;
                    h.reconnect_attempt = 0;
                    h.ssh_heal_attempted = false;
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
                if let Some(host_info) = welcome_host {
                    self.handle_inbound_wire(host_id, WireMessage::Welcome { host: host_info });
                } else {
                    self.emit(|revision| CoreNotification::FleetChanged { revision });
                    self.emit(|revision| CoreNotification::HostChanged {
                        revision,
                        host_id: host_id.to_string(),
                    });
                }

                // Fast SQLite state (`< 2ms`) FIRST so sidebar sessions appear immediately,
                // followed by filesystem project discovery & background quota probes!
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
                    .send_wire(WireMessage::ListModels {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListTerminals {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListProviderModels {
                        request_id: Uuid::new_v4(),
                        force_refresh: false,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::DiscoverProjects {
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
                let is_identity_mismatch =
                    matches!(err, ClientCoreError::HostIdentityMismatch { .. });
                if is_identity_mismatch {
                    let _ = self.store.clear_host_replica(host_id);
                }

                let mut ssh_self_heal_alias: Option<String> = None;
                let mut backoff_ms = None;
                if let Ok(mut guard) = self.hosts.write()
                    && let Some(h) = guard.get_mut(host_id)
                {
                    h.connecting_in_flight = false;
                    h.profile.state = HostConnectionState::Disconnected;
                    h.via_ssh_tunnel = false;
                    h.last_error = Some(err.to_string());
                    h.reconnect_attempt = h.reconnect_attempt.saturating_add(1);

                    if is_identity_mismatch {
                        h.workspaces.clear();
                        h.agents.clear();
                        h.discovered_projects.clear();
                        h.timelines.clear();
                    }

                    if !h.ssh_heal_attempted
                        && let Some(ref alias) = h.profile.ssh_alias
                        && !alias.trim().is_empty()
                        && (is_identity_mismatch || h.reconnect_attempt == 1)
                    {
                        h.ssh_heal_attempted = true;
                        ssh_self_heal_alias = Some(alias.trim().to_string());
                    } else {
                        backoff_ms = Some(match h.reconnect_attempt {
                            1 => 2000,
                            2 => 4000,
                            3 => 7000,
                            _ => 12000,
                        });
                    }
                }
                self.emit(|revision| CoreNotification::FleetChanged { revision });
                self.emit(|revision| CoreNotification::HostChanged {
                    revision,
                    host_id: host_id.to_string(),
                });

                // Zero-touch Multi-User SSH Self-Healing:
                // Automatically upgrades remote CLI/Daemon, isolates onto a conflict-free per-user port,
                // updates local port & endpoint metadata, and reconnects immediately!
                if let Some(alias) = ssh_self_heal_alias {
                    let heal_core = Arc::clone(self);
                    let owned_id = host_id.to_string();
                    tokio::spawn(async move {
                        let healed = tokio::task::spawn_blocking(move || {
                            bootstrap_ssh_host_and_pair(&alias, None)
                        })
                        .await
                        .ok()
                        .and_then(Result::ok);

                        if let Some(uri) = healed
                            && let Ok(offer) = PairingOffer::from_qr_uri(&uri)
                            && offer.host_id == owned_id
                        {
                            let new_port = extract_ws_port(&offer.direct_endpoints).unwrap_or(6868);
                            let mut maybe_stored = None;
                            if let Ok(mut guard) = heal_core.hosts.write()
                                && let Some(h) = guard.get_mut(&owned_id)
                            {
                                h.profile.remote_daemon_port = new_port;
                                h.profile.direct_endpoints = offer.direct_endpoints;
                                h.profile.daemon_pubkey_b64 = offer.daemon_pubkey_b64;
                                maybe_stored = Some(h.to_stored_host());
                            }
                            if let Some(stored) = maybe_stored {
                                let _ = heal_core.store.upsert_client_host(&stored);
                            }
                            heal_core.connect_host_async(owned_id);
                        } else {
                            heal_core.schedule_reconnect(owned_id, 3000);
                        }
                    });
                } else if let Some(delay) = backoff_ms {
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
            if h.pending_outbox.is_empty() {
                h.agent_req_by_req_id.clear();
            }
            fail_pending_host_requests(h, "Host disconnected before request completed");
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
        let mut continue_catchup: Option<(String, u64)> = None;
        let mut host_to_persist: Option<StoredHostProfile> = None;
        let mut rewound_to_persist: Option<(AgentSummary, Vec<TimelineEvent>, bool)> = None;
        let mut stale_replica_agents: Vec<(String, bool)> = Vec::new();

        if let Ok(mut guard) = self.hosts.write()
            && let Some(state) = guard.get_mut(host_id)
        {
            match msg {
                WireMessage::Welcome { host } => {
                    state.profile.hostname = host.hostname;
                    state.daemon_version = Some(host.daemon_version);
                    state.daemon_protocol_version = Some(host.protocol_version);
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
                    let prev_agents: HashMap<String, (u64, u64)> = state
                        .agents
                        .iter()
                        .map(|(k, v)| (k.clone(), (v.timeline_epoch, v.latest_seq)))
                        .collect();
                    state.agents.retain(|k, _| k.starts_with("opt_"));
                    for a in agents {
                        let ag_id = a.agent_id.clone();
                        let prev = prev_agents.get(&ag_id).copied();
                        let local_max_seq = state
                            .timelines
                            .get(&ag_id)
                            .and_then(|evs| evs.iter().map(|e| e.seq).max())
                            .unwrap_or(0);
                        let epoch_advanced = prev.is_some_and(|(ep, _)| a.timeline_epoch > ep);
                        let seq_regressed = local_max_seq > 0 && a.latest_seq < local_max_seq;
                        if epoch_advanced || seq_regressed {
                            let had_mem = state.timelines.remove(&ag_id).is_some();
                            state.rewound_agents.insert(ag_id.clone());
                            stale_replica_agents.push((ag_id.clone(), had_mem));
                            notify_timeline_agent = Some(ag_id.clone());
                        }
                        state.agents.insert(ag_id, a);
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
                    let prev_epoch = state.agents.get(&ag_id).map(|a| a.timeline_epoch);
                    let local_max_seq = state
                        .timelines
                        .get(&ag_id)
                        .and_then(|evs| evs.iter().map(|e| e.seq).max())
                        .unwrap_or(0);
                    let epoch_advanced = prev_epoch.is_some_and(|ep| agent.timeline_epoch > ep);
                    let seq_regressed = local_max_seq > 0 && agent.latest_seq < local_max_seq;
                    if epoch_advanced || seq_regressed {
                        let had_mem = state.timelines.remove(&ag_id).is_some();
                        state.rewound_agents.insert(ag_id.clone());
                        stale_replica_agents.push((ag_id.clone(), had_mem));
                    }
                    state.agents.insert(ag_id.clone(), agent);
                    notify_fleet = true;
                    notify_host = true;
                    notify_timeline_agent = Some(ag_id);
                }
                WireMessage::AgentRewound {
                    request_id,
                    agent,
                    events,
                    restored_files,
                    restored_prompt,
                    notice,
                    ..
                } => {
                    let ag_id = agent.agent_id.clone();
                    let agent_card = agent_to_card_view(&agent);
                    let mut ordered = events;
                    ordered.sort_by_key(|e| e.seq);
                    state.timelines.insert(ag_id.clone(), ordered.clone());
                    state.rewound_agents.insert(ag_id.clone());
                    state.agents.insert(ag_id.clone(), agent.clone());
                    rewound_to_persist = Some((agent, ordered, restored_files > 0));
                    if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_rewinds.remove(&req_id)
                    {
                        let _ = tx.send(Ok(RewindAgentResultView {
                            agent: agent_card,
                            restored_files,
                            restored_prompt,
                            notice,
                        }));
                    }
                    notify_fleet = true;
                    notify_host = true;
                    notify_timeline_agent = Some(ag_id);
                }
                WireMessage::TimelineCatchup {
                    agent_id,
                    events,
                    has_more,
                    ..
                } => {
                    let list = state.timelines.entry(agent_id.clone()).or_default();
                    let mut ordered = events;
                    ordered.sort_by_key(|e| e.seq);
                    let max_seq = ordered.last().map(|e| e.seq);
                    for ev in ordered {
                        coalesce_or_push_timeline(list, ev);
                    }
                    if has_more && let Some(next_since) = max_seq {
                        continue_catchup = Some((agent_id.clone(), next_since));
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
                WireMessage::WorkspaceFilesResponse { cwd, files, .. } => {
                    state.workspace_files.insert(cwd.clone(), files);
                    notify_git_cwd = Some(cwd);
                }
                WireMessage::WorkspaceFileContentResponse {
                    request_id,
                    resolved_path,
                    content_kind,
                    mime_type,
                    content,
                    size_bytes,
                    truncated,
                    ..
                } => {
                    if let Some(tx) = state.pending_file_reads.remove(&request_id) {
                        let _ = tx.send(Ok(RawFileReadResponse {
                            resolved_path,
                            content_kind,
                            mime_type,
                            content,
                            size_bytes,
                            truncated,
                        }));
                    }
                }
                WireMessage::SideQuestionDelta {
                    request_id,
                    text_delta,
                } => {
                    if let Some(tx) = state.pending_side_questions.get(&request_id) {
                        let _ = tx.send(SideQuestionStreamEvent::Delta {
                            request_id: request_id.to_string(),
                            text_delta,
                        });
                    }
                }
                WireMessage::SideQuestionDone {
                    request_id,
                    full_text,
                    error,
                } => {
                    if let Some(tx) = state.pending_side_questions.remove(&request_id) {
                        let _ = tx.send(SideQuestionStreamEvent::Done {
                            request_id: request_id.to_string(),
                            full_text,
                            error,
                        });
                    }
                }
                WireMessage::TerminalsSnapshot { terminals, .. } => {
                    state.terminals = terminals;
                    notify_host = true;
                }
                WireMessage::TerminalAttached { terminal, slot, .. } => {
                    state.active_terminal_id = Some(terminal.terminal_id.clone());
                    state.active_terminal_slot = slot;
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
                WireMessage::ProviderModelsSnapshot { catalogs, .. } => {
                    state.provider_catalogs = catalogs;
                    notify_host = true;
                }
                WireMessage::Ack { request_id } => {
                    state.agent_req_by_req_id.remove(&request_id);
                }
                WireMessage::AttachmentStaged {
                    request_id,
                    completed,
                    path,
                    filename,
                    size_bytes,
                    ..
                } => {
                    if let Some(tx) = state.pending_attachment_acks.remove(&request_id) {
                        let _ = tx.send(Ok(AttachmentStageAck {
                            completed,
                            path,
                            filename,
                            size_bytes,
                        }));
                    }
                }
                WireMessage::RpcError {
                    request_id,
                    code,
                    message,
                } => {
                    let formatted_err = format!("[{code}] {message}");
                    if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_attachment_acks.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_file_reads.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_rewinds.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_side_questions.remove(&req_id)
                    {
                        let _ = tx.send(SideQuestionStreamEvent::Done {
                            request_id: req_id.to_string(),
                            full_text: String::new(),
                            error: Some(formatted_err),
                        });
                    } else {
                        state.last_error = Some(formatted_err.clone());
                        if let Some(req_id) = request_id {
                            if let Some(opt_id) = state.optimistic_by_req_id.remove(&req_id) {
                                record_agent_rpc_error(
                                    state,
                                    &opt_id,
                                    &message,
                                    message.clone(),
                                    true,
                                );
                                notify_fleet = true;
                                notify_timeline_agent = Some(opt_id);
                            } else if let Some(agent_id) = state.agent_req_by_req_id.remove(&req_id)
                            {
                                let is_pending_turn =
                                    state.agents.get(&agent_id).is_some_and(|ag| {
                                        matches!(
                                            &ag.status,
                                            AgentStatus::Running { turn_id } if turn_id == "pending"
                                        )
                                    });
                                let reset_status = is_pending_turn || code == "send_prompt_failed";
                                record_agent_rpc_error(
                                    state,
                                    &agent_id,
                                    &message,
                                    formatted_err,
                                    reset_status,
                                );
                                if reset_status {
                                    notify_fleet = true;
                                }
                                notify_timeline_agent = Some(agent_id);
                            }
                        }
                        notify_host = true;
                    }
                }
                _ => {}
            }
        }

        if let Some(stored) = host_to_persist {
            let _ = self.store.upsert_client_host(&stored);
        }
        if let Some((ag, evs, files_restored)) = rewound_to_persist {
            let _ = self.store.replace_replica_timeline(&ag.agent_id, &evs);
            let _ = self.store.upsert_single_replica_agent(host_id, &ag);
            if files_restored {
                self.fetch_git_status(host_id, &ag.cwd);
                self.fetch_git_diff(host_id, &ag.cwd, None, false);
            }
        }
        for (stale_ag_id, should_resubscribe) in stale_replica_agents {
            let _ = self.store.replace_replica_timeline(&stale_ag_id, &[]);
            if should_resubscribe {
                self.dispatch_wire(
                    host_id,
                    WireMessage::SubscribeTimeline {
                        request_id: Uuid::new_v4(),
                        agent_id: stale_ag_id,
                        since_seq: 0,
                    },
                );
            }
        }
        if let Some(new_agent_id) = auto_subscribe_agent {
            self.open_agent_timeline(host_id, &new_agent_id);
        }
        if let Some((catchup_agent_id, since_seq)) = continue_catchup {
            self.dispatch_wire(
                host_id,
                WireMessage::SubscribeTimeline {
                    request_id: Uuid::new_v4(),
                    agent_id: catchup_agent_id,
                    since_seq,
                },
            );
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
                    if slot == state.active_terminal_slot {
                        state.terminal_emulator.process(&data);
                        raw_out = Some((slot, data));
                    }
                }
                TerminalBinaryFrame::Snapshot { slot, data } => {
                    if slot == state.active_terminal_slot {
                        state.terminal_emulator.clear();
                        state.terminal_emulator.process(&data);
                        raw_out = Some((slot, data));
                    }
                }
                TerminalBinaryFrame::Resize { slot, cols, rows } => {
                    if slot == state.active_terminal_slot {
                        state.terminal_emulator.resize(cols, rows);
                    }
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
        self.dispatch_wire(
            host_id,
            WireMessage::ListProviderModels {
                request_id: Uuid::new_v4(),
                force_refresh: false,
            },
        );
    }

    pub fn refresh_provider_models(self: &Arc<Self>, host_id: &str, force_refresh: bool) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ListProviderModels {
                request_id: Uuid::new_v4(),
                force_refresh,
            },
        );
    }

    pub fn refresh_quotas(self: &Arc<Self>, host_id: &str) {
        self.dispatch_wire_queued(
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
        let _ = self.create_agent_with_config(CreateAgentRequest {
            host_id,
            cwd,
            extra_dirs: Vec::new(),
            provider,
            model: None,
            effort: None,
            mode,
            initial_prompt,
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch,
        });
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
        self.create_agent_with_config(CreateAgentRequest {
            host_id,
            cwd,
            extra_dirs,
            provider,
            model,
            effort: None,
            mode,
            initial_prompt,
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch,
        })
    }

    pub fn create_agent_with_config(self: &Arc<Self>, req: CreateAgentRequest<'_>) -> String {
        let clean_branch = req.worktree_branch.map(str::trim).filter(|b| !b.is_empty());
        let use_auto_worktree = clean_branch == Some("auto");
        if let Some(branch) = clean_branch.filter(|b| *b != "auto") {
            self.dispatch_wire_queued(
                req.host_id,
                WireMessage::CreateWorkspace {
                    request_id: Uuid::new_v4(),
                    source_cwd: req.cwd.to_string(),
                    title: Some(branch.to_string()),
                    worktree_branch: Some(branch.to_string()),
                    base_ref: None,
                },
            );
        }

        let req_id = Uuid::new_v4();
        let opt_id = format!("opt_{req_id}");
        let now = Utc::now();
        let clean_model = req.model.filter(|m| !m.trim().is_empty());
        let visible_seed = req
            .initial_display_prompt
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| req.initial_badges.first().map(|b| b.label.as_str()))
            .unwrap_or(req.initial_prompt);
        let first_line = visible_seed.lines().next().unwrap_or("").trim();
        let title: String = {
            let chars: Vec<char> = first_line.chars().collect();
            if chars.is_empty() {
                "New Session".to_string()
            } else if chars.len() > 48 {
                format!("{}…", chars[..48].iter().collect::<String>())
            } else {
                first_line.to_string()
            }
        };

        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(req.host_id)
        {
            let opt_agent = AgentSummary {
                agent_id: opt_id.clone(),
                workspace_id: format!("ws_{opt_id}"),
                cwd: req.cwd.to_string(),
                title,
                provider: req.provider,
                model: clean_model.clone(),
                effort: req.effort,
                mode: req.mode.clone(),
                status: AgentStatus::Initializing,
                parent_agent_id: None,
                latest_seq: 0,
                timeline_epoch: 0,
                created_at: now,
                updated_at: now,
            };
            h.optimistic_by_req_id.insert(req_id, opt_id.clone());
            h.last_created_agent_id = Some(opt_id.clone());
            h.agents.insert(opt_id.clone(), opt_agent);
            let visible_text = req
                .initial_display_prompt
                .clone()
                .unwrap_or_else(|| req.initial_prompt.to_string());
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
                        text: visible_text,
                        badges: req.initial_badges.clone(),
                    },
                }],
            );
        }

        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: req.host_id.to_string(),
        });
        self.emit(|revision| CoreNotification::TimelineChanged {
            revision,
            host_id: req.host_id.to_string(),
            agent_id: opt_id.clone(),
        });

        self.dispatch_wire_queued(
            req.host_id,
            WireMessage::CreateAgent {
                request_id: req_id,
                cwd: req.cwd.to_string(),
                extra_dirs: req.extra_dirs,
                title: None,
                provider: req.provider,
                model: clean_model,
                effort: req.effort,
                mode: req.mode,
                initial_prompt: req.initial_prompt.to_string(),
                initial_display_prompt: req.initial_display_prompt,
                initial_badges: req.initial_badges,
                worktree: use_auto_worktree,
            },
        );

        opt_id
    }

    pub fn open_agent_timeline(self: &Arc<Self>, host_id: &str, agent_id: &str) {
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
        self.dispatch_wire_queued(
            host_id,
            WireMessage::SubscribeTimeline {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
                since_seq,
            },
        );
    }

    fn record_agent_request(&self, host_id: &str, req_id: Uuid, agent_id: &str) {
        if agent_id.starts_with("opt_") {
            return;
        }
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.agent_req_by_req_id.insert(req_id, agent_id.to_string());
        }
    }

    pub fn send_prompt(self: &Arc<Self>, host_id: &str, agent_id: &str, prompt: &str) {
        self.send_prompt_with_meta(host_id, agent_id, prompt, None, Vec::new());
    }

    pub fn send_prompt_with_meta(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        prompt: &str,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
    ) {
        let now = Utc::now();
        let req_id = Uuid::new_v4();
        let visible_text = display_prompt.clone().unwrap_or_else(|| prompt.to_string());
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ag) = h.agents.get_mut(agent_id) {
                ag.status = AgentStatus::Running {
                    turn_id: "pending".into(),
                };
                ag.updated_at = now;
            }
            if !agent_id.starts_with("opt_") {
                h.agent_req_by_req_id.insert(req_id, agent_id.to_string());
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
                        text: visible_text,
                        badges: badges.clone(),
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
                request_id: req_id,
                agent_id: agent_id.to_string(),
                prompt: prompt.to_string(),
                display_prompt,
                badges,
            },
        );
    }

    pub fn interrupt_agent(&self, host_id: &str, agent_id: &str) {
        let req_id = Uuid::new_v4();
        if !agent_id.starts_with("opt_")
            && let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && h.session.is_some()
        {
            h.agent_req_by_req_id.insert(req_id, agent_id.to_string());
        }
        self.dispatch_wire(
            host_id,
            WireMessage::InterruptTurn {
                request_id: req_id,
                agent_id: agent_id.to_string(),
            },
        );
    }

    pub fn set_session_mode(self: &Arc<Self>, host_id: &str, agent_id: &str, mode: SessionMode) {
        let req_id = Uuid::new_v4();
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ag) = h.agents.get_mut(agent_id) {
                ag.mode = mode.clone();
            }
            if !agent_id.starts_with("opt_") {
                h.agent_req_by_req_id.insert(req_id, agent_id.to_string());
            }
        }
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::SetSessionMode {
                    request_id: req_id,
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

    pub fn set_agent_config(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
    ) {
        let req_id = Uuid::new_v4();
        let clean_model = model.filter(|m| !m.trim().is_empty());
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ag) = h.agents.get_mut(agent_id) {
                ag.model = clean_model.clone();
                ag.effort = effort;
            }
            if !agent_id.starts_with("opt_") {
                h.agent_req_by_req_id.insert(req_id, agent_id.to_string());
            }
        }
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::SetAgentConfig {
                    request_id: req_id,
                    agent_id: agent_id.to_string(),
                    model: clean_model,
                    effort,
                },
            );
        }
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn set_agent_model(self: &Arc<Self>, host_id: &str, agent_id: &str, model: Option<String>) {
        let req_id = Uuid::new_v4();
        let clean_model = model.filter(|m| !m.trim().is_empty());
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ag) = h.agents.get_mut(agent_id) {
                ag.model = clean_model.clone();
            }
            if !agent_id.starts_with("opt_") {
                h.agent_req_by_req_id.insert(req_id, agent_id.to_string());
            }
        }
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::SetAgentModel {
                    request_id: req_id,
                    agent_id: agent_id.to_string(),
                    model: clean_model,
                },
            );
        }
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn switch_agent_provider(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        provider: ProviderKind,
    ) {
        self.switch_agent_provider_with_config(host_id, agent_id, provider, None, None);
    }

    pub fn switch_agent_provider_with_config(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        provider: ProviderKind,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
    ) {
        let req_id = Uuid::new_v4();
        let clean_model = model.filter(|m| !m.trim().is_empty());
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ag) = h.agents.get_mut(agent_id) {
                ag.provider = provider;
                ag.model = clean_model.clone();
                ag.effort = effort;
            }
            if !agent_id.starts_with("opt_") {
                h.agent_req_by_req_id.insert(req_id, agent_id.to_string());
            }
        }
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::SwitchAgentProvider {
                    request_id: req_id,
                    agent_id: agent_id.to_string(),
                    provider,
                    model: clean_model,
                    effort,
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
        let req_id = Uuid::new_v4();
        self.record_agent_request(host_id, req_id, agent_id);
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ResolvePermission {
                request_id: req_id,
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
        let req_id = Uuid::new_v4();
        self.record_agent_request(host_id, req_id, agent_id);
        self.dispatch_wire_queued(
            host_id,
            WireMessage::AnswerQuestion {
                request_id: req_id,
                agent_id: agent_id.to_string(),
                question_request_id: question_request_id.to_string(),
                answers,
            },
        );
    }

    pub fn fetch_git_status(self: &Arc<Self>, host_id: &str, cwd: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::GetGitStatus {
                request_id: Uuid::new_v4(),
                cwd: cwd.to_string(),
            },
        );
    }

    pub fn fetch_git_diff(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
        path: Option<&str>,
        staged: bool,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::GetGitDiff {
                request_id: Uuid::new_v4(),
                cwd: cwd.to_string(),
                path: path.map(ToOwned::to_owned),
                staged,
            },
        );
    }

    pub fn fetch_workspace_files(self: &Arc<Self>, host_id: &str, cwd: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ListWorkspaceFiles {
                request_id: Uuid::new_v4(),
                cwd: cwd.to_string(),
            },
        );
    }

    pub fn stage_attachment(
        self: &Arc<Self>,
        host_id: &str,
        filename: &str,
        _mime_type: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<StagedAttachmentView, ClientCoreError> {
        if bytes.is_empty() {
            return Err(ClientCoreError::Transport("Attachment is empty".into()));
        }
        let info = self
            .runtime
            .block_on(self.stage_attachment_on_host(host_id, filename, bytes))
            .map_err(ClientCoreError::Transport)?;
        Ok(StagedAttachmentView {
            path: info.path,
            filename: info.filename,
            size_bytes: info.size_bytes as u64,
        })
    }

    pub fn workspace_files(&self, host_id: &str, cwd: &str) -> Vec<String> {
        let Ok(guard) = self.hosts.read() else {
            return Vec::new();
        };
        guard
            .get(host_id)
            .and_then(|h| {
                h.workspace_files.get(cwd).or_else(|| {
                    h.workspace_files
                        .iter()
                        .find_map(|(k, v)| cwd_paths_equivalent(k, cwd).then_some(v))
                })
            })
            .cloned()
            .unwrap_or_default()
    }

    pub fn start_terminal(self: &Arc<Self>, host_id: &str, cwd: &str, cols: u16, rows: u16) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.terminal_emulator.clear();
            h.terminal_emulator.resize(cols, rows);
        }
        self.dispatch_wire_queued(
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

    pub fn attach_terminal(self: &Arc<Self>, host_id: &str, terminal_id: &str, slot: u8) {
        let slot = slot.max(1);
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if h.active_terminal_id.as_deref() != Some(terminal_id) {
                h.terminal_emulator.clear();
            }
            h.active_terminal_id = Some(terminal_id.to_string());
            h.active_terminal_slot = slot;
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::AttachTerminal {
                request_id: Uuid::new_v4(),
                terminal_id: terminal_id.to_string(),
                slot,
            },
        );
    }

    pub fn kill_terminal(&self, host_id: &str, terminal_id: &str) {
        let _ = self.store.delete_terminal(terminal_id);
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.terminals.retain(|t| t.terminal_id != terminal_id);
            if h.active_terminal_id.as_deref() == Some(terminal_id) {
                h.active_terminal_id = h.terminals.first().map(|t| t.terminal_id.clone());
            }
        }
        self.dispatch_wire(
            host_id,
            WireMessage::KillTerminal {
                request_id: Uuid::new_v4(),
                terminal_id: terminal_id.to_string(),
            },
        );
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn send_terminal_input(&self, host_id: &str, input_bytes: Vec<u8>) {
        let slot = self
            .hosts
            .read()
            .ok()
            .and_then(|g| g.get(host_id).map(|h| h.active_terminal_slot));
        if let Some(s) = slot {
            self.send_terminal_input_slot(host_id, s, input_bytes);
        }
    }

    pub fn send_terminal_input_slot(&self, host_id: &str, slot: u8, input_bytes: Vec<u8>) {
        let handle_opt = self
            .hosts
            .read()
            .ok()
            .and_then(|g| g.get(host_id).and_then(|h| h.session.clone()));
        if let Some(handle) = handle_opt {
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
        let slot = self
            .hosts
            .read()
            .ok()
            .and_then(|g| g.get(host_id).map(|h| h.active_terminal_slot));
        if let Some(s) = slot {
            self.resize_terminal_slot(host_id, s, cols, rows);
        }
    }

    pub fn resize_terminal_slot(&self, host_id: &str, slot: u8, cols: u16, rows: u16) {
        let mut handle_opt = None;
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if h.active_terminal_slot == slot {
                h.terminal_emulator.resize(cols, rows);
            }
            handle_opt = h.session.clone();
        }
        if let Some(handle) = handle_opt {
            self.runtime.spawn(async move {
                let _ = handle
                    .send_terminal(TerminalBinaryFrame::Resize { slot, cols, rows })
                    .await;
            });
        }
    }

    // --- Mobile App & Network Lifecycle Hooks (iOS / Android / HarmonyOS) ---

    /// Invoked when a mobile client returns to foreground:
    /// - Resets `reconnect_attempt = 0` and immediately reconnects disconnected hosts.
    /// - Sends a fresh `Ping` + `ListWorkspaces` + `ListAgents` (plus active timeline catch-ups)
    ///   on connected hosts so suspended mobile sockets immediately detect staleness or catch up.
    pub fn on_app_foreground(self: &Arc<Self>) {
        type ConnectedRefresh = (String, HostSessionHandle, Vec<(String, u64)>);

        let mut disconnected_ids: Vec<String> = Vec::new();
        let mut connected: Vec<ConnectedRefresh> = Vec::new();

        if let Ok(mut guard) = self.hosts.write() {
            for (host_id, h) in guard.iter_mut() {
                if let Some(ref handle) = h.session
                    && h.profile.state != HostConnectionState::Disconnected
                {
                    let resubs: Vec<(String, u64)> = h
                        .timelines
                        .iter()
                        .filter(|(aid, _)| !aid.starts_with("opt_"))
                        .map(|(aid, evs)| {
                            let max_seq = evs.iter().map(|e| e.seq).max().unwrap_or(0);
                            (aid.clone(), max_seq)
                        })
                        .collect();
                    connected.push((host_id.clone(), handle.clone(), resubs));
                } else {
                    h.reconnect_attempt = 0;
                    h.ssh_heal_attempted = false;
                    if !h.connecting_in_flight {
                        disconnected_ids.push(host_id.clone());
                    }
                }
            }
        }

        for host_id in disconnected_ids {
            self.connect_host_async(host_id);
        }

        for (host_id, handle, resubs) in connected {
            let core = Arc::clone(self);
            self.runtime.spawn(async move {
                let ping_res = handle
                    .send_wire(WireMessage::Ping {
                        ts_ms: unix_ms_now(),
                    })
                    .await;
                if ping_res.is_err() {
                    core.handle_unexpected_disconnect(&host_id);
                    return;
                }
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
                for (agent_id, since_seq) in resubs {
                    let _ = handle
                        .send_wire(WireMessage::SubscribeTimeline {
                            request_id: Uuid::new_v4(),
                            agent_id,
                            since_seq,
                        })
                        .await;
                }
            });
        }
    }

    /// Invoked when a mobile client enters background: performs a passive SQLite WAL checkpoint
    /// without dropping active WebSocket / Noise E2EE sessions.
    pub fn on_app_background(&self) {
        let _ = self.store.checkpoint_passive();
    }

    /// Invoked when mobile network reachability changes (e.g. Wi-Fi <-> Cellular):
    /// - Resets `reconnect_attempt = 0` across all hosts.
    /// - Connects disconnected hosts immediately and sends an immediate `Ping` on connected hosts
    ///   so dead LAN sockets flip to E2EE Relay fast.
    pub fn on_network_changed(self: &Arc<Self>) {
        let mut disconnected_ids: Vec<String> = Vec::new();
        let mut connected: Vec<(String, HostSessionHandle)> = Vec::new();

        if let Ok(mut guard) = self.hosts.write() {
            for (host_id, h) in guard.iter_mut() {
                h.reconnect_attempt = 0;
                h.ssh_heal_attempted = false;
                if let Some(ref handle) = h.session
                    && h.profile.state != HostConnectionState::Disconnected
                {
                    connected.push((host_id.clone(), handle.clone()));
                } else if !h.connecting_in_flight {
                    disconnected_ids.push(host_id.clone());
                }
            }
        }

        for host_id in disconnected_ids {
            self.connect_host_async(host_id);
        }

        for (host_id, handle) in connected {
            let core = Arc::clone(self);
            self.runtime.spawn(async move {
                if handle
                    .send_wire(WireMessage::Ping {
                        ts_ms: unix_ms_now(),
                    })
                    .await
                    .is_err()
                {
                    core.handle_unexpected_disconnect(&host_id);
                }
            });
        }
    }

    pub async fn stage_attachment_b64_on_host(
        self: &Arc<Self>,
        host_id: &str,
        filename: &str,
        data_b64: &str,
    ) -> Result<StagedAttachmentInfo, String> {
        let bytes = B64
            .decode(data_b64.trim())
            .map_err(|e| format!("invalid base64 attachment payload: {e}"))?;
        self.stage_attachment_on_host(host_id, filename, bytes)
            .await
    }

    pub async fn stage_attachment_on_host(
        self: &Arc<Self>,
        host_id: &str,
        filename: &str,
        bytes: Vec<u8>,
    ) -> Result<StagedAttachmentInfo, String> {
        const MAX_BYTES: usize = 25 * 1024 * 1024;
        const CHUNK_BYTES: usize = 512 * 1024;

        if bytes.len() > MAX_BYTES {
            return Err("Attachment exceeds 25 MiB limit".into());
        }

        let (session, is_local, daemon_version, daemon_protocol_version) = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            (
                h.session.clone(),
                h.is_local,
                h.daemon_version.clone(),
                h.daemon_protocol_version,
            )
        };

        let Some(session) = session else {
            if is_local {
                return stage_local_attachment(filename, &bytes);
            }
            return Err(
                "Remote host is disconnected; reconnect before uploading attachments".into(),
            );
        };

        let supports_remote_attachments = daemon_protocol_version.is_some_and(|pv| pv >= 2);
        if !supports_remote_attachments
            && let Some(ref ver) = daemon_version
            && !version_supports_remote_attachments(ver)
        {
            return Err(format!(
                "Remote orbit daemon (v{ver}) does not support remote attachment upload; please upgrade it to v0.3.3+"
            ));
        }

        let core = Arc::clone(self);
        let owned_host_id = host_id.to_string();
        let owned_filename = filename.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let result = core
                .upload_attachment_chunks(
                    AttachmentUploadTarget {
                        host_id: &owned_host_id,
                        session,
                        daemon_version: daemon_version.as_deref(),
                        daemon_protocol_version,
                    },
                    &owned_filename,
                    &bytes,
                    CHUNK_BYTES,
                )
                .await;
            let _ = res_tx.send(result);
        });

        res_rx
            .await
            .map_err(|_| "attachment upload task cancelled".to_string())?
    }

    async fn upload_attachment_chunks(
        self: &Arc<Self>,
        target: AttachmentUploadTarget<'_>,
        filename: &str,
        bytes: &[u8],
        chunk_size: usize,
    ) -> Result<StagedAttachmentInfo, String> {
        let AttachmentUploadTarget {
            host_id,
            session,
            daemon_version,
            daemon_protocol_version,
        } = target;
        let chunks: Vec<&[u8]> = if bytes.is_empty() {
            vec![&[]]
        } else {
            bytes.chunks(chunk_size).collect()
        };
        let total_chunks = chunks.len() as u32;
        let upload_id = Uuid::new_v4().simple().to_string();

        for (idx, slice) in chunks.into_iter().enumerate() {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            {
                let mut guard = self
                    .hosts
                    .write()
                    .map_err(|_| "host state lock poisoned".to_string())?;
                let Some(h) = guard.get_mut(host_id) else {
                    return Err(format!("host `{host_id}` removed during attachment upload"));
                };
                if h.session.is_none() {
                    return Err("Host disconnected during attachment upload".into());
                }
                h.pending_attachment_acks.insert(req_id, ack_tx);
            }

            let wire_msg = WireMessage::StageAttachmentChunk {
                request_id: req_id,
                upload_id: upload_id.clone(),
                filename: filename.to_string(),
                chunk_index: idx as u32,
                total_chunks,
                data_b64: B64.encode(slice),
            };

            if let Err(e) = session.send_wire(wire_msg).await {
                if let Ok(mut guard) = self.hosts.write()
                    && let Some(h) = guard.get_mut(host_id)
                {
                    h.pending_attachment_acks.remove(&req_id);
                }
                return Err(format!("failed to send attachment chunk: {e}"));
            }

            let ack = match tokio::time::timeout(Duration::from_secs(30), ack_rx).await {
                Ok(Ok(Ok(ack))) => ack,
                Ok(Ok(Err(rpc_err))) => return Err(rpc_err),
                Ok(Err(_)) => return Err("Host disconnected during attachment upload".into()),
                Err(_) => {
                    if let Ok(mut guard) = self.hosts.write()
                        && let Some(h) = guard.get_mut(host_id)
                    {
                        h.pending_attachment_acks.remove(&req_id);
                    }
                    if !daemon_protocol_version.is_some_and(|pv| pv >= 2)
                        && let Some(ver) = daemon_version
                        && !version_supports_remote_attachments(ver)
                    {
                        return Err(format!(
                            "Remote orbit daemon (v{ver}) does not support remote attachment upload; please upgrade it to v0.3.3+"
                        ));
                    }
                    return Err("Timed out waiting for host to acknowledge attachment chunk".into());
                }
            };

            if idx + 1 == total_chunks as usize {
                if !ack.completed {
                    return Err("Daemon did not mark final attachment chunk as completed".into());
                }
                let remote_path = ack
                    .path
                    .ok_or_else(|| "Daemon did not return staged attachment path".to_string())?;
                return Ok(StagedAttachmentInfo {
                    path: remote_path,
                    filename: ack.filename.unwrap_or_else(|| filename.to_string()),
                    size_bytes: ack.size_bytes.map(|n| n as usize).unwrap_or(bytes.len()),
                });
            }
        }

        Err("Attachment upload ended without confirmation".into())
    }

    pub fn is_local_host(&self, host_id: &str) -> bool {
        self.hosts
            .read()
            .ok()
            .and_then(|g| g.get(host_id).map(|h| h.is_local))
            .unwrap_or(false)
    }

    /// Rewind an agent conversation (and optionally restore workspace files) to strictly before `to_before_seq`.
    pub async fn rewind_agent_on_host(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        to_before_seq: u64,
        restore_files: bool,
    ) -> Result<RewindAgentResultView, String> {
        if agent_id.starts_with("opt_") {
            return Err("Wait for the session to finish initializing before rewinding".into());
        }
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to rewind this session".into());
        };

        let core = Arc::clone(self);
        let owned_host_id = host_id.to_string();
        let owned_agent_id = agent_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&owned_host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_rewinds.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some("Host disconnected before rewinding session".to_string()),
                        None => Some(format!("host `{owned_host_id}` removed")),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session
                .send_wire(WireMessage::RewindAgent {
                    request_id: req_id,
                    agent_id: owned_agent_id,
                    to_before_seq,
                    restore_files,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host_id)
                {
                    h.pending_rewinds.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send rewind request: {e}")));
                return;
            }

            let outcome = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(Ok(view))) => Ok(view),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while rewinding session".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host_id)
                    {
                        h.pending_rewinds.remove(&req_id);
                    }
                    Err(
                        "Timed out waiting for daemon to rewind session (ensure orbit daemon is up to date)"
                            .to_string(),
                    )
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "Rewind task terminated unexpectedly".to_string())?
    }

    /// Read a workspace file from a local or remote Orbit host and pre-render syntax highlighting / Markdown AST.
    pub async fn read_workspace_file_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
        path: &str,
    ) -> Result<WorkspaceFileView, String> {
        let (session, is_local) = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            (h.session.clone(), h.is_local)
        };

        let Some(session) = session else {
            if is_local {
                let owned_host = host_id.to_string();
                let owned_cwd = cwd.to_string();
                let owned_path = path.to_string();
                return tokio::task::spawn_blocking(move || {
                    let raw = read_local_workspace_file(&owned_cwd, &owned_path, None)?;
                    Ok(build_workspace_file_view(
                        &owned_host,
                        &owned_cwd,
                        &owned_path,
                        RawFileReadResponse {
                            resolved_path: raw.resolved_path,
                            content_kind: raw.content_kind,
                            mime_type: raw.mime_type,
                            content: raw.content,
                            size_bytes: raw.size_bytes,
                            truncated: raw.truncated,
                        },
                    ))
                })
                .await
                .map_err(|_| "local file read task cancelled".to_string())?;
            }
            return Err("Remote host is disconnected; reconnect to open files".into());
        };

        let core = Arc::clone(self);
        let owned_host_id = host_id.to_string();
        let owned_cwd = cwd.to_string();
        let owned_path = path.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&owned_host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_file_reads.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some("Host disconnected before reading file".to_string()),
                        None => Some(format!("host `{owned_host_id}` removed")),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            let send_res = session
                .send_wire(WireMessage::ReadWorkspaceFile {
                    request_id: req_id,
                    cwd: owned_cwd.clone(),
                    path: owned_path.clone(),
                    max_bytes: Some(2 * 1024 * 1024),
                })
                .await;

            if let Err(e) = send_res {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host_id)
                {
                    h.pending_file_reads.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to request file read: {e}")));
                return;
            }

            // Shorter wait on local hosts so older local daemons fall back cleanly to local fs read
            let timeout_secs = if is_local { 3 } else { 15 };
            let raw_res = match tokio::time::timeout(Duration::from_secs(timeout_secs), ack_rx).await
            {
                Ok(Ok(Ok(raw))) => Ok(raw),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while reading file".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host_id)
                    {
                        h.pending_file_reads.remove(&req_id);
                    }
                    if is_local {
                        let fallback_cwd = owned_cwd.clone();
                        let fallback_path = owned_path.clone();
                        match tokio::task::spawn_blocking(move || {
                            read_local_workspace_file(&fallback_cwd, &fallback_path, None)
                        })
                        .await
                        {
                            Ok(Ok(local)) => Ok(RawFileReadResponse {
                                resolved_path: local.resolved_path,
                                content_kind: local.content_kind,
                                mime_type: local.mime_type,
                                content: local.content,
                                size_bytes: local.size_bytes,
                                truncated: local.truncated,
                            }),
                            Ok(Err(e)) => Err(e),
                            Err(_) => Err("local file read task cancelled".to_string()),
                        }
                    } else {
                        Err(
                            "Timed out waiting for remote daemon to read file (ensure remote orbit daemon is up to date)"
                                .to_string(),
                        )
                    }
                }
            };

            let final_view = match raw_res {
                Ok(raw) => tokio::task::spawn_blocking(move || {
                    build_workspace_file_view(&owned_host_id, &owned_cwd, &owned_path, raw)
                })
                .await
                .map_err(|_| "file highlight task cancelled".to_string()),
                Err(e) => Err(e),
            };
            let _ = res_tx.send(final_view);
        });

        res_rx
            .await
            .map_err(|_| "file read task cancelled".to_string())?
    }

    /// Execute an ephemeral side question (`/btw`) on the target host and stream chunks via `event_tx`.
    /// Returns `Ok(request_id)` if dispatched to the host daemon, or `Err(reason)` if unavailable.
    pub async fn ask_side_question_on_host(
        self: &Arc<Self>,
        req: AskSideQuestionRequest,
        event_tx: tokio::sync::mpsc::UnboundedSender<SideQuestionStreamEvent>,
    ) -> Result<String, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&req.host_id)
                .ok_or_else(|| format!("host `{}` not found", req.host_id))?;
            h.session.clone()
        };

        let Some(session) = session else {
            return Err("Host is disconnected; reconnect before asking a side question".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let req_id_str = req_id.to_string();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&req.host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_side_questions.insert(req_id, event_tx);
                            None
                        }
                        Some(_) => Some("Host disconnected before sending side question".into()),
                        None => Some(format!("host `{}` removed", req.host_id)),
                    },
                    Err(_) => Some("host state lock poisoned".into()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            let wire_msg = WireMessage::AskSideQuestion {
                request_id: req_id,
                agent_id: req.agent_id,
                cwd: req.cwd,
                provider: req.provider,
                model: req.model,
                question: req.question,
                context_summary: req.context_summary,
            };

            if let Err(e) = session.send_wire(wire_msg).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&req.host_id)
                {
                    h.pending_side_questions.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send side question: {e}")));
                return;
            }

            let _ = res_tx.send(Ok(req_id_str));

            // Reap orphaned pending side-question channels if an older remote daemon never replies.
            let host_for_timeout = req.host_id;
            let core_for_timeout = Arc::clone(&core);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(105)).await;
                if let Ok(mut guard) = core_for_timeout.hosts.write()
                    && let Some(h) = guard.get_mut(&host_for_timeout)
                    && let Some(tx) = h.pending_side_questions.remove(&req_id)
                {
                    let _ = tx.send(SideQuestionStreamEvent::Done {
                        request_id: req_id.to_string(),
                        full_text: String::new(),
                        error: Some(
                            "Timed out waiting for side question response (ensure remote orbit daemon is up to date)"
                                .to_string(),
                        ),
                    });
                }
            });
        });

        res_rx
            .await
            .map_err(|_| "side question dispatch cancelled".to_string())?
    }
}

fn build_workspace_file_view(
    host_id: &str,
    cwd: &str,
    path: &str,
    raw: RawFileReadResponse,
) -> WorkspaceFileView {
    let language = detect_language_for_path(path).to_string();
    let (highlighted_lines, markdown_blocks) = if raw.content_kind == "text" {
        let lines = highlight_code_lines_capped(&language, &raw.content, 2500);
        let md = if language == "markdown" {
            parse_markdown_blocks(&raw.content, false)
        } else {
            Vec::new()
        };
        (lines, md)
    } else {
        (Vec::new(), Vec::new())
    };

    WorkspaceFileView {
        host_id: host_id.to_string(),
        cwd: cwd.to_string(),
        path: path.to_string(),
        resolved_path: raw.resolved_path,
        language,
        content_kind: raw.content_kind,
        mime_type: raw.mime_type,
        content: raw.content,
        size_bytes: raw.size_bytes,
        truncated: raw.truncated,
        highlighted_lines,
        markdown_blocks,
    }
}

struct AttachmentUploadTarget<'a> {
    host_id: &'a str,
    session: HostSessionHandle,
    daemon_version: Option<&'a str>,
    daemon_protocol_version: Option<u32>,
}

fn fail_pending_host_requests(h: &mut HostMemoryState, reason: &str) {
    for (_, tx) in h.pending_attachment_acks.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_file_reads.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_rewinds.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (req_id, tx) in h.pending_side_questions.drain() {
        let _ = tx.send(SideQuestionStreamEvent::Done {
            request_id: req_id.to_string(),
            full_text: String::new(),
            error: Some(reason.to_string()),
        });
    }
}

fn version_supports_remote_attachments(raw: &str) -> bool {
    let clean = raw
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()
        .unwrap_or("");
    let mut parts = clean.split('.').map(|s| s.parse::<u32>());
    let (Some(Ok(maj)), Some(Ok(min)), Some(Ok(pat))) = (parts.next(), parts.next(), parts.next())
    else {
        return true;
    };
    (maj, min, pat) >= (0, 3, 3)
}

fn normalize_cwd_trim(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed == "/" {
        "/"
    } else {
        trimmed.trim_end_matches('/')
    }
}

fn cwd_paths_equivalent(stored_cwd: &str, query_cwd: &str) -> bool {
    let a = normalize_cwd_trim(stored_cwd);
    let b = normalize_cwd_trim(query_cwd);
    if a == b {
        return true;
    }
    if let Some(rel) = b.strip_prefix("~/") {
        let suffix = format!("/{rel}");
        if a.ends_with(&suffix) {
            return true;
        }
    }
    if let Some(rel) = a.strip_prefix("~/") {
        let suffix = format!("/{rel}");
        if b.ends_with(&suffix) {
            return true;
        }
    }
    false
}

struct CoalesceSlot<T> {
    last_emit: Option<tokio::time::Instant>,
    pending: Option<T>,
    timer_scheduled: bool,
}

impl<T> Default for CoalesceSlot<T> {
    fn default() -> Self {
        Self {
            last_emit: None,
            pending: None,
            timer_scheduled: false,
        }
    }
}

struct CoalescingState {
    timelines: HashMap<(String, String), CoalesceSlot<CoreNotification>>,
    terminals: HashMap<(String, u8), CoalesceSlot<CoreNotification>>,
}

struct CoalescingSubscriber {
    downstream: Arc<dyn CoreEventSubscriber>,
    runtime_handle: tokio::runtime::Handle,
    interval: Duration,
    state: Arc<Mutex<CoalescingState>>,
}

impl CoalescingSubscriber {
    fn new(
        downstream: Arc<dyn CoreEventSubscriber>,
        runtime_handle: tokio::runtime::Handle,
        interval: Duration,
    ) -> Self {
        Self {
            downstream,
            runtime_handle,
            interval,
            state: Arc::new(Mutex::new(CoalescingState {
                timelines: HashMap::new(),
                terminals: HashMap::new(),
            })),
        }
    }
}

impl CoreEventSubscriber for CoalescingSubscriber {
    fn on_notification(&self, event: CoreNotification) {
        match event {
            CoreNotification::FleetChanged { .. }
            | CoreNotification::HostChanged { .. }
            | CoreNotification::GitChanged { .. } => {
                self.downstream.on_notification(event);
            }
            CoreNotification::TimelineChanged {
                ref host_id,
                ref agent_id,
                ..
            } => {
                let key = (host_id.clone(), agent_id.clone());
                let now = tokio::time::Instant::now();
                let mut emit_now = false;
                let mut schedule_after: Option<Duration> = None;

                if let Ok(mut guard) = self.state.lock() {
                    let slot = guard.timelines.entry(key.clone()).or_default();
                    let elapsed = slot.last_emit.map(|t| now.saturating_duration_since(t));
                    if !slot.timer_scheduled && elapsed.is_none_or(|d| d >= self.interval) {
                        slot.last_emit = Some(now);
                        emit_now = true;
                    } else {
                        slot.pending = Some(event.clone());
                        if !slot.timer_scheduled {
                            slot.timer_scheduled = true;
                            let wait = elapsed
                                .map(|d| self.interval.saturating_sub(d))
                                .unwrap_or(self.interval);
                            schedule_after = Some(wait);
                        }
                    }
                }

                if emit_now {
                    self.downstream.on_notification(event);
                } else if let Some(wait) = schedule_after {
                    let state = Arc::clone(&self.state);
                    let downstream = Arc::clone(&self.downstream);
                    self.runtime_handle.spawn(async move {
                        tokio::time::sleep(wait).await;
                        let ready = state.lock().ok().and_then(|mut g| {
                            let slot = g.timelines.get_mut(&key)?;
                            slot.timer_scheduled = false;
                            let ev = slot.pending.take()?;
                            slot.last_emit = Some(tokio::time::Instant::now());
                            Some(ev)
                        });
                        if let Some(ev) = ready {
                            downstream.on_notification(ev);
                        }
                    });
                }
            }
            CoreNotification::TerminalChanged {
                revision,
                ref host_id,
                slot: term_slot,
                ref raw_bytes,
            } => {
                let key = (host_id.clone(), term_slot);
                let now = tokio::time::Instant::now();
                let mut emit_now = false;
                let mut schedule_after: Option<Duration> = None;

                if let Ok(mut guard) = self.state.lock() {
                    let slot = guard.terminals.entry(key.clone()).or_default();
                    let elapsed = slot.last_emit.map(|t| now.saturating_duration_since(t));
                    if !slot.timer_scheduled && elapsed.is_none_or(|d| d >= self.interval) {
                        slot.last_emit = Some(now);
                        emit_now = true;
                    } else {
                        match slot.pending {
                            Some(CoreNotification::TerminalChanged {
                                revision: ref mut prev_rev,
                                raw_bytes: ref mut prev_bytes,
                                ..
                            }) => {
                                *prev_rev = revision;
                                prev_bytes.extend_from_slice(raw_bytes);
                            }
                            _ => {
                                slot.pending = Some(event.clone());
                            }
                        }
                        if !slot.timer_scheduled {
                            slot.timer_scheduled = true;
                            let wait = elapsed
                                .map(|d| self.interval.saturating_sub(d))
                                .unwrap_or(self.interval);
                            schedule_after = Some(wait);
                        }
                    }
                }

                if emit_now {
                    self.downstream.on_notification(event);
                } else if let Some(wait) = schedule_after {
                    let state = Arc::clone(&self.state);
                    let downstream = Arc::clone(&self.downstream);
                    self.runtime_handle.spawn(async move {
                        tokio::time::sleep(wait).await;
                        let ready = state.lock().ok().and_then(|mut g| {
                            let slot = g.terminals.get_mut(&key)?;
                            slot.timer_scheduled = false;
                            let ev = slot.pending.take()?;
                            slot.last_emit = Some(tokio::time::Instant::now());
                            Some(ev)
                        });
                        if let Some(ev) = ready {
                            downstream.on_notification(ev);
                        }
                    });
                }
            }
        }
    }
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn truncate_utf8_prefix(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn folder_basename(path: &str) -> String {
    let clean = path.trim_end_matches('/');
    if clean.is_empty() || clean == "~" {
        return "Home".to_string();
    }
    clean
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(clean)
        .to_string()
}

fn strip_segment_scope(id: &str) -> &str {
    id.split_once("@s").map(|(base, _)| base).unwrap_or(id)
}

fn is_legacy_shared_item_id(raw_id: &str) -> bool {
    matches!(
        raw_id,
        "agy_msg_default" | "agy_thought_default" | "codex_msg" | "codex_rsn" | "item"
    )
}

/// Records an RPC error against an agent in memory, injecting a client-local error `Notice`
/// (with `seq == 0` so daemon `latest_seq` cursors are never skewed) and optionally resetting
/// an optimistic `Running` status back to `AgentStatus::Error`.
fn record_agent_rpc_error(
    state: &mut HostMemoryState,
    agent_id: &str,
    status_message: &str,
    notice_message: String,
    reset_status: bool,
) {
    if reset_status && let Some(ag) = state.agents.get_mut(agent_id) {
        ag.status = AgentStatus::Error {
            message: status_message.to_string(),
            recoverable: true,
        };
    }
    state
        .timelines
        .entry(agent_id.to_string())
        .or_default()
        .push(TimelineEvent {
            seq: 0,
            epoch: 1,
            agent_id: agent_id.to_string(),
            turn_id: None,
            timestamp: Utc::now(),
            payload: TimelinePayload::Notice {
                level: "error".into(),
                message: notice_message,
            },
        });
}

fn is_pending_optimistic_prompt(ev: &TimelineEvent) -> bool {
    ev.seq == 0 && matches!(ev.payload, TimelinePayload::UserPrompt { .. })
}

fn coalesce_or_push_timeline(list: &mut Vec<TimelineEvent>, mut incoming: TimelineEvent) {
    let active_end = list
        .iter()
        .position(is_pending_optimistic_prompt)
        .unwrap_or(list.len());
    let precedes_incoming =
        |ev: &TimelineEvent| -> bool { ev.seq > 0 && (incoming.seq == 0 || ev.seq < incoming.seq) };

    let turn_start_idx = list[..active_end]
        .iter()
        .rposition(|ev| {
            precedes_incoming(ev) && matches!(ev.payload, TimelinePayload::UserPrompt { .. })
        })
        .map(|i| i + 1)
        .unwrap_or(0);

    let seg_start_idx = list[..active_end]
        .iter()
        .rposition(|ev| {
            precedes_incoming(ev)
                && matches!(
                    ev.payload,
                    TimelinePayload::UserPrompt { .. } | TimelinePayload::ToolCall { .. }
                )
        })
        .map(|i| i + 1)
        .unwrap_or(0);

    let seg_boundary_seq = if seg_start_idx > 0 {
        list[seg_start_idx - 1].seq
    } else {
        0
    };

    match &mut incoming.payload {
        TimelinePayload::UserPrompt {
            text: inc_text,
            message_id: inc_mid,
            ..
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
            let raw_id = strip_segment_scope(item_id).to_string();
            let collides_earlier = is_legacy_shared_item_id(&raw_id)
                || list[..seg_start_idx].iter().any(|ev| {
                    matches!(
                        &ev.payload,
                        TimelinePayload::AssistantMessage { item_id: eid, .. }
                            if strip_segment_scope(eid) == raw_id
                    )
                });
            let effective_id = if collides_earlier {
                format!("{raw_id}@s{seg_boundary_seq}")
            } else {
                raw_id
            };
            *item_id = effective_id.clone();

            if let Some(prev) = list[seg_start_idx..active_end]
                .iter_mut()
                .rev()
                .find(|prev_ev| {
                    matches!(
                        &prev_ev.payload,
                        TimelinePayload::AssistantMessage { item_id: existing_id, .. }
                            if existing_id == &effective_id
                    )
                })
            {
                *prev = incoming;
                return;
            }
        }
        TimelinePayload::AssistantThinking { item_id, .. } if !item_id.is_empty() => {
            let raw_id = strip_segment_scope(item_id).to_string();
            let collides_earlier = is_legacy_shared_item_id(&raw_id)
                || list[..seg_start_idx].iter().any(|ev| {
                    matches!(
                        &ev.payload,
                        TimelinePayload::AssistantThinking { item_id: eid, .. }
                            if strip_segment_scope(eid) == raw_id
                    )
                });
            let effective_id = if collides_earlier {
                format!("{raw_id}@s{seg_boundary_seq}")
            } else {
                raw_id
            };
            *item_id = effective_id.clone();

            if let Some(prev) = list[seg_start_idx..active_end]
                .iter_mut()
                .rev()
                .find(|prev_ev| {
                    matches!(
                        &prev_ev.payload,
                        TimelinePayload::AssistantThinking { item_id: existing_id, .. }
                            if existing_id == &effective_id
                    )
                })
            {
                *prev = incoming;
                return;
            }
        }
        TimelinePayload::ToolCall { call_id, .. } if !call_id.is_empty() => {
            let target_call_id = call_id.clone();
            if let Some(prev) = list[turn_start_idx..active_end]
                .iter_mut()
                .rev()
                .find(|prev_ev| {
                    matches!(
                        &prev_ev.payload,
                        TimelinePayload::ToolCall { call_id: existing_id, .. }
                            if existing_id == &target_call_id
                    )
                })
            {
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
        if let Some(first_opt_idx) = list.iter().position(is_pending_optimistic_prompt) {
            list.insert(first_opt_idx, incoming);
            return;
        }
    }
    list.push(incoming);
}

fn agent_to_card_view(a: &AgentSummary) -> AgentCardView {
    let (status_label, is_running, is_awaiting_permission) = match &a.status {
        AgentStatus::Initializing => ("Starting".to_string(), true, false),
        AgentStatus::Idle => ("Idle".to_string(), false, false),
        AgentStatus::Running { .. } => ("Working".to_string(), true, false),
        AgentStatus::AutonomousWakeup { source, .. } => (format!("Waking ({source})"), true, false),
        AgentStatus::AwaitingPermission { .. } => ("Needs approval".to_string(), false, true),
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
        effort: a.effort,
        mode: a.mode.clone(),
        status: a.status.clone(),
        status_label,
        is_running,
        is_awaiting_permission,
        parent_agent_id: a.parent_agent_id.clone(),
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

fn default_client_models_map() -> HashMap<ProviderKind, Vec<ModelOption>> {
    let mut m = HashMap::new();
    m.insert(
        ProviderKind::Antigravity,
        vec![
            ModelOption {
                id: "gemini-4-carbon-high".into(),
                label: "Gemini 4 Carbon (High)".into(),
                description: Some("Flagship deep reasoning and multi-step orchestration".into()),
                is_default: true,
            },
            ModelOption {
                id: "gemini-4-carbon-medium".into(),
                label: "Gemini 4 Carbon (Medium)".into(),
                description: Some("Balanced speed and reasoning depth".into()),
                is_default: false,
            },
            ModelOption {
                id: "gemini-4-argon-high".into(),
                label: "Gemini 4 Argon (High)".into(),
                description: Some("High-intensity code architecture reasoning".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-opus-5-5-high".into(),
                label: "Claude Opus 5.5 (High)".into(),
                description: Some("Antigravity Opus deep reasoning mode".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-sonnet-5-5-high".into(),
                label: "Claude Sonnet 5.5 (High)".into(),
                description: Some("Responsive daily software engineering".into()),
                is_default: false,
            },
            ModelOption {
                id: "gemini-3.8-flash-high".into(),
                label: "Gemini 3.8 Flash (High)".into(),
                description: Some("Low-latency lightweight execution".into()),
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
                description: Some("Primary daily coding and refactoring model".into()),
                is_default: true,
            },
            ModelOption {
                id: "gpt-6-astra".into(),
                label: "GPT-6-Astra".into(),
                description: Some("Complex systems architecture model".into()),
                is_default: false,
            },
            ModelOption {
                id: "gpt-6.1-terra".into(),
                label: "GPT-6.1-Terra".into(),
                description: Some("Balanced coding and computer-use model".into()),
                is_default: false,
            },
            ModelOption {
                id: "gpt-5.4".into(),
                label: "GPT-5.4".into(),
                description: Some("General-purpose compatible Codex model".into()),
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
                description: Some("Flagship architecture and deep reasoning".into()),
                is_default: true,
            },
            ModelOption {
                id: "claude-sonnet-5-5".into(),
                label: "Sonnet 5.5".into(),
                description: Some("Fast, high-precision coding model".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-opus-4-6".into(),
                label: "Opus 4.6".into(),
                description: Some("Proven complex engineering model".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-sonnet-4-6".into(),
                label: "Sonnet 4.6".into(),
                description: Some("Fast everyday development".into()),
                is_default: false,
            },
            ModelOption {
                id: "claude-haiku-5-5".into(),
                label: "Haiku 5.5".into(),
                description: Some("Instant Q&A and codebase search".into()),
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

    #[test]
    fn timeline_delta_and_coalescing_work_incrementally() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = PairingOffer {
            host_id: "host_1".into(),
            hostname: "workstation".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "sec".into(),
        };
        let uri = offer.to_qr_uri().expect("uri");
        core.pair_host(&uri).expect("pair");

        let now = chrono::Utc::now();
        core.handle_inbound_wire(
            "host_1",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 1,
                    epoch: 1,
                    agent_id: "ag_1".into(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::UserPrompt {
                        message_id: "m1".into(),
                        text: "Optimize desktop UI".into(),
                        badges: Vec::new(),
                    },
                },
            },
        );
        core.handle_inbound_wire(
            "host_1",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 2,
                    epoch: 1,
                    agent_id: "ag_1".into(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::ToolCall {
                        call_id: "tc_1".into(),
                        status: ToolCallStatus::Running,
                        detail: ToolCallDetail::FileRead {
                            path: "src/App.tsx".into(),
                            start_line: Some(1),
                            end_line: Some(80),
                        },
                    },
                },
            },
        );

        let full = core.agent_timeline_delta("host_1", "ag_1", 0);
        assert_eq!(full.latest_seq, 2);
        assert_eq!(full.upserted_items.len(), 2);

        // Emit completed status for the SAME `tc_1` call_id at seq 3: should coalesce in place and return 1 delta item!
        core.handle_inbound_wire(
            "host_1",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 3,
                    epoch: 1,
                    agent_id: "ag_1".into(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::ToolCall {
                        call_id: "tc_1".into(),
                        status: ToolCallStatus::Completed,
                        detail: ToolCallDetail::FileRead {
                            path: "src/App.tsx".into(),
                            start_line: Some(1),
                            end_line: Some(80),
                        },
                    },
                },
            },
        );

        let delta = core.agent_timeline_delta("host_1", "ag_1", 2);
        assert_eq!(delta.latest_seq, 3);
        assert_eq!(delta.upserted_items.len(), 1);
        assert_eq!(core.agent_timeline("host_1", "ag_1").items.len(), 2);
    }

    #[test]
    fn coalesce_timeline_isolates_shared_item_ids_across_turns_and_tools() {
        let now = chrono::Utc::now();
        let mut list = Vec::new();

        // Turn 1: "你好" -> Assistant reply with shared legacy `agy_msg_default`
        coalesce_or_push_timeline(
            &mut list,
            TimelineEvent {
                seq: 1,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: now,
                payload: TimelinePayload::UserPrompt {
                    message_id: "u1".into(),
                    text: "你好".into(),
                    badges: Vec::new(),
                },
            },
        );
        coalesce_or_push_timeline(
            &mut list,
            TimelineEvent {
                seq: 2,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: now,
                payload: TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_default".into(),
                    text: "你好！".into(),
                    streaming: true,
                },
            },
        );
        coalesce_or_push_timeline(
            &mut list,
            TimelineEvent {
                seq: 3,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: now,
                payload: TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_default".into(),
                    text: "你好！请问有什么可以帮你的？".into(),
                    streaming: false,
                },
            },
        );

        // Turn 2: "看看技术栈" -> ToolCall -> Assistant reply with the SAME `agy_msg_default`
        coalesce_or_push_timeline(
            &mut list,
            TimelineEvent {
                seq: 4,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: now,
                payload: TimelinePayload::UserPrompt {
                    message_id: "u2".into(),
                    text: "看看技术栈".into(),
                    badges: Vec::new(),
                },
            },
        );
        coalesce_or_push_timeline(
            &mut list,
            TimelineEvent {
                seq: 5,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: now,
                payload: TimelinePayload::ToolCall {
                    call_id: "tc_read".into(),
                    status: ToolCallStatus::Completed,
                    detail: ToolCallDetail::FileRead {
                        path: "Cargo.toml".into(),
                        start_line: None,
                        end_line: None,
                    },
                },
            },
        );
        coalesce_or_push_timeline(
            &mut list,
            TimelineEvent {
                seq: 6,
                epoch: 1,
                agent_id: "ag_1".into(),
                turn_id: None,
                timestamp: now,
                payload: TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_default".into(),
                    text: "Orbit 采用纯 Rust 核心。".into(),
                    streaming: false,
                },
            },
        );

        assert_eq!(list.len(), 5);
        match (&list[1].payload, &list[4].payload) {
            (
                TimelinePayload::AssistantMessage {
                    item_id: id1,
                    text: t1,
                    ..
                },
                TimelinePayload::AssistantMessage {
                    item_id: id2,
                    text: t2,
                    ..
                },
            ) => {
                assert_eq!(t1, "你好！请问有什么可以帮你的？");
                assert_eq!(t2, "Orbit 采用纯 Rust 核心。");
                assert_ne!(id1, id2);
            }
            _ => panic!("unexpected timeline order: {list:?}"),
        }
    }

    #[test]
    fn host_detail_effective_catalogs_agent_stage_and_lifecycle_hooks() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = PairingOffer {
            host_id: "host_mobile".into(),
            hostname: "mac-studio".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "sec".into(),
        };
        let uri = offer.to_qr_uri().expect("uri");
        core.pair_host(&uri).expect("pair");

        let detail = core.host_detail("host_mobile").expect("detail");
        assert_eq!(detail.provider_catalogs.len(), 3);

        let opt_id = core.create_agent_with_config(CreateAgentRequest {
            host_id: "host_mobile",
            cwd: "/Users/jess/Code/orbit",
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            model: Some("claude-opus-5-5".into()),
            effort: Some(ReasoningEffort::High),
            mode: SessionMode::AcceptEdits,
            initial_prompt: "Run clippy",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
        });

        let stage = core.agent_stage("host_mobile", &opt_id);
        assert_eq!(stage.turns.len(), 1);
        assert!(stage.turns[0].user_prompt.is_some());

        core.on_app_background();
        core.on_app_foreground();
        core.on_network_changed();
    }

    #[test]
    fn subscribe_coalesced_throttles_timeline_bursts_while_passing_fleet_immediately() {
        struct CountSub {
            fleet_count: AtomicU64,
            timeline_count: AtomicU64,
        }
        impl CoreEventSubscriber for CountSub {
            fn on_notification(&self, event: CoreNotification) {
                match event {
                    CoreNotification::FleetChanged { .. } => {
                        self.fleet_count.fetch_add(1, Ordering::SeqCst);
                    }
                    CoreNotification::TimelineChanged { .. } => {
                        self.timeline_count.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
        }

        let core = OrbitAppCore::open_in_memory().expect("open core");
        let counter = Arc::new(CountSub {
            fleet_count: AtomicU64::new(0),
            timeline_count: AtomicU64::new(0),
        });
        core.subscribe_coalesced(30, counter.clone());

        // FleetChanged passes immediately without coalescing
        core.emit(|revision| CoreNotification::FleetChanged { revision });
        core.emit(|revision| CoreNotification::FleetChanged { revision });
        assert_eq!(counter.fleet_count.load(Ordering::SeqCst), 2);

        // Emit 10 rapid TimelineChanged events for the same agent -> 1 immediate + 1 trailing flush
        for _ in 0..10 {
            core.emit(|revision| CoreNotification::TimelineChanged {
                revision,
                host_id: "h1".into(),
                agent_id: "a1".into(),
            });
        }
        assert_eq!(counter.timeline_count.load(Ordering::SeqCst), 1);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(counter.timeline_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn markdown_cache_keys_isolate_different_agents_on_same_host() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = PairingOffer {
            host_id: "host_cache".into(),
            hostname: "cache-box".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "sec".into(),
        };
        core.pair_host(&offer.to_qr_uri().expect("uri"))
            .expect("pair");

        // Same seq=1, same length (11 bytes), streaming=false, different markdown blocks!
        core.handle_inbound_wire(
            "host_cache",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 1,
                    epoch: 1,
                    agent_id: "ag_one".into(),
                    turn_id: None,
                    timestamp: Utc::now(),
                    payload: TimelinePayload::AssistantMessage {
                        item_id: "m1".into(),
                        text: "# Heading 1".into(),
                        streaming: false,
                    },
                },
            },
        );
        core.handle_inbound_wire(
            "host_cache",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 1,
                    epoch: 1,
                    agent_id: "ag_two".into(),
                    turn_id: None,
                    timestamp: Utc::now(),
                    payload: TimelinePayload::AssistantMessage {
                        item_id: "m2".into(),
                        text: "`codeblock`".into(),
                        streaming: false,
                    },
                },
            },
        );

        let tl1 = core.agent_timeline("host_cache", "ag_one");
        let tl2 = core.agent_timeline("host_cache", "ag_two");
        match (&tl1.items[0], &tl2.items[0]) {
            (
                TimelineItemView::AssistantMessage { blocks: b1, .. },
                TimelineItemView::AssistantMessage { blocks: b2, .. },
            ) => {
                assert!(matches!(b1[0], RichBlockView::Heading { .. }));
                assert!(matches!(b2[0], RichBlockView::Paragraph { .. }));
            }
            other => panic!("unexpected timeline items: {other:?}"),
        }
    }

    #[test]
    fn remote_host_attachment_upload_rejects_disconnected_and_legacy_daemon() {
        assert!(!version_supports_remote_attachments("0.3.2"));
        assert!(version_supports_remote_attachments("0.3.3"));
        assert!(version_supports_remote_attachments("v0.3.3-rc1"));
        assert!(version_supports_remote_attachments("0.4.0"));

        let core = OrbitAppCore::open_in_memory().expect("open core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = PairingOffer {
            host_id: "host_remote_att".into(),
            hostname: "devbox".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: Some("ws://127.0.0.1:7878".into()),
            pairing_secret: "sec".into(),
        };
        core.pair_host(&offer.to_qr_uri().expect("uri"))
            .expect("pair");

        let err = core
            .runtime
            .block_on(core.stage_attachment_on_host(
                "host_remote_att",
                "shot.png",
                b"png-bytes".to_vec(),
            ))
            .expect_err("disconnected remote host must not fall back to local disk");
        assert!(err.contains("disconnected"));
    }

    #[test]
    fn status_changed_error_projects_to_error_notice_in_timeline_and_delta() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = PairingOffer {
            host_id: "host_err_vis".into(),
            hostname: "mac-studio".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "sec".into(),
        };
        let uri = offer.to_qr_uri().expect("uri");
        core.pair_host(&uri).expect("pair");

        let now = chrono::Utc::now();
        core.handle_inbound_wire(
            "host_err_vis",
            WireMessage::TimelineCatchup {
                request_id: Uuid::new_v4(),
                agent_id: "ag_1".into(),
                has_more: false,
                events: vec![TimelineEvent {
                    seq: 1,
                    epoch: 1,
                    agent_id: "ag_1".into(),
                    turn_id: Some("t1".into()),
                    timestamp: now,
                    payload: TimelinePayload::UserPrompt {
                        message_id: "u1".into(),
                        text: "hello".into(),
                        badges: Vec::new(),
                    },
                }],
            },
        );

        core.handle_inbound_wire(
            "host_err_vis",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 2,
                    epoch: 1,
                    agent_id: "ag_1".into(),
                    turn_id: Some("t1".into()),
                    timestamp: now,
                    payload: TimelinePayload::StatusChanged {
                        status: AgentStatus::Error {
                            message: "Prompt rejected while awaiting question answer".into(),
                            recoverable: true,
                        },
                    },
                },
            },
        );

        let full = core.agent_timeline("host_err_vis", "ag_1");
        assert!(full.items.iter().any(|it| matches!(
            it,
            TimelineItemView::Notice {
                seq: 2,
                level,
                message,
                ..
            } if level == "error" && message == "Prompt rejected while awaiting question answer"
        )));

        let delta = core.agent_timeline_delta("host_err_vis", "ag_1", 1);
        assert_eq!(delta.latest_seq, 2);
        assert_eq!(delta.upserted_items.len(), 1);
        assert!(matches!(
            &delta.upserted_items[0],
            TimelineItemView::Notice {
                seq: 2,
                level,
                message,
                ..
            } if level == "error" && message == "Prompt rejected while awaiting question answer"
        ));
    }

    #[test]
    fn rpc_error_for_existing_agent_resets_pending_status_and_surfaces_in_delta() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = PairingOffer {
            host_id: "host_rpc_err".into(),
            hostname: "mac-studio".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "sec".into(),
        };
        let uri = offer.to_qr_uri().expect("uri");
        core.pair_host(&uri).expect("pair");

        let now = chrono::Utc::now();
        core.handle_inbound_wire(
            "host_rpc_err",
            WireMessage::AgentsSnapshot {
                request_id: Uuid::new_v4(),
                agents: vec![AgentSummary {
                    agent_id: "ag_existing".into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/tmp/proj".into(),
                    title: "Existing".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    latest_seq: 5,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                }],
            },
        );
        core.handle_inbound_wire(
            "host_rpc_err",
            WireMessage::TimelineCatchup {
                request_id: Uuid::new_v4(),
                agent_id: "ag_existing".into(),
                has_more: false,
                events: vec![TimelineEvent {
                    seq: 5,
                    epoch: 1,
                    agent_id: "ag_existing".into(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::AssistantMessage {
                        item_id: "m5".into(),
                        text: "ready".into(),
                        streaming: false,
                    },
                }],
            },
        );

        core.send_prompt("host_rpc_err", "ag_existing", "follow up");
        let req_id = {
            let guard = core.hosts.read().expect("read");
            let h = guard.get("host_rpc_err").expect("host");
            assert_eq!(h.pending_outbox.len(), 1);
            match &h.pending_outbox[0] {
                WireMessage::SendPrompt { request_id, .. } => *request_id,
                other => panic!("unexpected outbox msg: {other:?}"),
            }
        };

        core.handle_inbound_wire(
            "host_rpc_err",
            WireMessage::RpcError {
                request_id: Some(req_id),
                code: "send_prompt_failed".into(),
                message: "agent session not active".into(),
            },
        );

        let delta = core.agent_timeline_delta("host_rpc_err", "ag_existing", 5);
        assert_eq!(
            delta.latest_seq, 5,
            "synthetic seq=0 notice must not advance daemon latest_seq"
        );
        let agent_card = delta.agent.expect("agent card");
        assert!(!agent_card.is_running);
        assert_eq!(agent_card.status_label, "Error");
        assert!(delta.upserted_items.iter().any(|it| matches!(
            it,
            TimelineItemView::Notice {
                seq: 0,
                level,
                message,
                ..
            } if level == "error" && message.contains("agent session not active")
        )));
    }
}
