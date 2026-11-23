//! Headless Multi-Host MVU Application Core (`OrbitAppCore`) shared across:
//! - Desktop: Tauri v2 (macOS & Linux)
//! - iOS 16.4+: Swift + SwiftUI (via `orbit-ffi-uniffi`)
//! - Android (minSdk 29 / targetSdk 36): Kotlin + Jetpack Compose (via `orbit-ffi-uniffi`)
//! - HarmonyOS 7+ (API 26): ArkTS + ArkUI V2 (via `orbit-ffi-ohos`)

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::Utc;
use orbit_crypto::PairingOffer;
use orbit_protocol::{
    AcpAgentAvailability, AcpAgentSpec, AcpProbeResult, ActiveTurnBehavior, AgentMessageKind,
    AgentProfile, AgentStatus, AgentSummary, ArtifactDetail, ArtifactSummary, AttentionState,
    BackgroundTaskInfo, BrowserPolicyConfig, BrowserSessionSummary, ConditionalTimerRecord,
    ConditionalTimerState, ConnectedHostSummary, ContextOccupancyBreakdown, DirectoryEntry,
    DiscoveredProject, EventTriggerDelivery, ExternalMcpServerRecord, GitStatusSnapshot,
    GoalRunRecord, GuardianAssessment, GuardianAuditEntry, GuardianPolicyConfig,
    HookExecutionRecord, HookScope, HostProjectEntry, HostSyncState, ImChannelRecord, ImPlatform,
    ImThreadBinding, KeepAwakeStatus, LifecycleHookRule, LspDiagnostic, LspLocation,
    LspServerStatus, LspSymbolInfo, ManagedSkillRecord, McpProbeResult, ModelOption,
    MonitorWatchRecord, OrchestrationConfig, OrchestrationSkillInfo, PathScopedRule,
    PendingPermissionSummary, PermissionCheckpointRef, PromptBadge, ProviderKind,
    ProviderModelCatalog, ProviderQuotaSnapshot, ProviderSubagentDescriptor, ProviderSubagentEvent,
    PushDeviceRecord, PushPlatform, PushPreferences, QuestionItem, QueuedPromptView,
    ReasoningEffort, RuleTriggerMode, ScheduleRunRecord, ScheduleSpec, ScheduleTarget, SessionMode,
    SkillWriteScope, TerminalBinaryFrame, TerminalSummary, TimelineEvent, TimelinePayload,
    TimerCondition, ToolCallDetail, ToolCallStatus, UsageInsightsSnapshot, WireMessage,
    WorkflowRunRecord, WorkspaceBoardSnapshot, WorkspaceClusterSuggestion, WorkspaceEventKind,
    WorkspaceEventRecord, WorkspaceIsolation, WorkspaceOrganizeEvent, WorkspaceScriptInfo,
    WorkspaceSummary, WorktreeCreateMode, WorktreeHandoffOutcome, WorktreeHandoffPreview,
};
pub use orbit_store::ClientThreadPref;
use orbit_store::{OrbitStore, StoredClientProject, StoredHostMeta, StoredHostProfile};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cloud_sync::{
    CloudSyncDevicePollStatus, CloudSyncDevicePollView, CloudSyncError, CloudSyncResultView,
    CloudSyncStatusView, DevicePollOutcome, GitHubCloudClient, GitHubDeviceFlowStartView,
    LocalHostSyncRecord, PersistedCloudSyncConfig, SyncedPreferenceEntry,
    decrypt_remote_with_cached_key, generate_sync_passphrase, load_sync_config_from_store,
    load_sync_meta_from_store, load_sync_prefs_from_store, merge_cloud_vault, now_unix_ms,
    push_vault_payload, resolve_github_client_id, save_sync_config_to_store,
    save_sync_meta_to_store, save_sync_prefs_to_store, stored_host_from_synced,
    unlock_remote_vault_with_passphrase,
};
pub use crate::contract::{
    BrowserActionResponseView, BrowserSessionsSnapshotView, CancelWorkflowRunRequest,
    ControlGoalRunRequest, ConversationSearchHit, CreateBrowserSessionRequest,
    CreateWorkspaceEntryRequest, DeletePathRuleRequest, DeleteWorkspaceEntryRequest,
    EventTriggersSnapshotView, ExecuteBrowserActionRequest, ExecuteWorktreeHandoffRequest,
    ForkAgentRequest, ForkAgentResultView, InstructionFileContentView, InstructionFileSavedView,
    InstructionFilesSnapshotView, LifecycleHooksSnapshotView, ListInstructionFilesRequest,
    MatchPathRulesRequest, MatchedPathRulesView, PathRulesSnapshotView,
    PreviewWorktreeHandoffRequest, ReadInstructionFileRequest, RenameWorkspaceEntryRequest,
    SaveInstructionFileRequest, SelectBestOfNWinnerRequest, StartBatchFanoutRequest,
    StartBestOfNRequest, StartDagWorkflowRequest, StartGoalRunRequest, StartMonitorWatchRequest,
    StopMonitorWatchRequest, UpsertArtifactRequest, UpsertEventTriggerRequest,
    UpsertExternalMcpServerRequest, UpsertPathRuleRequest, WorkspaceEntryMutationResultView,
    WorkspaceFileWriteResultView, WriteWorkspaceFileRequest,
};
use crate::contract::{GitReposInspectionView, SpawnWorkspacePeerRequest};

use crate::desktop_ops::{
    StagedAttachmentInfo, bootstrap_ssh_host_and_pair, discover_local_daemon_pairing_uri,
    extract_ws_port, normalize_relay_input, read_local_workspace_file,
    read_local_workspace_file_with_writable, stage_local_attachment,
};
use crate::render::{
    DiffFileView, HighlightedLineView, RichBlockView, TerminalGridEmulator, TerminalScreenView,
    detect_language_for_path, highlight_code_lines_capped, parse_markdown_blocks,
    parse_unified_diff,
};
use crate::stage::{self, SessionInsightsView, StageTurnView};
use crate::{
    ClientCoreError, ClientInboundEvent, HostConnectionState, HostRouteMode, HostSessionHandle,
    SavedHostProfile, connect_smart,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscribeOptions {
    pub min_interval_ms: u64,
    pub include_terminal_raw_bytes: bool,
}

impl Default for SubscribeOptions {
    fn default() -> Self {
        Self {
            min_interval_ms: 0,
            include_terminal_raw_bytes: true,
        }
    }
}

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
    #[serde(default)]
    pub daemon_protocol_version: Option<u32>,
    #[serde(default)]
    pub client_protocol_version: u32,
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
    #[serde(default)]
    pub supports_server_queue: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetOverviewView {
    pub revision: u64,
    pub total_pending_approvals: u32,
    pub hosts: Vec<HostCardView>,
    pub projects: Vec<ClientProjectView>,
    #[serde(default)]
    pub project_order: Vec<String>,
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
    #[serde(default)]
    pub fast_mode: bool,
    pub mode: SessionMode,
    pub status: AgentStatus,
    pub status_label: String,
    pub is_running: bool,
    #[serde(default)]
    pub has_background_tasks: bool,
    pub is_awaiting_permission: bool,
    pub parent_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_host_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub auto_archive: bool,
    #[serde(default)]
    pub attention: AttentionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at_rfc3339: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
    pub latest_seq: u64,
    pub updated_at_rfc3339: String,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub seen_seq: u64,
    #[serde(default)]
    pub read_seq: u64,
    #[serde(default)]
    pub is_unread: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queued_prompts: Vec<QueuedPromptView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_suggestion: Option<WorkspaceClusterSuggestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_at_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_agent_id: Option<String>,
}

pub struct CreateAgentRequest<'a> {
    pub host_id: &'a str,
    pub cwd: &'a str,
    pub extra_dirs: Vec<String>,
    pub provider: ProviderKind,
    pub acp_agent_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    pub fast_mode: Option<bool>,
    pub mode: SessionMode,
    pub initial_prompt: &'a str,
    pub initial_display_prompt: Option<String>,
    pub initial_badges: Vec<PromptBadge>,
    pub worktree_branch: Option<&'a str>,
    pub workspace_id: Option<&'a str>,
    pub handle: Option<String>,
    pub auto_cluster: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedSkillContentView {
    pub skill_id: String,
    pub content: String,
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertManagedSkillRequest {
    pub host_id: String,
    pub scope: SkillWriteScope,
    #[serde(default)]
    pub cwd: Option<String>,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub enabled: bool,
    #[serde(default)]
    pub expected_etag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportManagedSkillRequest {
    pub host_id: String,
    pub scope: SkillWriteScope,
    #[serde(default)]
    pub cwd: Option<String>,
    pub source_markdown_or_path: String,
    #[serde(default)]
    pub slug_override: Option<String>,
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
    #[serde(default)]
    pub profiles: Vec<AgentProfile>,
    #[serde(default)]
    pub schedules: Vec<ScheduleSpec>,
    #[serde(default)]
    pub conditional_timers: Vec<ConditionalTimerRecord>,
    #[serde(default)]
    pub connected_hosts: Vec<ConnectedHostSummary>,
    #[serde(default)]
    pub orchestration_config: OrchestrationConfig,
    #[serde(default)]
    pub orchestration_skills: Vec<OrchestrationSkillInfo>,
    #[serde(default)]
    pub managed_skills: Vec<ManagedSkillRecord>,
    #[serde(default)]
    pub keep_awake_status: Option<KeepAwakeStatus>,
    #[serde(default)]
    pub workspace_scripts: HashMap<String, Vec<WorkspaceScriptInfo>>,
    #[serde(default)]
    pub pending_permissions: Vec<PendingPermissionSummary>,
    #[serde(default)]
    pub provider_subagents: HashMap<String, Vec<ProviderSubagentDescriptor>>,
    #[serde(default)]
    pub git_repos_by_cwd: HashMap<String, GitReposInspectionView>,
    #[serde(default)]
    pub workspace_boards: HashMap<String, WorkspaceBoardSnapshot>,
    #[serde(default)]
    pub workspace_events: HashMap<String, Vec<WorkspaceEventRecord>>,
    #[serde(default)]
    pub workspace_organize_status: HashMap<String, WorkspaceOrganizeEvent>,
    #[serde(default)]
    pub host_sync_state: HostSyncState,
    #[serde(default)]
    pub acp_agents: Vec<AcpAgentAvailability>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactSummary>,
    #[serde(default)]
    pub goal_runs: Vec<GoalRunRecord>,
    #[serde(default)]
    pub monitor_watches: Vec<MonitorWatchRecord>,
    #[serde(default)]
    pub workflow_runs: Vec<WorkflowRunRecord>,
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
        background_task: Option<BackgroundTaskInfo>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guardian_assessment: Option<GuardianAssessment>,
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
    AgentMessage {
        seq: u64,
        message_id: String,
        kind: AgentMessageKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_agent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_host_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<String>,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission_context: Option<PermissionCheckpointRef>,
        blocks: Vec<RichBlockView>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianConfigSnapshotView {
    pub config: GuardianPolicyConfig,
    pub recent_audits: Vec<GuardianAuditEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImChannelsSnapshotView {
    pub channels: Vec<ImChannelRecord>,
    pub bindings: Vec<ImThreadBinding>,
    pub base_webhook_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImChannelTestResultView {
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct UpsertImChannelRequest {
    #[serde(default)]
    pub channel_id: Option<String>,
    pub name: String,
    pub platform: ImPlatform,
    pub enabled: bool,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub primary_secret: Option<String>,
    #[serde(default)]
    pub signing_secret: Option<String>,
    #[serde(default = "default_keep_existing_secrets")]
    pub keep_existing_secrets: bool,
    #[serde(default)]
    pub allowed_chat_ids: Vec<String>,
    #[serde(default)]
    pub allowed_user_ids: Vec<String>,
    pub default_workspace_cwd: String,
    pub default_provider: ProviderKind,
    pub default_mode: SessionMode,
    #[serde(default = "default_keep_existing_secrets")]
    pub forward_approvals: bool,
}

impl std::fmt::Debug for UpsertImChannelRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpsertImChannelRequest")
            .field("channel_id", &self.channel_id)
            .field("name", &self.name)
            .field("platform", &self.platform)
            .field("enabled", &self.enabled)
            .field("app_id", &self.app_id)
            .field(
                "primary_secret",
                &self.primary_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "signing_secret",
                &self.signing_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("keep_existing_secrets", &self.keep_existing_secrets)
            .field("allowed_chat_ids", &self.allowed_chat_ids)
            .field("allowed_user_ids", &self.allowed_user_ids)
            .field("default_workspace_cwd", &self.default_workspace_cwd)
            .field("default_provider", &self.default_provider)
            .field("default_mode", &self.default_mode)
            .field("forward_approvals", &self.forward_approvals)
            .finish()
    }
}

fn default_keep_existing_secrets() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushDevicesSnapshotView {
    pub devices: Vec<PushDeviceRecord>,
    pub preferences: PushPreferences,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushTestResultView {
    pub attempted: u32,
    pub delivered: u32,
    pub detail: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RegisterPushDeviceRequest {
    pub device_id: String,
    pub platform: PushPlatform,
    pub device_name: String,
    pub device_token: String,
    #[serde(default)]
    pub sandbox: bool,
}

impl std::fmt::Debug for RegisterPushDeviceRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisterPushDeviceRequest")
            .field("device_id", &self.device_id)
            .field("platform", &self.platform)
            .field("device_name", &self.device_name)
            .field("device_token", &"[REDACTED]")
            .field("sandbox", &self.sandbox)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct UpdatePushPreferencesRequest {
    pub preferences: PushPreferences,
    #[serde(default)]
    pub gateway_auth_token: Option<String>,
    #[serde(default = "default_keep_existing_secrets")]
    pub keep_existing_gateway_token: bool,
}

impl std::fmt::Debug for UpdatePushPreferencesRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdatePushPreferencesRequest")
            .field("preferences", &self.preferences)
            .field(
                "gateway_auth_token",
                &self.gateway_auth_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "keep_existing_gateway_token",
                &self.keep_existing_gateway_token,
            )
            .finish()
    }
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
    /// `"text"` | `"image"` | `"pdf"` | `"audio"` | `"video"` | `"binary"`
    pub content_kind: String,
    pub mime_type: Option<String>,
    pub content: String,
    pub size_bytes: u64,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_unix_ms: Option<i64>,
    #[serde(default)]
    pub writable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_ending: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskSideQuestionRequest {
    pub host_id: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    pub cwd: String,
    pub provider: ProviderKind,
    #[serde(default)]
    pub model: Option<String>,
    pub question: String,
    #[serde(default)]
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
    etag: Option<String>,
    mtime_unix_ms: Option<i64>,
    writable: bool,
    line_ending: Option<String>,
}

#[derive(Debug, Clone)]
struct AttachmentStageAck {
    completed: bool,
    path: Option<String>,
    filename: Option<String>,
    size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default)]
struct StageDirtyState {
    dirty_from_event_idx: Option<usize>,
    force_full: bool,
    meta_dirty: bool,
    aux_dirty: bool,
}

struct CachedAgentStage {
    rendered_by_event: Vec<Option<TimelineItemView>>,
    items: Vec<TimelineItemView>,
    event_to_item_idx: Vec<usize>,
    turns: Vec<StageTurnView>,
    turn_item_starts: Vec<usize>,
    pending_permissions: Vec<TimelineItemView>,
    pending_questions: Vec<TimelineItemView>,
    session_turn_diffs: Vec<DiffFileView>,
    insights: SessionInsightsView,
    stage_revision: u64,
    aux_revision: u64,
    last_reset_revision: u64,
    history_floor_revision: u64,
    delta_history: VecDeque<(u64, usize)>,
    latest_seq: u64,
}

type PendingArtifactListEntry = (
    bool,
    tokio::sync::oneshot::Sender<Result<Vec<ArtifactSummary>, String>>,
);

type PendingManagedSkillsListEntry = (
    Option<String>,
    tokio::sync::oneshot::Sender<Result<Vec<ManagedSkillRecord>, String>>,
);

type PendingSymbolSearchTx =
    tokio::sync::oneshot::Sender<Result<(Vec<orbit_protocol::WorkspaceSymbolEntry>, bool), String>>;

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
    reconnect_scheduled: bool,
    session_gen: u64,
    available_providers: Vec<ProviderKind>,
    session: Option<HostSessionHandle>,
    pending_outbox: Vec<WireMessage>,
    pending_queue_adds: HashMap<String, Vec<(Uuid, QueuedPromptView)>>,
    pending_queue_removals: HashMap<String, Vec<(Uuid, String)>>,
    pending_queue_reorders: HashMap<String, (Uuid, Vec<String>)>,
    pending_attachment_acks:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<AttachmentStageAck, String>>>,
    pending_file_reads:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<RawFileReadResponse, String>>>,
    pending_symbol_searches: HashMap<Uuid, PendingSymbolSearchTx>,
    pending_file_writes:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<WorkspaceFileWriteResultView, String>>>,
    pending_file_mutations: HashMap<
        Uuid,
        tokio::sync::oneshot::Sender<Result<WorkspaceEntryMutationResultView, String>>,
    >,
    pending_instruction_lists:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<InstructionFilesSnapshotView, String>>>,
    pending_instruction_reads:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<InstructionFileContentView, String>>>,
    pending_instruction_saves:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<InstructionFileSavedView, String>>>,
    pending_side_questions:
        HashMap<Uuid, tokio::sync::mpsc::UnboundedSender<SideQuestionStreamEvent>>,
    pending_rewinds:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<RewindAgentResultView, String>>>,
    pending_forks: HashMap<Uuid, tokio::sync::oneshot::Sender<Result<ForkAgentResultView, String>>>,
    pending_handoff_previews:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<WorktreeHandoffPreview, String>>>,
    pending_handoff_executes:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<WorktreeHandoffOutcome, String>>>,
    pending_daemon_upgrades: HashMap<Uuid, tokio::sync::oneshot::Sender<Result<(), String>>>,
    pending_upgrade_terminals: HashMap<Uuid, tokio::sync::oneshot::Sender<Result<String, String>>>,
    pending_git_repo_inspections:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<GitReposInspectionView, String>>>,
    pending_acp_probes: HashMap<Uuid, tokio::sync::oneshot::Sender<Result<AcpProbeResult, String>>>,
    pending_acp_snapshots: HashMap<
        Uuid,
        tokio::sync::oneshot::Sender<Result<crate::contract::AcpRegistryRefreshView, String>>,
    >,
    pending_mcp_snapshots:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<Vec<ExternalMcpServerRecord>, String>>>,
    pending_mcp_probes: HashMap<Uuid, tokio::sync::oneshot::Sender<Result<McpProbeResult, String>>>,
    pending_artifact_lists: HashMap<Uuid, PendingArtifactListEntry>,
    unsolicited_unfiltered_artifact_reqs: HashSet<Uuid>,
    pending_artifact_reads:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<ArtifactDetail, String>>>,
    pending_artifact_upserts:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<ArtifactSummary, String>>>,
    pending_artifact_deletes: HashMap<Uuid, tokio::sync::oneshot::Sender<Result<String, String>>>,
    pending_managed_skills_lists: HashMap<Uuid, PendingManagedSkillsListEntry>,
    background_managed_skills_reqs: HashSet<Uuid>,
    active_managed_skills_cwd: Option<String>,
    pending_managed_skill_reads:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<ManagedSkillContentView, String>>>,
    pending_path_rules:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<PathRulesSnapshotView, String>>>,
    pending_path_rule_matches:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<MatchedPathRulesView, String>>>,
    path_rules_by_cwd: HashMap<String, Vec<PathScopedRule>>,
    pending_lsp_servers:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<Vec<LspServerStatus>, String>>>,
    pending_lsp_symbols:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<Vec<LspSymbolInfo>, String>>>,
    pending_lsp_diagnostics:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<Vec<LspDiagnostic>, String>>>,
    pending_lsp_locations:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<Vec<LspLocation>, String>>>,
    pending_usage_insights:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<UsageInsightsSnapshot, String>>>,
    pending_context_occupancy:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<ContextOccupancyBreakdown, String>>>,
    pending_lifecycle_hooks:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<LifecycleHooksSnapshotView, String>>>,
    pending_lifecycle_hook_tests:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<HookExecutionRecord, String>>>,
    pending_timer_creates:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<ConditionalTimerRecord, String>>>,
    pending_timer_lists:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<Vec<ConditionalTimerRecord>, String>>>,
    pending_timer_updates:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<ConditionalTimerRecord, String>>>,
    background_timer_list_reqs: HashSet<Uuid>,
    pending_guardian_configs:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<GuardianConfigSnapshotView, String>>>,
    pending_goal_updates:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<GoalRunRecord, String>>>,
    pending_goal_lists:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<Vec<GoalRunRecord>, String>>>,
    pending_monitor_updates:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<MonitorWatchRecord, String>>>,
    pending_monitor_lists:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<Vec<MonitorWatchRecord>, String>>>,
    pending_event_triggers:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<EventTriggersSnapshotView, String>>>,
    pending_event_trigger_tests:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<EventTriggerDelivery, String>>>,
    pending_browser_snapshots:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<BrowserSessionsSnapshotView, String>>>,
    pending_browser_sessions:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<BrowserSessionSummary, String>>>,
    pending_browser_actions:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<BrowserActionResponseView, String>>>,
    pending_workflow_updates:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<WorkflowRunRecord, String>>>,
    pending_workflow_lists:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<Vec<WorkflowRunRecord>, String>>>,
    pending_im_channels:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<ImChannelsSnapshotView, String>>>,
    pending_im_channel_tests:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<ImChannelTestResultView, String>>>,
    pending_push_snapshots:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<PushDevicesSnapshotView, String>>>,
    pending_push_tests:
        HashMap<Uuid, tokio::sync::oneshot::Sender<Result<PushTestResultView, String>>>,
    rewound_agents: std::collections::HashSet<String>,
    caught_up_agents: std::collections::HashSet<String>,
    optimistic_by_req_id: HashMap<Uuid, String>,
    agent_req_by_req_id: HashMap<Uuid, String>,
    replaced_optimistic_agents: HashMap<String, String>,
    last_created_agent_id: Option<String>,
    initial_agents_hydrated: bool,
    host_sync_supported: bool,
    host_sync_state: HostSyncState,
    discovered_projects: Vec<DiscoveredProject>,
    directory_listing: Option<DirectoryListingView>,
    models_by_provider: HashMap<ProviderKind, Vec<ModelOption>>,
    workspaces: Vec<WorkspaceSummary>,
    agents: HashMap<String, AgentSummary>,
    timelines: HashMap<String, Vec<TimelineEvent>>,
    stage_cache: HashMap<String, CachedAgentStage>,
    stage_access_order: VecDeque<String>,
    stage_dirty: HashMap<String, StageDirtyState>,
    markdown_cache: HashMap<(String, u64, usize), Vec<RichBlockView>>,
    streaming_markdown_cache: HashMap<(String, String), (usize, Vec<RichBlockView>)>,
    quotas: Vec<ProviderQuotaSnapshot>,
    provider_catalogs: Vec<ProviderModelCatalog>,
    terminals: Vec<TerminalSummary>,
    active_terminal_id: Option<String>,
    active_terminal_slot: u8,
    slot_terminal_ids: HashMap<u8, String>,
    terminal_emulators: HashMap<u8, TerminalGridEmulator>,
    git_statuses: HashMap<String, GitStatusSnapshot>,
    git_diffs: HashMap<String, String>,
    git_repos_by_cwd: HashMap<String, GitReposInspectionView>,
    workspace_files: HashMap<String, Vec<String>>,
    workspace_boards: HashMap<String, WorkspaceBoardSnapshot>,
    workspace_events: HashMap<String, Vec<WorkspaceEventRecord>>,
    workspace_organize_status: HashMap<String, WorkspaceOrganizeEvent>,
    profiles: Vec<AgentProfile>,
    schedules: Vec<ScheduleSpec>,
    conditional_timers: Vec<ConditionalTimerRecord>,
    schedule_runs: HashMap<String, Vec<ScheduleRunRecord>>,
    connected_hosts: Vec<ConnectedHostSummary>,
    orchestration_config: OrchestrationConfig,
    orchestration_skills: Vec<OrchestrationSkillInfo>,
    managed_skills: Vec<ManagedSkillRecord>,
    keep_awake_status: Option<KeepAwakeStatus>,
    workspace_scripts: HashMap<String, Vec<WorkspaceScriptInfo>>,
    pending_permissions: Vec<PendingPermissionSummary>,
    provider_subagents: HashMap<String, Vec<ProviderSubagentDescriptor>>,
    provider_subagent_timelines: HashMap<(String, String), Vec<ProviderSubagentEvent>>,
    provider_subagent_access_order: VecDeque<(String, String)>,
    active_provider_subagent: Option<(String, String)>,
    subscribed_provider_subagents: HashSet<(String, String)>,
    acp_agents: Vec<AcpAgentAvailability>,
    artifacts: Vec<ArtifactSummary>,
    goal_runs: Vec<GoalRunRecord>,
    monitor_watches: Vec<MonitorWatchRecord>,
    workflow_runs: Vec<WorkflowRunRecord>,
}

impl HostMemoryState {
    fn new(profile: SavedHostProfile) -> Self {
        let mut terminal_emulators = HashMap::new();
        terminal_emulators.insert(1, TerminalGridEmulator::new(100, 30));
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
            reconnect_scheduled: false,
            session_gen: 0,
            available_providers: vec![
                ProviderKind::Antigravity,
                ProviderKind::Codex,
                ProviderKind::ClaudeCode,
            ],
            session: None,
            pending_outbox: Vec::new(),
            pending_queue_adds: HashMap::new(),
            pending_queue_removals: HashMap::new(),
            pending_queue_reorders: HashMap::new(),
            pending_attachment_acks: HashMap::new(),
            pending_file_reads: HashMap::new(),
            pending_symbol_searches: HashMap::new(),
            pending_file_writes: HashMap::new(),
            pending_file_mutations: HashMap::new(),
            pending_instruction_lists: HashMap::new(),
            pending_instruction_reads: HashMap::new(),
            pending_instruction_saves: HashMap::new(),
            pending_side_questions: HashMap::new(),
            pending_rewinds: HashMap::new(),
            pending_forks: HashMap::new(),
            pending_handoff_previews: HashMap::new(),
            pending_handoff_executes: HashMap::new(),
            pending_daemon_upgrades: HashMap::new(),
            pending_upgrade_terminals: HashMap::new(),
            pending_git_repo_inspections: HashMap::new(),
            pending_acp_probes: HashMap::new(),
            pending_acp_snapshots: HashMap::new(),
            pending_mcp_snapshots: HashMap::new(),
            pending_mcp_probes: HashMap::new(),
            pending_artifact_lists: HashMap::new(),
            unsolicited_unfiltered_artifact_reqs: HashSet::new(),
            pending_artifact_reads: HashMap::new(),
            pending_artifact_upserts: HashMap::new(),
            pending_artifact_deletes: HashMap::new(),
            pending_managed_skills_lists: HashMap::new(),
            background_managed_skills_reqs: HashSet::new(),
            active_managed_skills_cwd: None,
            pending_managed_skill_reads: HashMap::new(),
            pending_path_rules: HashMap::new(),
            pending_path_rule_matches: HashMap::new(),
            path_rules_by_cwd: HashMap::new(),
            pending_lsp_servers: HashMap::new(),
            pending_lsp_symbols: HashMap::new(),
            pending_lsp_diagnostics: HashMap::new(),
            pending_lsp_locations: HashMap::new(),
            pending_usage_insights: HashMap::new(),
            pending_context_occupancy: HashMap::new(),
            pending_lifecycle_hooks: HashMap::new(),
            pending_lifecycle_hook_tests: HashMap::new(),
            pending_timer_creates: HashMap::new(),
            pending_timer_lists: HashMap::new(),
            pending_timer_updates: HashMap::new(),
            background_timer_list_reqs: HashSet::new(),
            pending_guardian_configs: HashMap::new(),
            pending_goal_updates: HashMap::new(),
            pending_goal_lists: HashMap::new(),
            pending_monitor_updates: HashMap::new(),
            pending_monitor_lists: HashMap::new(),
            pending_event_triggers: HashMap::new(),
            pending_event_trigger_tests: HashMap::new(),
            pending_browser_snapshots: HashMap::new(),
            pending_browser_sessions: HashMap::new(),
            pending_browser_actions: HashMap::new(),
            pending_workflow_updates: HashMap::new(),
            pending_workflow_lists: HashMap::new(),
            pending_im_channels: HashMap::new(),
            pending_im_channel_tests: HashMap::new(),
            pending_push_snapshots: HashMap::new(),
            pending_push_tests: HashMap::new(),
            rewound_agents: std::collections::HashSet::new(),
            caught_up_agents: std::collections::HashSet::new(),
            optimistic_by_req_id: HashMap::new(),
            agent_req_by_req_id: HashMap::new(),
            replaced_optimistic_agents: HashMap::new(),
            last_created_agent_id: None,
            initial_agents_hydrated: false,
            host_sync_supported: false,
            host_sync_state: HostSyncState::default(),
            discovered_projects: Vec::new(),
            directory_listing: None,
            models_by_provider: default_client_models_map(),
            workspaces: Vec::new(),
            agents: HashMap::new(),
            timelines: HashMap::new(),
            stage_cache: HashMap::new(),
            stage_access_order: VecDeque::new(),
            stage_dirty: HashMap::new(),
            markdown_cache: HashMap::new(),
            streaming_markdown_cache: HashMap::new(),
            quotas: Vec::new(),
            provider_catalogs: Vec::new(),
            terminals: Vec::new(),
            active_terminal_id: None,
            active_terminal_slot: 1,
            slot_terminal_ids: HashMap::new(),
            terminal_emulators,
            git_statuses: HashMap::new(),
            git_diffs: HashMap::new(),
            git_repos_by_cwd: HashMap::new(),
            workspace_files: HashMap::new(),
            workspace_boards: HashMap::new(),
            workspace_events: HashMap::new(),
            workspace_organize_status: HashMap::new(),
            profiles: Vec::new(),
            schedules: Vec::new(),
            conditional_timers: Vec::new(),
            schedule_runs: HashMap::new(),
            connected_hosts: Vec::new(),
            orchestration_config: OrchestrationConfig::default(),
            orchestration_skills: Vec::new(),
            managed_skills: Vec::new(),
            keep_awake_status: None,
            workspace_scripts: HashMap::new(),
            pending_permissions: Vec::new(),
            provider_subagents: HashMap::new(),
            provider_subagent_timelines: HashMap::new(),
            provider_subagent_access_order: VecDeque::new(),
            active_provider_subagent: None,
            subscribed_provider_subagents: HashSet::new(),
            acp_agents: Vec::new(),
            artifacts: Vec::new(),
            goal_runs: Vec::new(),
            monitor_watches: Vec::new(),
            workflow_runs: Vec::new(),
        }
    }

    fn mark_stage_dirty(&mut self, agent_id: &str, event_idx: usize, force_full: bool) {
        self.mark_stage_dirty_with_aux(agent_id, event_idx, force_full, true);
    }

    fn mark_stage_dirty_with_aux(
        &mut self,
        agent_id: &str,
        event_idx: usize,
        force_full: bool,
        aux_dirty: bool,
    ) {
        let entry = self.stage_dirty.entry(agent_id.to_string()).or_default();
        entry.force_full |= force_full;
        entry.aux_dirty |= force_full || aux_dirty;
        entry.dirty_from_event_idx = Some(match entry.dirty_from_event_idx {
            Some(prev) => prev.min(event_idx),
            None => event_idx,
        });
    }

    fn mark_stage_meta_dirty(&mut self, agent_id: &str) {
        self.stage_dirty
            .entry(agent_id.to_string())
            .or_default()
            .meta_dirty = true;
    }

    fn touch_stage_lru(&mut self, agent_id: &str) {
        if let Some(pos) = self.stage_access_order.iter().position(|id| id == agent_id) {
            self.stage_access_order.remove(pos);
        }
        self.stage_access_order.push_back(agent_id.to_string());
    }

    fn is_agent_active_for_cache(&self, agent_id: &str) -> bool {
        self.agents.get(agent_id).is_some_and(|a| {
            matches!(
                a.status,
                AgentStatus::Initializing
                    | AgentStatus::Running { .. }
                    | AgentStatus::AutonomousWakeup { .. }
                    | AgentStatus::AwaitingPermission { .. }
            )
        })
    }

    fn touch_provider_subagent_lru(&mut self, key: &(String, String)) {
        if let Some(pos) = self
            .provider_subagent_access_order
            .iter()
            .position(|k| k == key)
        {
            self.provider_subagent_access_order.remove(pos);
        }
        self.provider_subagent_access_order.push_back(key.clone());
    }

    fn is_provider_subagent_running(&self, parent_agent_id: &str, subagent_id: &str) -> bool {
        self.provider_subagents
            .get(parent_agent_id)
            .is_some_and(|list| {
                list.iter().any(|ps| {
                    ps.subagent_id == subagent_id
                        && ps.status == orbit_protocol::ProviderSubagentStatus::Running
                })
            })
    }

    fn enforce_provider_subagent_timeline_bounds(&mut self, keep_key: Option<&(String, String)>) {
        const MAX_PROVIDER_SUBAGENT_TIMELINES_PER_HOST: usize = 8;

        // Drop stale access-order entries no longer present in the map.
        self.provider_subagent_access_order
            .retain(|k| self.provider_subagent_timelines.contains_key(k));

        if self.provider_subagent_timelines.len() <= MAX_PROVIDER_SUBAGENT_TIMELINES_PER_HOST {
            return;
        }

        // First evict oldest finished entries tracked in LRU order.
        let mut idx = 0;
        while self.provider_subagent_timelines.len() > MAX_PROVIDER_SUBAGENT_TIMELINES_PER_HOST
            && idx < self.provider_subagent_access_order.len()
        {
            let candidate = self.provider_subagent_access_order[idx].clone();
            let is_pinned = keep_key.is_some_and(|k| k == &candidate)
                || self
                    .active_provider_subagent
                    .as_ref()
                    .is_some_and(|active| active == &candidate);
            if is_pinned || self.is_provider_subagent_running(&candidate.0, &candidate.1) {
                idx += 1;
                continue;
            }
            self.provider_subagent_access_order.remove(idx);
            self.provider_subagent_timelines.remove(&candidate);
            self.subscribed_provider_subagents.remove(&candidate);
        }

        // Fallback: evict any untracked finished keys (e.g. inserted directly without LRU touch).
        if self.provider_subagent_timelines.len() > MAX_PROVIDER_SUBAGENT_TIMELINES_PER_HOST {
            let extra: Vec<(String, String)> = self
                .provider_subagent_timelines
                .keys()
                .filter(|k| {
                    !keep_key.is_some_and(|keep| keep == *k)
                        && !self
                            .active_provider_subagent
                            .as_ref()
                            .is_some_and(|active| active == *k)
                        && !self.is_provider_subagent_running(&k.0, &k.1)
                        && !self.provider_subagent_access_order.contains(*k)
                })
                .cloned()
                .collect();
            let mut to_remove = self
                .provider_subagent_timelines
                .len()
                .saturating_sub(MAX_PROVIDER_SUBAGENT_TIMELINES_PER_HOST);
            for k in extra {
                if to_remove == 0 {
                    break;
                }
                self.provider_subagent_timelines.remove(&k);
                self.subscribed_provider_subagents.remove(&k);
                to_remove -= 1;
            }
        }
    }

    fn enforce_memory_cache_bounds(&mut self, keep_agent_id: &str) {
        const MAX_CACHED_STAGES_PER_HOST: usize = 8;
        const MAX_CACHED_TIMELINES_PER_HOST: usize = 12;
        const MAX_MARKDOWN_CACHE_ENTRIES: usize = 2048;
        const MAX_STREAMING_MARKDOWN_CACHE_ENTRIES: usize = 256;

        if self.stage_cache.len() > MAX_CACHED_STAGES_PER_HOST {
            let mut idx = 0;
            while self.stage_cache.len() > MAX_CACHED_STAGES_PER_HOST
                && idx < self.stage_access_order.len()
            {
                let candidate = self.stage_access_order[idx].clone();
                if candidate == keep_agent_id || self.is_agent_active_for_cache(&candidate) {
                    idx += 1;
                    continue;
                }
                self.stage_access_order.remove(idx);
                self.stage_cache.remove(&candidate);
                self.stage_dirty.remove(&candidate);
            }
        }

        if self.timelines.len() > MAX_CACHED_TIMELINES_PER_HOST {
            let evict_candidates: Vec<String> = self
                .timelines
                .keys()
                .filter(|id| {
                    id.as_str() != keep_agent_id
                        && !id.starts_with("opt_")
                        && !self.stage_cache.contains_key(id.as_str())
                        && !self.is_agent_active_for_cache(id.as_str())
                })
                .cloned()
                .collect();
            let mut to_remove = self
                .timelines
                .len()
                .saturating_sub(MAX_CACHED_TIMELINES_PER_HOST);
            for id in evict_candidates {
                if to_remove == 0 {
                    break;
                }
                self.timelines.remove(&id);
                to_remove -= 1;
            }
        }

        if self.markdown_cache.len() > MAX_MARKDOWN_CACHE_ENTRIES {
            self.markdown_cache.clear();
        }
        if self.streaming_markdown_cache.len() > MAX_STREAMING_MARKDOWN_CACHE_ENTRIES {
            self.streaming_markdown_cache.clear();
        }
        self.enforce_provider_subagent_timeline_bounds(None);
    }

    fn emulator_slot_mut(&mut self, slot: u8) -> &mut TerminalGridEmulator {
        self.terminal_emulators
            .entry(slot)
            .or_insert_with(|| TerminalGridEmulator::new(100, 30))
    }

    fn to_stored_host(&self) -> StoredHostProfile {
        StoredHostProfile {
            host_id: self.profile.host_id.clone(),
            display_name: self.profile.hostname.clone(),
            daemon_pubkey_b64: self.profile.daemon_pubkey_b64.clone(),
            direct_endpoints: self.profile.direct_endpoints.clone(),
            relay_url: self.profile.relay_url.clone(),
            pairing_secret: self.profile.pairing_secret.clone(),
            meta: StoredHostMeta {
                custom_name: self.profile.custom_name.clone(),
                ssh_alias: self.profile.ssh_alias.clone(),
                route_mode: Some(self.profile.route_mode.as_str().to_string()),
                remote_daemon_port: Some(self.profile.remote_daemon_port),
                custom_direct_endpoint: self.profile.custom_direct_endpoint.clone(),
                pairing_secret_updated_at_ms: self.profile.pairing_secret_updated_at_ms,
            },
        }
    }

    fn to_card(&self) -> HostCardView {
        let pending_approval_count = self
            .agents
            .values()
            .filter(|a| {
                a.archived_at.is_none()
                    && matches!(a.status, AgentStatus::AwaitingPermission { .. })
            })
            .count() as u32;
        let active_workspace_count = self
            .workspaces
            .iter()
            .filter(|w| w.archived_at.is_none() && !w.background)
            .count() as u32;
        let active_agent_count = self
            .agents
            .values()
            .filter(|a| a.archived_at.is_none() && !a.background)
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
            daemon_protocol_version: self.daemon_protocol_version,
            client_protocol_version: orbit_protocol::PROTOCOL_VERSION,
            os_arch: self.os_arch.clone(),
            daemon_pubkey_b64: self.profile.daemon_pubkey_b64.clone(),
            direct_endpoints: self.profile.direct_endpoints.clone(),
            relay_url: self.profile.relay_url.clone(),
            connection_state: self.profile.state,
            last_error: self.last_error.clone(),
            available_providers: self.available_providers.clone(),
            workspace_count: active_workspace_count,
            agent_count: active_agent_count,
            pending_approval_count,
            supports_server_queue: self.daemon_protocol_version.is_some_and(|pv| pv >= 4),
        }
    }
}

/// Thread-safe, self-contained Rust Application Core with its own background Tokio runtime.
pub struct OrbitAppCore {
    runtime: tokio::runtime::Runtime,
    store: Arc<OrbitStore>,
    hosts: Arc<RwLock<HashMap<String, HostMemoryState>>>,
    projects: Arc<RwLock<HashMap<String, ClientProjectView>>>,
    project_order: Arc<RwLock<Vec<String>>>,
    thread_prefs: Arc<RwLock<HashMap<(String, String), ClientThreadPref>>>,
    pin_order_seq: Arc<AtomicU64>,
    subscribers: Arc<Mutex<Vec<Arc<dyn CoreEventSubscriber>>>>,
    revision: Arc<AtomicU64>,
    cloud_sync_client: Arc<RwLock<GitHubCloudClient>>,
    cloud_sync_in_flight: Arc<AtomicBool>,
    cloud_sync_last_auto_ms: Arc<AtomicU64>,
    cloud_sync_mutex: Arc<tokio::sync::Mutex<()>>,
}

impl OrbitAppCore {
    pub fn open(
        local_db_path: impl AsRef<Path>,
        auto_discover_local_daemon: bool,
    ) -> Result<Arc<Self>, ClientCoreError> {
        let store = Arc::new(OrbitStore::open(local_db_path)?);
        if let Ok(purged) = store.purge_legacy_client_timeline_events()
            && purged > 0
        {
            tracing::info!(
                purged_rows = purged,
                "Purged legacy daemon timeline_events rows from client replica database"
            );
        }
        Self::from_store(store, auto_discover_local_daemon)
    }

    pub fn open_in_memory() -> Result<Arc<Self>, ClientCoreError> {
        Self::open_in_memory_with_discovery(false)
    }

    pub fn open_in_memory_with_discovery(
        auto_discover_local_daemon: bool,
    ) -> Result<Arc<Self>, ClientCoreError> {
        let store = Arc::new(OrbitStore::open_in_memory()?);
        Self::from_store(store, auto_discover_local_daemon)
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

        let mut initial_prefs: HashMap<(String, String), ClientThreadPref> = HashMap::new();
        let mut max_pin_order = unix_ms_now();
        if let Ok(saved_prefs) = store.list_client_thread_prefs() {
            for pref in saved_prefs {
                if pref.pinned_order > 0 {
                    max_pin_order = max_pin_order.max(pref.pinned_order as u64);
                }
                initial_prefs.insert((pref.host_id.clone(), pref.agent_id.clone()), pref);
            }
        }

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
                    pairing_secret: h.pairing_secret,
                    pairing_secret_updated_at_ms: h.meta.pairing_secret_updated_at_ms,
                    state: HostConnectionState::Disconnected,
                };
                let mut mem = HostMemoryState::new(profile);
                if let Ok(ws_list) = store.list_replica_workspaces(&h.host_id) {
                    mem.workspaces = ws_list;
                }
                if let Ok(ag_list) = store.list_replica_agents(&h.host_id) {
                    if !ag_list.is_empty() {
                        mem.initial_agents_hydrated = true;
                    }
                    for ag in ag_list {
                        initial_prefs
                            .entry((h.host_id.clone(), ag.agent_id.clone()))
                            .or_insert_with(|| ClientThreadPref {
                                host_id: h.host_id.clone(),
                                agent_id: ag.agent_id.clone(),
                                seen_seq: ag.latest_seq,
                                is_pinned: false,
                                pinned_order: 0,
                                updated_at_ms: 0,
                            });
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
                    if existing.profile.pairing_secret != offer.pairing_secret {
                        existing.profile.pairing_secret = offer.pairing_secret;
                        existing.profile.pairing_secret_updated_at_ms = now_unix_ms();
                    } else if existing.profile.pairing_secret_updated_at_ms == 0 {
                        existing.profile.pairing_secret_updated_at_ms = now_unix_ms();
                    }
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
                    pairing_secret: offer.pairing_secret,
                    pairing_secret_updated_at_ms: now_unix_ms(),
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

        let initial_project_order: Vec<String> = store
            .get_client_kv("project_order")
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default();

        let core = Arc::new(Self {
            runtime,
            store,
            hosts: Arc::new(RwLock::new(initial_hosts)),
            projects: Arc::new(RwLock::new(initial_projects)),
            project_order: Arc::new(RwLock::new(initial_project_order)),
            thread_prefs: Arc::new(RwLock::new(initial_prefs)),
            pin_order_seq: Arc::new(AtomicU64::new(max_pin_order.saturating_add(1))),
            subscribers: Arc::new(Mutex::new(Vec::new())),
            revision: Arc::new(AtomicU64::new(1)),
            cloud_sync_client: Arc::new(RwLock::new(GitHubCloudClient::new())),
            cloud_sync_in_flight: Arc::new(AtomicBool::new(false)),
            cloud_sync_last_auto_ms: Arc::new(AtomicU64::new(0)),
            cloud_sync_mutex: Arc::new(tokio::sync::Mutex::new(())),
        });
        if !core.store.is_replica_fts_backfill_done() {
            let store_bg = Arc::clone(&core.store);
            core.runtime.spawn(async move {
                const BACKFILL_BATCH_SIZE: usize = 200;
                const MAX_CONSECUTIVE_ERRORS: u32 = 5;
                let mut consecutive_errors: u32 = 0;
                loop {
                    let store_step = Arc::clone(&store_bg);
                    let step = tokio::task::spawn_blocking(move || {
                        store_step.backfill_replica_fts_batch(BACKFILL_BATCH_SIZE)
                    })
                    .await;
                    match step {
                        Ok(Ok(progress)) => {
                            consecutive_errors = 0;
                            if progress.done {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(15)).await;
                        }
                        Ok(Err(err)) => {
                            consecutive_errors += 1;
                            tracing::warn!(
                                error = %err,
                                attempt = consecutive_errors,
                                "replica FTS5 backfill batch failed"
                            );
                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                tracing::warn!(
                                    "aborting replica FTS5 background backfill after repeated errors"
                                );
                                break;
                            }
                            let backoff_ms = 50u64
                                .saturating_mul(1u64 << (consecutive_errors - 1).min(4))
                                .min(1000);
                            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                        }
                        Err(join_err) => {
                            tracing::warn!(
                                error = %join_err,
                                "replica FTS5 backfill worker task aborted"
                            );
                            break;
                        }
                    }
                }
            });
        }
        Ok(core)
    }

    pub fn subscribe(&self, sub: Arc<dyn CoreEventSubscriber>) {
        self.subscribe_with_options(SubscribeOptions::default(), sub);
    }

    pub fn subscribe_coalesced(&self, min_interval_ms: u64, sub: Arc<dyn CoreEventSubscriber>) {
        self.subscribe_with_options(
            SubscribeOptions {
                min_interval_ms,
                include_terminal_raw_bytes: false,
            },
            sub,
        );
    }

    pub fn subscribe_with_options(
        &self,
        options: SubscribeOptions,
        sub: Arc<dyn CoreEventSubscriber>,
    ) {
        let wrapped: Arc<dyn CoreEventSubscriber> = if options.min_interval_ms == 0 {
            if options.include_terminal_raw_bytes {
                sub
            } else {
                Arc::new(StripTerminalBytesSubscriber { downstream: sub })
            }
        } else {
            Arc::new(CoalescingSubscriber::new(
                sub,
                self.runtime.handle().clone(),
                Duration::from_millis(options.min_interval_ms),
                options.include_terminal_raw_bytes,
            ))
        };
        if let Ok(mut guard) = self.subscribers.lock() {
            guard.push(wrapped);
        }
    }

    pub fn replace_subscribers(
        &self,
        options: SubscribeOptions,
        sub: Arc<dyn CoreEventSubscriber>,
    ) {
        let wrapped: Arc<dyn CoreEventSubscriber> = if options.min_interval_ms == 0 {
            if options.include_terminal_raw_bytes {
                sub
            } else {
                Arc::new(StripTerminalBytesSubscriber { downstream: sub })
            }
        } else {
            Arc::new(CoalescingSubscriber::new(
                sub,
                self.runtime.handle().clone(),
                Duration::from_millis(options.min_interval_ms),
                options.include_terminal_raw_bytes,
            ))
        };
        if let Ok(mut guard) = self.subscribers.lock() {
            guard.clear();
            guard.push(wrapped);
        }
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

        let project_order = self
            .project_order
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();

        FleetOverviewView {
            revision: rev,
            total_pending_approvals,
            hosts: cards,
            projects,
            project_order,
        }
    }

    pub fn host_detail(&self, host_id: &str) -> Option<HostDetailView> {
        let rev = self.revision.load(Ordering::SeqCst);
        let guard = self.hosts.read().ok()?;
        let state = guard.get(host_id)?;
        let prefs_guard = self.thread_prefs.read().ok();
        let mut agents: Vec<AgentCardView> = state
            .agents
            .values()
            .map(|a| {
                let pref = prefs_guard
                    .as_ref()
                    .and_then(|p| p.get(&(host_id.to_string(), a.agent_id.clone())));
                agent_to_card_view(a, pref)
            })
            .collect();
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
            profiles: state.profiles.clone(),
            schedules: state.schedules.clone(),
            conditional_timers: state.conditional_timers.clone(),
            connected_hosts: state.connected_hosts.clone(),
            orchestration_config: state.orchestration_config.clone(),
            orchestration_skills: state.orchestration_skills.clone(),
            managed_skills: state.managed_skills.clone(),
            keep_awake_status: state.keep_awake_status.clone(),
            workspace_scripts: state.workspace_scripts.clone(),
            pending_permissions: state.pending_permissions.clone(),
            provider_subagents: state.provider_subagents.clone(),
            git_repos_by_cwd: state.git_repos_by_cwd.clone(),
            workspace_boards: state.workspace_boards.clone(),
            workspace_events: state.workspace_events.clone(),
            workspace_organize_status: state.workspace_organize_status.clone(),
            host_sync_state: state.host_sync_state.clone(),
            acp_agents: state.acp_agents.clone(),
            artifacts: state.artifacts.clone(),
            goal_runs: state.goal_runs.clone(),
            monitor_watches: state.monitor_watches.clone(),
            workflow_runs: state.workflow_runs.clone(),
        })
    }

    fn hydrate_timeline_if_missing(
        &self,
        host_state: &mut HostMemoryState,
        agent_id: &str,
    ) -> bool {
        if !host_state.timelines.contains_key(agent_id)
            && !agent_id.starts_with("opt_")
            && let Ok(cached) = self.store.fetch_replica_timeline(agent_id, 50_000)
            && !cached.is_empty()
        {
            let mut coalesced = Vec::with_capacity(cached.len());
            for ev in cached {
                coalesce_or_push_timeline(&mut coalesced, ev);
            }
            host_state.timelines.insert(agent_id.to_string(), coalesced);
            host_state.mark_stage_dirty(agent_id, 0, true);
            return true;
        }
        false
    }

    fn ensure_stage_fresh_and_read<R>(
        &self,
        host_id: &str,
        agent_id: &str,
        reader: impl FnOnce(Option<&AgentCardView>, Option<&CachedAgentStage>) -> R,
    ) -> R {
        // Fast path: serve warm, clean stage cache under a shared read lock.
        if let Ok(read_guard) = self.hosts.read()
            && let Some(host_state) = read_guard.get(host_id)
            && !host_state.stage_dirty.contains_key(agent_id)
            && let Some(cached) = host_state.stage_cache.get(agent_id)
        {
            let prefs_guard = self.thread_prefs.read().ok();
            let current_agent_card = host_state.agents.get(agent_id).map(|ag| {
                let pref = prefs_guard
                    .as_ref()
                    .and_then(|p| p.get(&(host_id.to_string(), ag.agent_id.clone())));
                agent_to_card_view(ag, pref)
            });
            return reader(current_agent_card.as_ref(), Some(cached));
        }

        let Ok(mut guard) = self.hosts.write() else {
            return reader(None, None);
        };
        let Some(host_state) = guard.get_mut(host_id) else {
            return reader(None, None);
        };

        self.hydrate_timeline_if_missing(host_state, agent_id);

        let prefs_guard = self.thread_prefs.read().ok();
        let card_for = |ag: &AgentSummary| -> AgentCardView {
            let pref = prefs_guard
                .as_ref()
                .and_then(|p| p.get(&(host_id.to_string(), ag.agent_id.clone())));
            agent_to_card_view(ag, pref)
        };

        let current_agent_card = host_state.agents.get(agent_id).map(&card_for);
        let dirty = host_state.stage_dirty.remove(agent_id);
        let needs_build = !host_state.stage_cache.contains_key(agent_id);

        if needs_build || dirty.is_some() {
            let host_agent_cards: Vec<AgentCardView> =
                host_state.agents.values().map(&card_for).collect();
            let raw_events: &[TimelineEvent] = host_state
                .timelines
                .get(agent_id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let cwd_owned = current_agent_card.as_ref().map(|a| a.cwd.clone());
            let cwd = cwd_owned.as_deref();
            let latest_seq = raw_events
                .iter()
                .map(|e| e.seq)
                .max()
                .filter(|&s| s > 0)
                .unwrap_or_else(|| {
                    current_agent_card
                        .as_ref()
                        .map(|a| a.latest_seq)
                        .unwrap_or(0)
                });

            let dirty_state = dirty.unwrap_or(StageDirtyState {
                dirty_from_event_idx: Some(0),
                force_full: true,
                meta_dirty: true,
                aux_dirty: true,
            });
            let (all_resolved_perms, all_resolved_questions) = collect_resolved_gates(raw_events);

            let force_full = needs_build
                || dirty_state.force_full
                || dirty_state.dirty_from_event_idx == Some(0)
                || host_state
                    .stage_cache
                    .get(agent_id)
                    .is_some_and(|c| raw_events.len() < c.rendered_by_event.len());

            if force_full {
                let (prev_rev, prev_aux_rev) = host_state
                    .stage_cache
                    .get(agent_id)
                    .map(|c| (c.stage_revision, c.aux_revision))
                    .unwrap_or((0, 0));
                let next_rev = prev_rev.saturating_add(1).max(1);
                let next_aux_rev = prev_aux_rev.saturating_add(1).max(1);

                let mut rendered_by_event = Vec::with_capacity(raw_events.len());
                let mut items = Vec::with_capacity(raw_events.len());
                let mut event_to_item_idx = Vec::with_capacity(raw_events.len());
                for ev in raw_events {
                    event_to_item_idx.push(items.len());
                    let rendered = render_timeline_event_view(
                        agent_id,
                        cwd,
                        ev,
                        &all_resolved_perms,
                        &all_resolved_questions,
                        &mut host_state.markdown_cache,
                        &mut host_state.streaming_markdown_cache,
                    );
                    if let Some(ref item) = rendered {
                        items.push(item.clone());
                    }
                    rendered_by_event.push(rendered);
                }

                let sliced = stage::process_timeline_turns_slice(&items, 0, 0, cwd);
                let provider_subagents = host_state
                    .provider_subagents
                    .get(agent_id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let (pending_permissions, pending_questions, session_turn_diffs, insights) =
                    stage::collect_stage_auxiliary_state(
                        &items,
                        cwd,
                        current_agent_card.as_ref(),
                        &host_agent_cards,
                        provider_subagents,
                    );

                host_state.stage_cache.insert(
                    agent_id.to_string(),
                    CachedAgentStage {
                        rendered_by_event,
                        items,
                        event_to_item_idx,
                        turns: sliced.turns,
                        turn_item_starts: sliced.turn_item_starts,
                        pending_permissions,
                        pending_questions,
                        session_turn_diffs,
                        insights,
                        stage_revision: next_rev,
                        aux_revision: next_aux_rev,
                        last_reset_revision: next_rev,
                        history_floor_revision: next_rev,
                        delta_history: VecDeque::new(),
                        latest_seq,
                    },
                );
            } else if let Some(cached) = host_state.stage_cache.get_mut(agent_id) {
                let mut replace_from_turn_idx = cached.turns.len();
                if let Some(dirty_idx) = dirty_state.dirty_from_event_idx {
                    let clamped_idx = dirty_idx.min(cached.rendered_by_event.len());
                    let base_item_idx = if clamped_idx < cached.event_to_item_idx.len() {
                        cached.event_to_item_idx[clamped_idx]
                    } else {
                        cached.items.len()
                    };
                    cached.rendered_by_event.truncate(clamped_idx);
                    cached.event_to_item_idx.truncate(clamped_idx);
                    cached.items.truncate(base_item_idx);

                    for ev in raw_events.iter().skip(clamped_idx) {
                        cached.event_to_item_idx.push(cached.items.len());
                        let rendered = render_timeline_event_view(
                            agent_id,
                            cwd,
                            ev,
                            &all_resolved_perms,
                            &all_resolved_questions,
                            &mut host_state.markdown_cache,
                            &mut host_state.streaming_markdown_cache,
                        );
                        if let Some(ref item) = rendered {
                            cached.items.push(item.clone());
                        }
                        cached.rendered_by_event.push(rendered);
                    }

                    replace_from_turn_idx = if cached.turn_item_starts.is_empty() {
                        0
                    } else {
                        cached
                            .turn_item_starts
                            .partition_point(|&start| start <= base_item_idx)
                            .saturating_sub(1)
                    };
                    let start_item_idx = if replace_from_turn_idx == 0 {
                        0
                    } else {
                        cached.turn_item_starts[replace_from_turn_idx]
                    };
                    cached.turns.truncate(replace_from_turn_idx);
                    cached.turn_item_starts.truncate(replace_from_turn_idx);

                    let sliced = stage::process_timeline_turns_slice(
                        &cached.items,
                        start_item_idx,
                        replace_from_turn_idx as u32,
                        cwd,
                    );
                    cached.turns.extend(sliced.turns);
                    cached.turn_item_starts.extend(sliced.turn_item_starts);
                }

                if dirty_state.aux_dirty {
                    let provider_subagents = host_state
                        .provider_subagents
                        .get(agent_id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let (pending_permissions, pending_questions, session_turn_diffs, insights) =
                        stage::collect_stage_auxiliary_state(
                            &cached.items,
                            cwd,
                            current_agent_card.as_ref(),
                            &host_agent_cards,
                            provider_subagents,
                        );
                    cached.pending_permissions = pending_permissions;
                    cached.pending_questions = pending_questions;
                    cached.session_turn_diffs = session_turn_diffs;
                    cached.insights = insights;
                    cached.aux_revision = cached.aux_revision.saturating_add(1);
                } else if dirty_state.meta_dirty {
                    let provider_subagents = host_state
                        .provider_subagents
                        .get(agent_id)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let next_insights = stage::derive_session_insights(
                        &cached.items,
                        cwd,
                        current_agent_card.as_ref(),
                        &host_agent_cards,
                        provider_subagents,
                    );
                    if next_insights != cached.insights {
                        cached.insights = next_insights;
                        cached.aux_revision = cached.aux_revision.saturating_add(1);
                    }
                }
                cached.latest_seq = latest_seq;
                cached.stage_revision = cached.stage_revision.saturating_add(1);
                cached.delta_history.push_back((
                    cached.stage_revision,
                    replace_from_turn_idx.min(cached.turns.len()),
                ));
                while cached.delta_history.len() > 64 {
                    if let Some((evicted_rev, _)) = cached.delta_history.pop_front() {
                        cached.history_floor_revision =
                            cached.history_floor_revision.max(evicted_rev);
                    }
                }
            }
            host_state.touch_stage_lru(agent_id);
            host_state.enforce_memory_cache_bounds(agent_id);
        }

        let cached_ref = host_state.stage_cache.get(agent_id);
        reader(current_agent_card.as_ref(), cached_ref)
    }

    pub fn agent_stage(&self, host_id: &str, agent_id: &str) -> crate::stage::AgentStageView {
        self.ensure_stage_fresh_and_read(host_id, agent_id, |agent, cached| match cached {
            Some(c) => crate::stage::AgentStageView {
                revision: c.stage_revision,
                host_id: host_id.to_string(),
                agent: agent.cloned(),
                turns: c.turns.clone(),
                pending_permissions: c.pending_permissions.clone(),
                pending_questions: c.pending_questions.clone(),
                session_turn_diffs: c.session_turn_diffs.clone(),
                insights: c.insights.clone(),
                latest_seq: c.latest_seq,
            },
            None => crate::stage::AgentStageView {
                revision: 0,
                host_id: host_id.to_string(),
                agent: agent.cloned(),
                turns: Vec::new(),
                pending_permissions: Vec::new(),
                pending_questions: Vec::new(),
                session_turn_diffs: Vec::new(),
                insights: crate::stage::SessionInsightsView::default(),
                latest_seq: agent.map(|a| a.latest_seq).unwrap_or(0),
            },
        })
    }

    pub fn agent_stage_delta(
        &self,
        host_id: &str,
        agent_id: &str,
        since_revision: u64,
    ) -> crate::stage::AgentStageDeltaView {
        self.agent_stage_delta_with_aux(host_id, agent_id, since_revision, None)
    }

    pub fn agent_stage_delta_with_aux(
        &self,
        host_id: &str,
        agent_id: &str,
        since_revision: u64,
        since_aux_revision: Option<u64>,
    ) -> crate::stage::AgentStageDeltaView {
        self.ensure_stage_fresh_and_read(host_id, agent_id, |agent, cached| {
            let Some(c) = cached else {
                return crate::stage::AgentStageDeltaView {
                    revision: 0,
                    host_id: host_id.to_string(),
                    agent: agent.cloned(),
                    replace_from_turn_index: 0,
                    changed_turns: Vec::new(),
                    total_turn_count: 0,
                    pending_permissions: Vec::new(),
                    pending_questions: Vec::new(),
                    session_turn_diffs: Vec::new(),
                    insights: crate::stage::SessionInsightsView::default(),
                    latest_seq: agent.map(|a| a.latest_seq).unwrap_or(0),
                    reset: since_revision > 0,
                    aux_revision: 0,
                    aux_unchanged: false,
                };
            };

            let total_turns = c.turns.len();
            if since_revision == c.stage_revision {
                let aux_unchanged =
                    matches!(since_aux_revision, Some(prev_aux) if prev_aux == c.aux_revision);
                return crate::stage::AgentStageDeltaView {
                    revision: c.stage_revision,
                    host_id: host_id.to_string(),
                    agent: agent.cloned(),
                    replace_from_turn_index: total_turns as u32,
                    changed_turns: Vec::new(),
                    total_turn_count: total_turns as u32,
                    pending_permissions: if aux_unchanged {
                        Vec::new()
                    } else {
                        c.pending_permissions.clone()
                    },
                    pending_questions: if aux_unchanged {
                        Vec::new()
                    } else {
                        c.pending_questions.clone()
                    },
                    session_turn_diffs: if aux_unchanged {
                        Vec::new()
                    } else {
                        c.session_turn_diffs.clone()
                    },
                    insights: if aux_unchanged {
                        crate::stage::SessionInsightsView::default()
                    } else {
                        c.insights.clone()
                    },
                    latest_seq: c.latest_seq,
                    reset: false,
                    aux_revision: c.aux_revision,
                    aux_unchanged,
                };
            }

            let needs_reset = since_revision == 0
                || since_revision > c.stage_revision
                || since_revision < c.last_reset_revision
                || since_revision < c.history_floor_revision;

            let replace_from = if needs_reset {
                0
            } else {
                c.delta_history
                    .iter()
                    .filter(|(rev, _)| *rev > since_revision)
                    .map(|(_, idx)| *idx)
                    .min()
                    .unwrap_or(total_turns)
                    .min(total_turns)
            };

            let aux_unchanged = !needs_reset
                && matches!(since_aux_revision, Some(prev_aux) if prev_aux == c.aux_revision);

            crate::stage::AgentStageDeltaView {
                revision: c.stage_revision,
                host_id: host_id.to_string(),
                agent: agent.cloned(),
                replace_from_turn_index: replace_from as u32,
                changed_turns: c.turns[replace_from..].to_vec(),
                total_turn_count: total_turns as u32,
                pending_permissions: if aux_unchanged {
                    Vec::new()
                } else {
                    c.pending_permissions.clone()
                },
                pending_questions: if aux_unchanged {
                    Vec::new()
                } else {
                    c.pending_questions.clone()
                },
                session_turn_diffs: if aux_unchanged {
                    Vec::new()
                } else {
                    c.session_turn_diffs.clone()
                },
                insights: if aux_unchanged {
                    crate::stage::SessionInsightsView::default()
                } else {
                    c.insights.clone()
                },
                latest_seq: c.latest_seq,
                reset: needs_reset,
                aux_revision: c.aux_revision,
                aux_unchanged,
            }
        })
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
        self.hydrate_timeline_if_missing(host_state, agent_id);

        let agent_card = {
            let prefs_guard = self.thread_prefs.read().ok();
            host_state.agents.get(agent_id).map(|a| {
                let pref = prefs_guard
                    .as_ref()
                    .and_then(|p| p.get(&(host_id.to_string(), a.agent_id.clone())));
                agent_to_card_view(a, pref)
            })
        };
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
            if let Some(item) = render_timeline_event_view(
                agent_id,
                agent_cwd.as_deref(),
                ev,
                &all_resolved_perms,
                &all_resolved_questions,
                &mut host_state.markdown_cache,
                &mut host_state.streaming_markdown_cache,
            ) {
                upserted_items.push(item);
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
        let (raw_diff, status) = {
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
            (raw_diff, status)
        };
        // Parse & syntax-highlight diff outside `self.hosts` read lock so inbound WS/PTY never stalls.
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
        let slot = state.active_terminal_slot;
        Some(
            state
                .terminal_emulators
                .get(&slot)
                .map(TerminalGridEmulator::snapshot)
                .unwrap_or_else(|| TerminalGridEmulator::new(100, 30).snapshot()),
        )
    }

    pub fn terminal_screen_slot(&self, host_id: &str, slot: u8) -> Option<TerminalScreenView> {
        let guard = self.hosts.read().ok()?;
        let state = guard.get(host_id)?;
        Some(
            state
                .terminal_emulators
                .get(&slot.max(1))
                .map(TerminalGridEmulator::snapshot)
                .unwrap_or_else(|| TerminalGridEmulator::new(100, 30).snapshot()),
        )
    }

    /// Unified sidebar + Launchpad snapshot computed centrally in Rust so Desktop, iOS,
    /// Android, and HarmonyOS share identical project grouping, worktree deduplication,
    /// unread/attention triage, pinned threads, and active git branch resolution.
    pub fn fleet_sidebar(
        &self,
        sort: crate::sidebar::SidebarProjectSort,
        filter: crate::sidebar::SidebarThreadFilter,
        query: &str,
        active_host_id: Option<&str>,
        active_agent_id: Option<&str>,
        active_cwd: Option<&str>,
    ) -> crate::sidebar::FleetSidebarView {
        let rev = self.revision.load(Ordering::SeqCst);
        let hosts_guard = self.hosts.read().ok();
        let projs_guard = self.projects.read().ok();
        let prefs_guard = self.thread_prefs.read().ok();

        let pinned_projects: Vec<ClientProjectView> = projs_guard
            .as_ref()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default();
        let project_order: Vec<String> = self
            .project_order
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();

        struct OwnedSidebarHostData {
            host_id: String,
            workspaces: Vec<WorkspaceSummary>,
            discovered_projects: Vec<DiscoveredProject>,
            agents: Vec<(AgentCardView, i64)>,
            active_git_branch: Option<String>,
            git_repos_by_cwd: HashMap<String, GitReposInspectionView>,
            workspace_conflict_ids: HashSet<String>,
        }
        let mut host_inputs_owned: Vec<OwnedSidebarHostData> = Vec::new();
        let conflict_cutoff_ms = (unix_ms_now() as i64).saturating_sub(15 * 60 * 1000);

        if let Some(ref hg) = hosts_guard {
            for (hid, state) in hg.iter() {
                let agents_with_order: Vec<(AgentCardView, i64)> = state
                    .agents
                    .values()
                    .map(|a| {
                        let pref = prefs_guard
                            .as_ref()
                            .and_then(|p| p.get(&(hid.clone(), a.agent_id.clone())));
                        let order = pref
                            .map(|p| p.pinned_order)
                            .unwrap_or(0)
                            .max(a.pinned_order);
                        (agent_to_card_view(a, pref), order)
                    })
                    .collect();

                let active_branch = if active_host_id == Some(hid.as_str()) {
                    active_cwd.and_then(|cwd| {
                        let gs = state.git_statuses.get(cwd).or_else(|| {
                            state
                                .git_statuses
                                .iter()
                                .find(|(k, _)| cwd_paths_equivalent(k, cwd))
                                .map(|(_, v)| v)
                        });
                        crate::sidebar::active_git_branch_from_state(
                            gs,
                            &state.workspaces,
                            &state.discovered_projects,
                            cwd,
                        )
                    })
                } else {
                    None
                };

                let workspace_conflict_ids: HashSet<String> = state
                    .workspace_events
                    .iter()
                    .filter_map(|(ws_id, evs)| {
                        let recent_conflict = evs.iter().any(|ev| {
                            ev.kind == WorkspaceEventKind::ConflictWarning
                                && ev.created_at.timestamp_millis() >= conflict_cutoff_ms
                        });
                        recent_conflict.then(|| ws_id.clone())
                    })
                    .collect();

                host_inputs_owned.push(OwnedSidebarHostData {
                    host_id: hid.clone(),
                    workspaces: state.workspaces.clone(),
                    discovered_projects: state.discovered_projects.clone(),
                    agents: agents_with_order,
                    active_git_branch: active_branch,
                    git_repos_by_cwd: state.git_repos_by_cwd.clone(),
                    workspace_conflict_ids,
                });
            }
        }

        let host_inputs: Vec<crate::sidebar::SidebarHostInput<'_>> = host_inputs_owned
            .iter()
            .map(|item| crate::sidebar::SidebarHostInput {
                host_id: item.host_id.as_str(),
                workspaces: item.workspaces.as_slice(),
                discovered_projects: item.discovered_projects.as_slice(),
                agents: item.agents.clone(),
                active_git_branch: item.active_git_branch.clone(),
                git_repos_by_cwd: &item.git_repos_by_cwd,
                workspace_conflict_ids: &item.workspace_conflict_ids,
            })
            .collect();

        crate::sidebar::build_fleet_sidebar(crate::sidebar::FleetSidebarParams {
            revision: rev,
            hosts: host_inputs,
            pinned_projects: &pinned_projects,
            project_order: &project_order,
            sort,
            filter,
            query,
            active_host_id,
            active_agent_id,
            active_cwd,
        })
    }

    pub fn fallback_provider_catalogs(&self) -> Vec<ProviderModelCatalog> {
        ProviderKind::all()
            .iter()
            .copied()
            .map(crate::catalog::fallback_provider_catalog)
            .collect()
    }

    pub fn bridge_info(&self) -> crate::contract::BridgeInfoView {
        crate::contract::BridgeInfoView {
            protocol_version: crate::contract::CLIENT_BRIDGE_PROTOCOL_VERSION,
            core_version: env!("CARGO_PKG_VERSION").to_string(),
            capabilities: vec![
                "agent_stage_delta".into(),
                "fleet_sidebar".into(),
                "thread_prefs".into(),
                "model_selection".into(),
                "composer_v2".into(),
                "mention_ranking".into(),
                "multislot_terminal".into(),
                "side_question".into(),
                "rewind".into(),
                "native_diagrams".into(),
                "fts5_search".into(),
                "transcript_export".into(),
            ],
        }
    }

    fn load_full_agent_export_context(
        &self,
        host_id: &str,
        agent_id: &str,
    ) -> Result<(crate::export::ExportAgentMeta, Vec<TimelineEvent>), String> {
        let mut meta = crate::export::ExportAgentMeta {
            host_id: host_id.to_string(),
            agent_id: agent_id.to_string(),
            title: String::new(),
            ..Default::default()
        };
        let mut mem_events: Vec<TimelineEvent> = Vec::new();
        let mut found_agent = false;

        if let Ok(guard) = self.hosts.read()
            && let Some(host_state) = guard.get(host_id)
        {
            let hname = host_state.profile.effective_display_name();
            if !hname.trim().is_empty() {
                meta.host_name = Some(hname);
            }
            if let Some(ag) = host_state.agents.get(agent_id) {
                found_agent = true;
                meta.title = ag.title.clone();
                meta.provider = Some(ag.provider);
                meta.model = ag.model.clone();
                meta.cwd = Some(ag.cwd.clone());
                meta.task_summary = ag.task_summary.clone();
                meta.created_at = Some(ag.created_at);
                meta.workspace_title = host_state
                    .workspaces
                    .iter()
                    .find(|w| w.workspace_id == ag.workspace_id)
                    .map(|w| w.title.clone());
            }
            if let Some(list) = host_state.timelines.get(agent_id) {
                mem_events = list.clone();
            }
        }

        // Fall back to persisted replica metadata if this host/agent is not yet hydrated in memory.
        if !found_agent
            && let Ok(replica_agents) = self.store.list_replica_agents(host_id)
            && let Some(ag) = replica_agents.into_iter().find(|a| a.agent_id == agent_id)
        {
            found_agent = true;
            meta.title = ag.title.clone();
            meta.provider = Some(ag.provider);
            meta.model = ag.model.clone();
            meta.cwd = Some(ag.cwd.clone());
            meta.task_summary = ag.task_summary.clone();
            meta.created_at = Some(ag.created_at);
            if let Ok(ws_list) = self.store.list_replica_workspaces(host_id) {
                meta.workspace_title = ws_list
                    .into_iter()
                    .find(|w| w.workspace_id == ag.workspace_id)
                    .map(|w| w.title);
            }
            if meta.host_name.is_none()
                && let Ok(saved_hosts) = self.store.list_client_hosts()
                && let Some(h) = saved_hosts.into_iter().find(|h| h.host_id == host_id)
            {
                let label = h
                    .meta
                    .custom_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        h.meta
                            .ssh_alias
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                    })
                    .unwrap_or(h.display_name.trim());
                if !label.is_empty() {
                    meta.host_name = Some(label.to_string());
                }
            }
        }

        // Strict multi-host isolation: NEVER query replica events unless this `host_id` owns `agent_id`.
        if !found_agent && mem_events.is_empty() {
            return Err(format!(
                "conversation `{agent_id}` not found on host `{host_id}`"
            ));
        }

        // Read the complete persisted local replica timeline so long virtualized conversations export in full.
        let persisted = self
            .store
            .fetch_replica_timeline_all(agent_id)
            .map_err(|e| format!("failed to load replica timeline for `{agent_id}`: {e}"))?;

        let combined = merge_persisted_and_memory_timeline(persisted, mem_events);
        Ok((meta, combined))
    }

    /// Export a complete conversation transcript (Markdown, JSON, or self-contained HTML5)
    /// directly from the local SQLite replica + live in-memory tail so virtualized threads never truncate.
    pub fn export_agent_transcript(
        &self,
        host_id: &str,
        agent_id: &str,
        opts: &crate::export::ExportOptions,
    ) -> Result<crate::export::ExportedTranscript, String> {
        let (meta, events) = self.load_full_agent_export_context(host_id, agent_id)?;
        Ok(crate::export::export_timeline_transcript(
            &meta, &events, opts,
        ))
    }

    /// Extract all fenced code blocks from assistant replies in reverse-chronological order.
    pub fn list_agent_code_blocks(
        &self,
        host_id: &str,
        agent_id: &str,
    ) -> Result<Vec<crate::export::ExtractedCodeBlock>, String> {
        let (_meta, events) = self.load_full_agent_export_context(host_id, agent_id)?;
        Ok(crate::export::extract_code_blocks_from_timeline(&events))
    }

    /// Cross-conversation full-text search across all locally replicated timeline messages,
    /// backed by SQLite FTS5 + BM25 ranking and enriched with host/project/title metadata.
    pub fn search_messages(
        &self,
        query: impl AsRef<str>,
        limit: u32,
    ) -> Vec<ConversationSearchHit> {
        let q = query.as_ref().trim();
        if q.is_empty() {
            return Vec::new();
        }
        let max_hits = if limit == 0 {
            20
        } else {
            (limit as usize).min(100)
        };
        let raw_hits = match self.store.search_replica_messages(q, max_hits) {
            Ok(hits) => hits,
            Err(err) => {
                tracing::warn!(query = %q, error = %err, "FTS5 message search failed");
                return Vec::new();
            }
        };
        if raw_hits.is_empty() {
            return Vec::new();
        }

        let Ok(hosts_guard) = self.hosts.read() else {
            return Vec::new();
        };
        let projects_guard = self.projects.read().ok();
        let mut results = Vec::with_capacity(raw_hits.len());

        for hit in raw_hits {
            let matched_pair = if !hit.host_id.is_empty() {
                hosts_guard
                    .get(&hit.host_id)
                    .map(|mem| (hit.host_id.as_str(), mem))
            } else {
                hosts_guard
                    .iter()
                    .find(|(_, mem)| mem.agents.contains_key(&hit.agent_id))
                    .map(|(hid, mem)| (hid.as_str(), mem))
            };

            let Some((hid, mem)) = matched_pair else {
                continue;
            };
            if hid.is_empty() {
                continue;
            }
            let Some(ag) = mem.agents.get(&hit.agent_id) else {
                continue;
            };

            let resolved_host_id = hid.to_string();
            let mut host_name = mem.profile.effective_display_name();
            if host_name.is_empty() {
                host_name = resolved_host_id.clone();
            }
            let title = if ag.title.trim().is_empty() {
                "Conversation".to_string()
            } else {
                ag.title.clone()
            };
            let mut updated_at_rfc3339 = hit.timestamp_rfc3339.clone().unwrap_or_default();
            if updated_at_rfc3339.is_empty() {
                updated_at_rfc3339 = ag.last_activity_at.unwrap_or(ag.updated_at).to_rfc3339();
            }
            let effective_cwd = mem
                .workspaces
                .iter()
                .find(|w| w.workspace_id == ag.workspace_id)
                .and_then(|w| {
                    w.project_root
                        .as_deref()
                        .or(w.source_cwd.as_deref())
                        .or(Some(w.cwd.as_str()))
                })
                .unwrap_or(ag.cwd.as_str());

            let mut project_name = String::new();
            if let Some(ref projects) = projects_guard
                && let Some(proj) = projects.values().find(|p| {
                    p.host_id == resolved_host_id
                        && p.directories.iter().any(|dir| {
                            cwd_paths_equivalent(effective_cwd, dir)
                                || cwd_paths_equivalent(&ag.cwd, dir)
                        })
                })
            {
                project_name = proj.name.clone();
            }
            if project_name.is_empty() {
                project_name = crate::sidebar::basename_of_project_path(effective_cwd);
            }

            results.push(ConversationSearchHit {
                host_id: resolved_host_id,
                host_name,
                agent_id: hit.agent_id,
                seq: hit.seq,
                role: hit.role,
                title,
                project_name,
                snippet: hit.snippet,
                updated_at_rfc3339,
            });
        }
        results
    }

    pub fn render_diagram(
        &self,
        req: crate::diagram::RenderDiagramRequest,
    ) -> crate::diagram::RenderDiagramView {
        crate::diagram::render_diagram(&req)
    }

    pub fn fleet_sidebar_by_request(
        &self,
        req: &crate::contract::FleetSidebarRequest,
    ) -> crate::sidebar::FleetSidebarView {
        self.fleet_sidebar(
            req.sort,
            req.filter,
            &req.query,
            req.active_host_id.as_deref(),
            req.active_agent_id.as_deref(),
            req.active_cwd.as_deref(),
        )
    }

    pub fn agent_stage_delta_by_request(
        &self,
        req: &crate::contract::AgentStageDeltaRequest,
    ) -> crate::stage::AgentStageDeltaView {
        self.agent_stage_delta_with_aux(
            &req.host_id,
            &req.agent_id,
            req.since_revision,
            req.since_aux_revision,
        )
    }

    pub fn create_session(self: &Arc<Self>, req: crate::contract::CreateSessionRequest) -> String {
        self.create_agent_with_config(CreateAgentRequest {
            host_id: &req.host_id,
            cwd: &req.cwd,
            extra_dirs: req.extra_dirs,
            provider: req.provider,
            acp_agent_id: req.acp_agent_id,
            model: req.model,
            effort: req.effort,
            fast_mode: req.fast_mode,
            mode: req.mode,
            initial_prompt: &req.initial_prompt,
            initial_display_prompt: req.initial_display_prompt,
            initial_badges: req.initial_badges,
            worktree_branch: req.worktree_branch.as_deref(),
            workspace_id: req.workspace_id.as_deref(),
            handle: req.handle,
            auto_cluster: req.auto_cluster,
        })
    }

    pub fn send_prompt_request(self: &Arc<Self>, req: crate::contract::SendPromptRequest) {
        self.send_prompt_with_meta(
            &req.host_id,
            &req.agent_id,
            &req.prompt,
            req.display_prompt,
            req.badges,
        );
    }

    pub fn update_host_config_request(
        self: &Arc<Self>,
        req: crate::contract::UpdateHostConfigRequest,
    ) -> Result<HostCardView, ClientCoreError> {
        self.update_host_config(
            &req.host_id,
            req.custom_name,
            req.route_mode,
            req.ssh_alias,
            req.remote_daemon_port,
            req.custom_direct_endpoint,
            req.relay_url,
            req.reconnect,
        )
    }

    pub fn resolve_model_selection(
        &self,
        req: &crate::contract::ModelSelectionRequest,
    ) -> crate::catalog::ResolvedModelSelectionView {
        let live_catalogs = req
            .host_id
            .as_deref()
            .and_then(|hid| {
                self.hosts
                    .read()
                    .ok()
                    .and_then(|g| g.get(hid).map(|h| h.provider_catalogs.clone()))
            })
            .unwrap_or_default();
        let effective = crate::catalog::effective_provider_catalogs(&live_catalogs);
        crate::catalog::resolve_model_selection(
            &effective,
            req.provider,
            req.desired_model.as_deref(),
            req.desired_effort,
        )
    }

    pub fn slash_commands(
        &self,
        platform: crate::composer::ClientPlatform,
        query: Option<&str>,
    ) -> Vec<crate::composer::SlashCommandView> {
        match query {
            Some(q) if !q.trim().is_empty() => crate::composer::filter_slash_commands(q, platform),
            _ => crate::composer::slash_commands(platform),
        }
    }

    pub fn compose_prompt_for_host(
        &self,
        req: &crate::composer::ComposePromptRequest,
    ) -> crate::composer::ComposedPromptView {
        let fallback_diff = match (req.host_id.as_deref(), req.cwd.as_deref()) {
            (Some(hid), Some(cwd)) if !hid.is_empty() && !cwd.trim().is_empty() => {
                let raw = self.git_review(hid, cwd, None).raw_diff;
                (!raw.trim().is_empty()).then_some(raw)
            }
            _ => None,
        };

        crate::composer::compose_prompt(req, fallback_diff.as_deref(), fallback_diff.as_deref())
    }

    pub fn rank_mention_candidates(
        &self,
        req: &crate::contract::MentionRankRequest,
    ) -> Vec<crate::composer::MentionCandidateView> {
        let files = self.workspace_files(&req.host_id, &req.cwd);
        let (git_files, workspace_peers, path_rules) = self
            .hosts
            .read()
            .ok()
            .and_then(|g| {
                let h = g.get(&req.host_id)?;
                let gfiles = h
                    .git_statuses
                    .get(&req.cwd)
                    .or_else(|| {
                        h.git_statuses
                            .iter()
                            .find(|(k, _)| cwd_paths_equivalent(k, &req.cwd))
                            .map(|(_, v)| v)
                    })
                    .map(|st| {
                        st.files
                            .iter()
                            .map(|f| (f.path.clone(), f.status.clone()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                let target_ws_id = req.workspace_id.as_deref().or_else(|| {
                    req.active_agent_id
                        .as_deref()
                        .and_then(|aid| h.agents.get(aid).map(|a| a.workspace_id.as_str()))
                });
                let peers: Vec<crate::composer::WorkspacePeerMentionInput> = match target_ws_id {
                    Some(ws_id) if !ws_id.is_empty() => h
                        .agents
                        .values()
                        .filter(|a| {
                            a.workspace_id == ws_id
                                && a.archived_at.is_none()
                                && !a.background
                                && req.active_agent_id.as_deref() != Some(a.agent_id.as_str())
                        })
                        .filter_map(|a| {
                            let handle = a.handle.clone()?;
                            let card = agent_to_card_view(a, None);
                            Some(crate::composer::WorkspacePeerMentionInput {
                                handle,
                                title: a.title.clone(),
                                status_label: card.status_label,
                            })
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let rules: Vec<crate::composer::PathRuleMentionInput> = h
                    .path_rules_by_cwd
                    .get(&req.cwd)
                    .or_else(|| {
                        h.path_rules_by_cwd
                            .iter()
                            .find(|(k, _)| cwd_paths_equivalent(k, &req.cwd))
                            .map(|(_, v)| v)
                    })
                    .map(|list| {
                        list.iter()
                            .filter(|r| r.enabled)
                            .map(|r| crate::composer::PathRuleMentionInput {
                                name: r.name.clone(),
                                description: r.description.clone(),
                                relative_path: r.relative_path.clone(),
                                trigger_label: match r.trigger_mode {
                                    RuleTriggerMode::Always => "Always".into(),
                                    RuleTriggerMode::GlobAutoAttach => "Glob".into(),
                                    RuleTriggerMode::AgentDecided => "Agent".into(),
                                    RuleTriggerMode::Manual => "Manual".into(),
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Some((gfiles, peers, rules))
            })
            .unwrap_or_default();

        crate::composer::rank_mention_candidates_with_peers_and_rules(
            &req.query,
            &files,
            &git_files,
            &workspace_peers,
            &path_rules,
            req.limit,
        )
    }

    pub fn build_side_question_context(
        &self,
        host_id: &str,
        agent_id: &str,
        max_chars: Option<usize>,
    ) -> String {
        let mut items: Vec<TimelineItemView> = Vec::new();
        let mut cwd = String::new();
        self.ensure_stage_fresh_and_read(host_id, agent_id, |agent, cached| {
            if let Some(a) = agent {
                cwd = a.cwd.clone();
            }
            if let Some(c) = cached {
                items = c.items.clone();
            }
        });

        let (branch_name, git_status) = if let Ok(guard) = self.hosts.read()
            && let Some(state) = guard.get(host_id)
        {
            let gs = if cwd.is_empty() {
                None
            } else {
                state.git_statuses.get(&cwd).or_else(|| {
                    state
                        .git_statuses
                        .iter()
                        .find(|(k, _)| cwd_paths_equivalent(k, &cwd))
                        .map(|(_, v)| v)
                })
            };
            let branch = if cwd.is_empty() {
                None
            } else {
                crate::sidebar::active_git_branch_from_state(
                    gs,
                    &state.workspaces,
                    &state.discovered_projects,
                    &cwd,
                )
            };
            (branch, gs.cloned())
        } else {
            (None, None)
        };

        let summary = crate::stage::build_side_question_context_summary(
            &cwd,
            branch_name.as_deref(),
            &items,
            git_status.as_ref(),
        );
        match max_chars {
            Some(limit) if summary.len() > limit => truncate_utf8_prefix(&summary, limit),
            _ => summary,
        }
    }

    pub fn mark_agent_seen(&self, host_id: &str, agent_id: &str, seq: u64) {
        if agent_id.is_empty() || agent_id.starts_with("opt_") {
            return;
        }
        let (host_latest, host_read_seq, host_attention, host_is_running) = self
            .hosts
            .read()
            .ok()
            .and_then(|g| {
                g.get(host_id).and_then(|h| {
                    h.agents.get(agent_id).map(|a| {
                        (
                            a.latest_seq,
                            a.read_seq,
                            a.attention,
                            agent_status_is_foreground_running(&a.status),
                        )
                    })
                })
            })
            .unwrap_or((0, 0, AttentionState::None, false));
        let effective_seq = if seq == 0 {
            host_latest
        } else {
            seq.max(host_latest)
        };
        let has_clearable_attention = matches!(
            host_attention,
            AttentionState::Finished | AttentionState::Error | AttentionState::ManualUnread
        );
        if effective_seq == 0 && !has_clearable_attention {
            return;
        }

        let mut pref_changed = false;
        let mut had_unread_badge = has_clearable_attention;
        let now_ms = unix_ms_now() as i64;
        if let Ok(mut prefs) = self.thread_prefs.write() {
            let key = (host_id.to_string(), agent_id.to_string());
            let prev = prefs.get(&key).map(|p| p.seen_seq).unwrap_or(0);
            let prev_effective_seen = host_read_seq.max(prev);
            if !host_is_running && host_latest > 0 && host_latest > prev_effective_seen {
                had_unread_badge = true;
            }
            if effective_seq > prev {
                pref_changed = true;
                let _ = self.store.set_client_thread_seen_seq(
                    host_id,
                    agent_id,
                    effective_seq,
                    false,
                    now_ms,
                );
                let entry = prefs.entry(key).or_insert_with(|| ClientThreadPref {
                    host_id: host_id.to_string(),
                    agent_id: agent_id.to_string(),
                    seen_seq: effective_seq,
                    is_pinned: false,
                    pinned_order: 0,
                    updated_at_ms: now_ms,
                });
                entry.seen_seq = entry.seen_seq.max(effective_seq);
                entry.updated_at_ms = entry.updated_at_ms.max(now_ms);
            }
        }
        let needs_wire = effective_seq > host_read_seq || has_clearable_attention;
        let mut cleared_attention = false;
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            if effective_seq > ag.read_seq {
                ag.read_seq = effective_seq;
            }
            if ag.attention != AttentionState::None && ag.attention != AttentionState::Permission {
                ag.attention = AttentionState::None;
                cleared_attention = true;
            }
        }
        if needs_wire {
            self.dispatch_wire_or_queue(
                host_id,
                WireMessage::MarkAgentRead {
                    request_id: Uuid::new_v4(),
                    agent_id: agent_id.to_string(),
                    seen_seq: (effective_seq > 0).then_some(effective_seq),
                },
            );
        }
        // Only broadcast FleetChanged/HostChanged when a visible unread/attention badge actually cleared.
        // Quiet cursor advances while streaming an already-viewed active session don't need full sidebar/host reloads.
        if cleared_attention || (had_unread_badge && (pref_changed || needs_wire)) {
            self.emit(|revision| CoreNotification::FleetChanged { revision });
            self.emit(|revision| CoreNotification::HostChanged {
                revision,
                host_id: host_id.to_string(),
            });
        }
    }

    pub fn mark_agent_unread(&self, host_id: &str, agent_id: &str) {
        if agent_id.is_empty() || agent_id.starts_with("opt_") {
            return;
        }
        let now_ms = unix_ms_now() as i64;
        let _ = self
            .store
            .set_client_thread_seen_seq(host_id, agent_id, 0, true, now_ms);
        if let Ok(mut prefs) = self.thread_prefs.write() {
            let key = (host_id.to_string(), agent_id.to_string());
            if let Some(entry) = prefs.get_mut(&key) {
                entry.seen_seq = 0;
                entry.updated_at_ms = now_ms;
            }
        }
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
            && ag.attention != AttentionState::Permission
        {
            ag.attention = AttentionState::ManualUnread;
        }
        self.dispatch_wire_or_queue(
            host_id,
            WireMessage::MarkAgentUnread {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
            },
        );
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn set_agent_pinned(&self, host_id: &str, agent_id: &str, pinned: bool) {
        if agent_id.is_empty() {
            return;
        }
        let next_order = if pinned {
            self.pin_order_seq.fetch_add(1, Ordering::SeqCst) as i64
        } else {
            0
        };
        let mut changed = false;
        let now_ms = unix_ms_now() as i64;
        if let Ok(mut prefs) = self.thread_prefs.write() {
            let key = (host_id.to_string(), agent_id.to_string());
            let prev_pinned = prefs.get(&key).is_some_and(|p| p.is_pinned);
            if prev_pinned != pinned {
                changed = true;
                let _ = self
                    .store
                    .set_client_thread_pinned(host_id, agent_id, pinned, next_order, now_ms);
                let entry = prefs.entry(key).or_insert_with(|| ClientThreadPref {
                    host_id: host_id.to_string(),
                    agent_id: agent_id.to_string(),
                    seen_seq: 0,
                    is_pinned: pinned,
                    pinned_order: next_order,
                    updated_at_ms: now_ms,
                });
                entry.is_pinned = pinned;
                entry.pinned_order = if pinned { next_order } else { 0 };
                entry.updated_at_ms = now_ms;
            }
        }
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
            && ag.is_pinned != pinned
        {
            ag.is_pinned = pinned;
            ag.pinned_order = if pinned { next_order } else { 0 };
            changed = true;
        }
        if changed && !agent_id.starts_with("opt_") {
            self.dispatch_wire_or_queue(
                host_id,
                WireMessage::SetAgentPinned {
                    request_id: Uuid::new_v4(),
                    agent_id: agent_id.to_string(),
                    pinned,
                    pinned_order: pinned.then_some(next_order),
                },
            );
        }
        if changed {
            self.emit(|revision| CoreNotification::FleetChanged { revision });
            self.emit(|revision| CoreNotification::HostChanged {
                revision,
                host_id: host_id.to_string(),
            });
        }
    }

    pub fn toggle_pin_agent(&self, host_id: &str, agent_id: &str) -> bool {
        let next_pinned = {
            let prefs = self.thread_prefs.read().ok();
            let was_pinned = prefs
                .as_ref()
                .and_then(|p| p.get(&(host_id.to_string(), agent_id.to_string())))
                .is_some_and(|p| p.is_pinned);
            !was_pinned
        };
        self.set_agent_pinned(host_id, agent_id, next_pinned);
        next_pinned
    }

    pub fn seed_thread_prefs(&self, seeds: Vec<ClientThreadPref>) {
        if seeds.is_empty() {
            return;
        }
        let mut changed = false;
        if let Ok(mut prefs) = self.thread_prefs.write() {
            for seed in seeds {
                if seed.host_id.is_empty() || seed.agent_id.is_empty() {
                    continue;
                }
                let key = (seed.host_id.clone(), seed.agent_id.clone());
                if let Some(existing) = prefs.get_mut(&key) {
                    let mut dirty = false;
                    if seed.seen_seq > existing.seen_seq {
                        existing.seen_seq = seed.seen_seq;
                        dirty = true;
                    }
                    if seed.is_pinned && !existing.is_pinned {
                        existing.is_pinned = true;
                        existing.pinned_order = if seed.pinned_order > 0 {
                            seed.pinned_order
                        } else {
                            self.pin_order_seq.fetch_add(1, Ordering::SeqCst) as i64
                        };
                        dirty = true;
                    }
                    if dirty {
                        let _ = self.store.upsert_client_thread_pref(existing);
                        changed = true;
                    }
                } else {
                    let mut normalized = seed;
                    if normalized.is_pinned && normalized.pinned_order <= 0 {
                        normalized.pinned_order =
                            self.pin_order_seq.fetch_add(1, Ordering::SeqCst) as i64;
                    }
                    let _ = self.store.upsert_client_thread_pref(&normalized);
                    prefs.insert(key, normalized);
                    changed = true;
                }
            }
        }
        if changed {
            self.emit(|revision| CoreNotification::FleetChanged { revision });
        }
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
        self.dispatch_wire_or_queue(
            host_id,
            WireMessage::UpsertHostProject {
                request_id: Uuid::new_v4(),
                project: HostProjectEntry {
                    project_id: view.project_id.clone(),
                    name: view.name.clone(),
                    directories: view.directories.clone(),
                    sort_order: 0,
                    hidden: false,
                    updated_at_ms: view.updated_at_ms,
                },
            },
        );
        self.record_cloud_meta_mutation(|meta| meta.record_project_mutated(&view.project_id));
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(view)
    }

    pub fn remove_project(&self, project_id: &str) -> Result<(), ClientCoreError> {
        let removed_host_id = self
            .projects
            .read()
            .ok()
            .and_then(|g| g.get(project_id).map(|p| p.host_id.clone()));
        self.store.delete_client_project(project_id)?;
        let now_ms = unix_ms_now() as i64;
        self.record_cloud_meta_mutation(|meta| meta.record_project_deleted(project_id, now_ms));
        if let Ok(mut guard) = self.projects.write() {
            guard.remove(project_id);
        }
        if let Ok(mut order) = self.project_order.write() {
            let before = order.len();
            order.retain(|k| k != project_id);
            if order.len() != before
                && let Ok(json) = serde_json::to_string(&*order)
            {
                let _ = self
                    .store
                    .set_client_kv("project_order", &json, unix_ms_now() as i64);
                self.record_cloud_meta_mutation(|meta| meta.record_project_order_mutated(now_ms));
            }
        }
        if let Some(hid) = removed_host_id {
            self.dispatch_wire_or_queue(
                &hid,
                WireMessage::DeleteHostProject {
                    request_id: Uuid::new_v4(),
                    project_id: project_id.to_string(),
                },
            );
        }
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(())
    }

    pub fn project_order(&self) -> Vec<String> {
        self.project_order
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Persists manual sidebar project order locally and synchronizes each host's slice
    /// to its Orbit Daemon (`SetHostProjectOrder`) so every connected device converges.
    pub fn reorder_projects(
        &self,
        ordered_keys: Vec<String>,
    ) -> Result<Vec<String>, ClientCoreError> {
        let mut deduped: Vec<String> = Vec::with_capacity(ordered_keys.len());
        let mut seen: HashSet<String> = HashSet::new();
        for raw in ordered_keys {
            let clean = raw.trim().to_string();
            if !clean.is_empty() && seen.insert(clean.clone()) {
                deduped.push(clean);
            }
        }
        let now_ms = unix_ms_now() as i64;
        if let Ok(json) = serde_json::to_string(&deduped) {
            self.store.set_client_kv("project_order", &json, now_ms)?;
        }
        self.record_cloud_meta_mutation(|meta| meta.record_project_order_mutated(now_ms));
        if let Ok(mut guard) = self.project_order.write() {
            *guard = deduped.clone();
        }

        // Partition keys by host and push to each host daemon.
        let project_hosts: HashMap<String, String> = self
            .projects
            .read()
            .ok()
            .map(|g| {
                g.iter()
                    .map(|(pid, p)| (pid.clone(), p.host_id.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let known_hosts: Vec<String> = self
            .hosts
            .read()
            .ok()
            .map(|g| g.keys().cloned().collect())
            .unwrap_or_default();

        let mut per_host: HashMap<String, Vec<String>> = HashMap::new();
        for key in &deduped {
            let owner = project_hosts.get(key).cloned().or_else(|| {
                key.split_once("::").and_then(|(prefix, _)| {
                    known_hosts
                        .iter()
                        .find(|hid| hid.as_str() == prefix)
                        .cloned()
                })
            });
            if let Some(hid) = owner {
                per_host.entry(hid).or_default().push(key.clone());
            }
        }
        for (hid, host_keys) in per_host {
            self.dispatch_wire_or_queue(
                &hid,
                WireMessage::SetHostProjectOrder {
                    request_id: Uuid::new_v4(),
                    project_order: host_keys,
                    project_sort_mode: Some("manual".to_string()),
                },
            );
        }

        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(deduped)
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

            let pairing_secret_updated_at_ms = match prev {
                Some(h)
                    if h.profile.pairing_secret == offer.pairing_secret
                        && h.profile.pairing_secret_updated_at_ms > 0 =>
                {
                    h.profile.pairing_secret_updated_at_ms
                }
                _ => now_unix_ms(),
            };

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
                pairing_secret: offer.pairing_secret,
                pairing_secret_updated_at_ms,
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
        let now_ms = now_unix_ms();
        self.record_cloud_meta_mutation(|meta| meta.record_host_mutated(&offer.host_id, now_ms));
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.connect_host_async(offer.host_id);
        self.schedule_cloud_auto_sync(false);
        Ok(card)
    }

    /// Export a valid `orbit://pair/...` URI for a paired host using its real persisted `pairing_secret`.
    pub fn export_host_pairing_uri(&self, host_id: &str) -> Result<String, ClientCoreError> {
        let guard = self
            .hosts
            .read()
            .map_err(|_| ClientCoreError::LockPoisoned)?;
        let h = guard.get(host_id).ok_or(ClientCoreError::Unreachable)?;
        let pairing_secret = if h.is_local {
            crate::desktop_ops::read_local_daemon_pairing_secret()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| h.profile.pairing_secret.clone())
        } else {
            h.profile.pairing_secret.clone()
        };
        if pairing_secret.trim().is_empty() {
            return Err(ClientCoreError::Handshake(
                "host has no persisted pairing secret — run `orbit pair` on the host and re-pair"
                    .into(),
            ));
        }
        let offer = PairingOffer {
            host_id: h.profile.host_id.clone(),
            hostname: h.profile.effective_display_name(),
            daemon_pubkey_b64: h.profile.daemon_pubkey_b64.clone(),
            direct_endpoints: h.profile.direct_endpoints.clone(),
            relay_url: h.profile.relay_url.clone(),
            pairing_secret,
        };
        Ok(offer.to_qr_uri()?)
    }

    pub fn remove_host(&self, host_id: &str) -> Result<(), ClientCoreError> {
        let _ = self.store.delete_client_host(host_id);
        let now_ms = now_unix_ms();
        self.record_cloud_meta_mutation(|meta| meta.record_host_deleted(host_id, now_ms));
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
        if let Ok(mut prefs) = self.thread_prefs.write() {
            prefs.retain(|(h, _), _| h != host_id);
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
        let now_ms = now_unix_ms();
        self.record_cloud_meta_mutation(|meta| meta.record_host_mutated(host_id, now_ms));
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
        if reconnect {
            self.connect_host_async(host_id.to_string());
        }
        self.schedule_cloud_auto_sync(false);
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
        self.schedule_cloud_auto_sync(true);
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
            h.caught_up_agents.clear();
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
                let (queued_outbox, resub_timelines, resub_provider_subagents, conn_gen) = {
                    let Ok(mut guard) = self.hosts.write() else {
                        return;
                    };
                    let Some(h) = guard.get_mut(host_id) else {
                        return;
                    };
                    h.connecting_in_flight = false;
                    h.reconnect_scheduled = false;
                    h.reconnect_attempt = 0;
                    h.ssh_heal_attempted = false;
                    h.profile.state = handle.mode;
                    h.via_ssh_tunnel = handle.via_ssh_tunnel;
                    h.last_error = None;
                    h.session_gen = h.session_gen.wrapping_add(1);
                    let conn_gen = h.session_gen;
                    h.session = Some(handle.clone());
                    h.caught_up_agents.clear();

                    let outbox = std::mem::take(&mut h.pending_outbox);
                    let resubs: Vec<(String, u64)> = h
                        .timelines
                        .iter()
                        .filter(|(aid, _)| !aid.starts_with("opt_"))
                        .map(|(aid, evs)| {
                            let max_seq = evs.iter().map(|e| e.seq).max().unwrap_or(0);
                            (aid.clone(), self.safe_catchup_since_seq(aid, max_seq))
                        })
                        .collect();
                    let resub_subagents = collect_provider_subagent_resubscriptions(h);
                    (outbox, resubs, resub_subagents, conn_gen)
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
                // followed by orchestration state, filesystem project discovery & background quota probes!
                let _ = handle
                    .send_wire(WireMessage::GetHostSyncState {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListWorkspaces {
                        request_id: Uuid::new_v4(),
                        include_archived: true,
                        include_background: false,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListAgents {
                        request_id: Uuid::new_v4(),
                        workspace_id: None,
                        include_archived: true,
                        include_background: false,
                        parent_agent_id: None,
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
                    .send_wire(WireMessage::ListProfiles {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListAcpAgents {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let list_artifacts_req_id = Uuid::new_v4();
                if let Ok(mut hosts) = self.hosts.write()
                    && let Some(h) = hosts.get_mut(host_id)
                {
                    if h.unsolicited_unfiltered_artifact_reqs.len() >= 64 {
                        h.unsolicited_unfiltered_artifact_reqs.clear();
                    }
                    h.unsolicited_unfiltered_artifact_reqs
                        .insert(list_artifacts_req_id);
                }
                let _ = handle
                    .send_wire(WireMessage::ListArtifacts {
                        request_id: list_artifacts_req_id,
                        workspace_id: None,
                        agent_id: None,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListSchedules {
                        request_id: Uuid::new_v4(),
                        include_completed: true,
                    })
                    .await;
                let list_timers_req_id = Uuid::new_v4();
                if let Ok(mut hosts) = self.hosts.write()
                    && let Some(h) = hosts.get_mut(host_id)
                {
                    if h.background_timer_list_reqs.len() >= 64 {
                        h.background_timer_list_reqs.clear();
                    }
                    h.background_timer_list_reqs.insert(list_timers_req_id);
                }
                let _ = handle
                    .send_wire(WireMessage::ListConditionalTimers {
                        request_id: list_timers_req_id,
                        agent_id: None,
                        include_resolved: true,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListGoalRuns {
                        request_id: Uuid::new_v4(),
                        agent_id: None,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListMonitorWatches {
                        request_id: Uuid::new_v4(),
                        agent_id: None,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListWorkflowRuns {
                        request_id: Uuid::new_v4(),
                        include_completed: true,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListConnectedHosts {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::GetOrchestrationConfig {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::GetKeepAwakeStatus {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListOrchestrationSkills {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let bg_skills_req_id = Uuid::new_v4();
                if let Ok(mut hosts) = self.hosts.write()
                    && let Some(h) = hosts.get_mut(host_id)
                {
                    if h.background_managed_skills_reqs.len() >= 64 {
                        h.background_managed_skills_reqs.clear();
                    }
                    h.background_managed_skills_reqs.insert(bg_skills_req_id);
                }
                let _ = handle
                    .send_wire(WireMessage::ListManagedSkills {
                        request_id: bg_skills_req_id,
                        cwd: None,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListPendingPermissions {
                        request_id: Uuid::new_v4(),
                        agent_id: None,
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
                for (parent_agent_id, subagent_id, since_seq) in resub_provider_subagents {
                    let _ = handle
                        .send_wire(WireMessage::SubscribeProviderSubagent {
                            request_id: Uuid::new_v4(),
                            parent_agent_id,
                            subagent_id,
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
                                loop_core
                                    .handle_unexpected_disconnect(&owned_host_id, Some(conn_gen));
                                break;
                            }
                        }
                    }
                });
            }
            Err(err) => {
                tracing::warn!(host_id = %host_id, error = %err, "Host connection attempt failed");
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
                        h.stage_cache.clear();
                        h.stage_dirty.clear();
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
                                if !offer.pairing_secret.trim().is_empty()
                                    && h.profile.pairing_secret != offer.pairing_secret
                                {
                                    h.profile.pairing_secret = offer.pairing_secret;
                                    h.profile.pairing_secret_updated_at_ms = now_unix_ms();
                                }
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

    fn handle_unexpected_disconnect(self: &Arc<Self>, host_id: &str, expected_gen: Option<u64>) {
        let mut should_reconnect = false;
        let mut did_disconnect = false;
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if h.session.is_none() {
                return;
            }
            if let Some(gen_id) = expected_gen
                && h.session_gen != gen_id
            {
                return;
            }
            did_disconnect = true;
            // Non-destructive disconnect: keep ALL workspaces, agents & timelines intact!
            h.profile.state = HostConnectionState::Disconnected;
            h.via_ssh_tunnel = false;
            h.session = None;
            h.caught_up_agents.clear();
            if h.pending_outbox.is_empty() {
                h.agent_req_by_req_id.clear();
                h.pending_queue_adds.clear();
                h.pending_queue_removals.clear();
                h.pending_queue_reorders.clear();
            }
            fail_pending_host_requests(h, "Host disconnected before request completed");
            should_reconnect = true;
        }
        if !did_disconnect {
            return;
        }
        tracing::warn!(host_id = %host_id, "Host session disconnected unexpectedly; scheduling reconnect");
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
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(&host_id)
        {
            if h.reconnect_scheduled || h.connecting_in_flight {
                return;
            }
            h.reconnect_scheduled = true;
        }
        let core = Arc::clone(self);
        self.runtime.spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            let still_disconnected = core
                .hosts
                .write()
                .ok()
                .and_then(|mut g| {
                    let h = g.get_mut(&host_id)?;
                    h.reconnect_scheduled = false;
                    Some(
                        h.profile.state == HostConnectionState::Disconnected
                            && !h.connecting_in_flight,
                    )
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
        let mut notify_timeline_agents: Vec<String> = Vec::new();
        let mut push_notify_timeline = |ag_id: String| {
            if !notify_timeline_agents
                .iter()
                .any(|existing| existing == &ag_id)
            {
                notify_timeline_agents.push(ag_id);
            }
        };
        let mut notify_git_cwd: Option<String> = None;
        let mut auto_subscribe_agent: Option<String> = None;
        let mut continue_catchup: Option<(String, u64)> = None;
        let mut continue_subagent_catchup: Option<(String, String, u64)> = None;
        let mut catchup_completed_cursor: Option<(String, u64)> = None;
        let mut live_event_cursor: Option<(String, u64)> = None;
        let mut host_to_persist: Option<StoredHostProfile> = None;
        let mut rewound_to_persist: Option<(AgentSummary, Vec<TimelineEvent>, bool, bool)> = None;
        let mut forked_to_persist: Option<(AgentSummary, Option<WorkspaceSummary>)> = None;
        let mut stale_replica_agents: Vec<(String, bool)> = Vec::new();
        let mut initial_seen_seeds: Vec<(String, u64)> = Vec::new();
        type ServerPrefSync = (String, u64, Option<(bool, i64)>);
        let mut server_pref_syncs: Vec<ServerPrefSync> = Vec::new();
        let mut host_sync_to_apply: Option<(HostSyncState, bool)> = None;
        let mut migrated_opt_pref: Option<(String, String)> = None;
        let mut rewound_seen_reset: Option<(String, u64)> = None;

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
                WireMessage::WorkspaceCreated { workspace, .. }
                | WireMessage::WorkspaceUpdated { workspace } => {
                    state
                        .workspaces
                        .retain(|w| w.workspace_id != workspace.workspace_id);
                    state.workspaces.insert(0, workspace);
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::WorkspaceSetupProgress { .. } => {
                    notify_host = true;
                }
                WireMessage::WorkspaceScriptsSnapshot {
                    workspace_id,
                    scripts,
                    ..
                } => {
                    state.workspace_scripts.insert(workspace_id, scripts);
                    notify_host = true;
                }
                WireMessage::WorkspaceScriptUpdated { script } => {
                    let list = state
                        .workspace_scripts
                        .entry(script.workspace_id.clone())
                        .or_default();
                    if let Some(slot) = list.iter_mut().find(|s| s.name == script.name) {
                        *slot = script;
                    } else {
                        list.push(script);
                    }
                    notify_host = true;
                }
                WireMessage::AgentsSnapshot { agents, .. } => {
                    // Keep any 0ms optimistic `opt_*` sessions while refreshing server-confirmed ones
                    let prev_agents: HashMap<String, (u64, u64)> = state
                        .agents
                        .iter()
                        .map(|(k, v)| (k.clone(), (v.timeline_epoch, v.latest_seq)))
                        .collect();
                    let is_initial_hydration = !state.initial_agents_hydrated;
                    let sync_pin_authoritative = state.host_sync_supported;
                    state.initial_agents_hydrated = true;
                    state.agents.retain(|k, _| k.starts_with("opt_"));
                    for mut a in agents {
                        reconcile_pending_queue_ops(state, &mut a);
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
                            state.caught_up_agents.remove(&ag_id);
                            state.stage_cache.remove(&ag_id);
                            state.mark_stage_dirty(&ag_id, 0, true);
                            state.rewound_agents.insert(ag_id.clone());
                            stale_replica_agents.push((ag_id.clone(), had_mem));
                            push_notify_timeline(ag_id.clone());
                        } else if state.stage_cache.contains_key(&ag_id) {
                            state.mark_stage_meta_dirty(&ag_id);
                            push_notify_timeline(ag_id.clone());
                        }
                        if is_initial_hydration && a.latest_seq > 0 {
                            let seed_seq = if a.read_seq == 0 && a.attention == AttentionState::None
                            {
                                a.latest_seq
                            } else {
                                a.read_seq
                            };
                            initial_seen_seeds.push((ag_id.clone(), seed_seq));
                        }
                        let pin_sync = if sync_pin_authoritative || a.is_pinned {
                            Some((a.is_pinned, a.pinned_order))
                        } else {
                            None
                        };
                        if a.read_seq > 0 || pin_sync.is_some() {
                            server_pref_syncs.push((ag_id.clone(), a.read_seq, pin_sync));
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
                        if let Some(opt_cache) = state.stage_cache.remove(&opt_id)
                            && !state.stage_cache.contains_key(&ag_id)
                        {
                            state.stage_cache.insert(ag_id.clone(), opt_cache);
                        }
                        if let Some(opt_dirty) = state.stage_dirty.remove(&opt_id) {
                            state.stage_dirty.entry(ag_id.clone()).or_insert(opt_dirty);
                        }
                        state.mark_stage_meta_dirty(&ag_id);
                        state
                            .replaced_optimistic_agents
                            .insert(opt_id.clone(), ag_id.clone());
                        migrated_opt_pref = Some((opt_id, ag_id.clone()));
                    } else {
                        state.mark_stage_dirty(&ag_id, 0, true);
                    }
                    if let Some(ref parent_id) = agent.parent_agent_id {
                        state.mark_stage_meta_dirty(parent_id);
                        push_notify_timeline(parent_id.clone());
                    }
                    if agent.read_seq > 0 || agent.is_pinned {
                        server_pref_syncs.push((
                            ag_id.clone(),
                            agent.read_seq,
                            agent.is_pinned.then_some((true, agent.pinned_order)),
                        ));
                    }
                    state.last_created_agent_id = Some(ag_id.clone());
                    state.agents.insert(ag_id.clone(), agent);
                    auto_subscribe_agent = Some(ag_id.clone());
                    notify_fleet = true;
                    notify_host = true;
                    push_notify_timeline(ag_id);
                }
                WireMessage::AgentForked {
                    request_id,
                    agent,
                    workspace,
                } => {
                    let ag_id = agent.agent_id.clone();
                    if let Some(ref ws) = workspace {
                        state
                            .workspaces
                            .retain(|w| w.workspace_id != ws.workspace_id);
                        state.workspaces.insert(0, ws.clone());
                    }
                    let agent_card = {
                        let prefs_guard = self.thread_prefs.read().ok();
                        let existing_pref = prefs_guard
                            .as_ref()
                            .and_then(|p| p.get(&(host_id.to_string(), ag_id.clone())));
                        agent_to_card_view(&agent, existing_pref)
                    };
                    if agent.read_seq > 0 || agent.is_pinned {
                        server_pref_syncs.push((
                            ag_id.clone(),
                            agent.read_seq,
                            agent.is_pinned.then_some((true, agent.pinned_order)),
                        ));
                    }
                    state.mark_stage_dirty(&ag_id, 0, true);
                    state.last_created_agent_id = Some(ag_id.clone());
                    state.agents.insert(ag_id.clone(), agent.clone());
                    forked_to_persist = Some((agent, workspace.clone()));
                    auto_subscribe_agent = Some(ag_id.clone());
                    if let Some(tx) = state.pending_forks.remove(&request_id) {
                        let _ = tx.send(Ok(ForkAgentResultView {
                            agent: agent_card,
                            workspace,
                        }));
                    }
                    notify_fleet = true;
                    notify_host = true;
                    push_notify_timeline(ag_id);
                }
                WireMessage::WorktreeHandoffPreviewResult {
                    request_id,
                    preview,
                } => {
                    if let Some(tx) = state.pending_handoff_previews.remove(&request_id) {
                        let _ = tx.send(Ok(preview));
                    }
                }
                WireMessage::WorktreeHandoffExecuted {
                    request_id,
                    outcome,
                } => {
                    if let Some(ref ws) = outcome.target_workspace {
                        if let Some(slot) = state
                            .workspaces
                            .iter_mut()
                            .find(|w| w.workspace_id == ws.workspace_id)
                        {
                            *slot = ws.clone();
                        } else {
                            state.workspaces.push(ws.clone());
                        }
                        notify_host = true;
                        notify_fleet = true;
                    }
                    if let Some(tx) = state.pending_handoff_executes.remove(&request_id) {
                        let _ = tx.send(Ok(outcome));
                    }
                }
                WireMessage::AgentUpdated { mut agent } => {
                    reconcile_pending_queue_ops(state, &mut agent);
                    let ag_id = agent.agent_id.clone();
                    let prev_summary = state.agents.get(&ag_id);
                    let prev_epoch = prev_summary.map(|a| a.timeline_epoch);
                    let prev_pinned = prev_summary.map(|a| a.is_pinned);
                    let prev_parent_id = prev_summary.and_then(|a| a.parent_agent_id.clone());
                    let stage_fields_unchanged =
                        prev_summary.is_some_and(|prev| agent_stage_meta_unchanged(prev, &agent));
                    let unread_badge_changed = prev_summary.is_none_or(|prev| {
                        let prev_running = agent_status_is_foreground_running(&prev.status);
                        let next_running = agent_status_is_foreground_running(&agent.status);
                        let prev_unread = !prev_running
                            && (prev.attention != AttentionState::None
                                || (prev.latest_seq > 0 && prev.latest_seq > prev.read_seq));
                        let next_unread = !next_running
                            && (agent.attention != AttentionState::None
                                || (agent.latest_seq > 0 && agent.latest_seq > agent.read_seq));
                        prev_unread != next_unread || prev.attention != agent.attention
                    });
                    let local_max_seq = state
                        .timelines
                        .get(&ag_id)
                        .and_then(|evs| evs.iter().map(|e| e.seq).max())
                        .unwrap_or(0);
                    let epoch_advanced = prev_epoch.is_some_and(|ep| agent.timeline_epoch > ep);
                    let seq_regressed = local_max_seq > 0 && agent.latest_seq < local_max_seq;
                    if epoch_advanced || seq_regressed {
                        let had_mem = state.timelines.remove(&ag_id).is_some();
                        state.caught_up_agents.remove(&ag_id);
                        state.stage_cache.remove(&ag_id);
                        state.mark_stage_dirty(&ag_id, 0, true);
                        state.rewound_agents.insert(ag_id.clone());
                        stale_replica_agents.push((ag_id.clone(), had_mem));
                    } else if !stage_fields_unchanged {
                        state.mark_stage_meta_dirty(&ag_id);
                    }
                    if !stage_fields_unchanged {
                        if let Some(ref parent_id) = agent.parent_agent_id {
                            state.mark_stage_meta_dirty(parent_id);
                            push_notify_timeline(parent_id.clone());
                        }
                        if let Some(prev_pid) = prev_parent_id
                            && agent.parent_agent_id.as_deref() != Some(prev_pid.as_str())
                        {
                            state.mark_stage_meta_dirty(&prev_pid);
                            push_notify_timeline(prev_pid);
                        }
                    }
                    let pin_sync = if state.host_sync_supported
                        || agent.is_pinned
                        || prev_pinned.is_some_and(|p| p != agent.is_pinned)
                    {
                        Some((agent.is_pinned, agent.pinned_order))
                    } else {
                        None
                    };
                    if agent.read_seq > 0 || pin_sync.is_some() {
                        server_pref_syncs.push((ag_id.clone(), agent.read_seq, pin_sync));
                    }
                    state.agents.insert(ag_id.clone(), agent);
                    if !stage_fields_unchanged || unread_badge_changed {
                        notify_fleet = true;
                        notify_host = true;
                    }
                    if epoch_advanced || seq_regressed || !stage_fields_unchanged {
                        push_notify_timeline(ag_id);
                    }
                }
                WireMessage::AgentDeleted { agent_id } => {
                    let parent_id = state
                        .agents
                        .get(&agent_id)
                        .and_then(|a| a.parent_agent_id.clone());
                    state.agents.remove(&agent_id);
                    state.timelines.remove(&agent_id);
                    state.conditional_timers.retain(|t| t.agent_id != agent_id);
                    state.provider_subagents.remove(&agent_id);
                    state
                        .provider_subagent_timelines
                        .retain(|(pid, _), _| pid != &agent_id);
                    state
                        .provider_subagent_access_order
                        .retain(|(pid, _)| pid != &agent_id);
                    if state
                        .active_provider_subagent
                        .as_ref()
                        .is_some_and(|(pid, _)| pid == &agent_id)
                    {
                        state.active_provider_subagent = None;
                    }
                    state
                        .subscribed_provider_subagents
                        .retain(|(pid, _)| pid != &agent_id);
                    state.stage_cache.remove(&agent_id);
                    state.stage_access_order.retain(|id| id != &agent_id);
                    state.stage_dirty.remove(&agent_id);
                    state.caught_up_agents.remove(&agent_id);
                    state.rewound_agents.remove(&agent_id);
                    if let Some(pid) = parent_id {
                        state.mark_stage_meta_dirty(&pid);
                        push_notify_timeline(pid);
                    }
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::PendingPermissionsSnapshot { permissions, .. } => {
                    state.pending_permissions = permissions;
                    notify_host = true;
                }
                WireMessage::ProfilesSnapshot { profiles, .. } => {
                    state.profiles = profiles;
                    notify_host = true;
                }
                WireMessage::ProfileUpserted { profile, .. } => {
                    if let Some(slot) = state
                        .profiles
                        .iter_mut()
                        .find(|p| p.profile_id == profile.profile_id)
                    {
                        *slot = profile;
                    } else {
                        state.profiles.push(profile);
                    }
                    notify_host = true;
                }
                WireMessage::AcpAgentsSnapshot {
                    request_id,
                    agents,
                    warning,
                } => {
                    let has_installed = agents.iter().any(|a| a.installed && a.spec.enabled);
                    if has_installed && !state.available_providers.contains(&ProviderKind::Acp) {
                        state.available_providers.push(ProviderKind::Acp);
                        notify_fleet = true;
                    } else if !has_installed
                        && state.available_providers.contains(&ProviderKind::Acp)
                    {
                        state
                            .available_providers
                            .retain(|p| *p != ProviderKind::Acp);
                        notify_fleet = true;
                    }
                    state.acp_agents = agents.clone();
                    if let Some(tx) = state.pending_acp_snapshots.remove(&request_id) {
                        let _ = tx.send(Ok(crate::contract::AcpRegistryRefreshView {
                            agents,
                            warning,
                        }));
                    }
                    notify_host = true;
                }
                WireMessage::AcpAgentProbeResult { request_id, result } => {
                    if let Some(tx) = state.pending_acp_probes.remove(&request_id) {
                        let _ = tx.send(Ok(result));
                    }
                }
                WireMessage::ExternalMcpServersSnapshot {
                    request_id,
                    servers,
                } => {
                    if let Some(tx) = state.pending_mcp_snapshots.remove(&request_id) {
                        let _ = tx.send(Ok(servers));
                    }
                }
                WireMessage::ExternalMcpProbeResponse {
                    request_id,
                    server_id: _,
                    result,
                } => {
                    if let Some(tx) = state.pending_mcp_probes.remove(&request_id) {
                        let _ = tx.send(Ok(result));
                    }
                }
                WireMessage::ArtifactsSnapshot {
                    request_id,
                    artifacts,
                } => {
                    let pending = state.pending_artifact_lists.remove(&request_id);
                    let was_unsolicited_unfiltered = state
                        .unsolicited_unfiltered_artifact_reqs
                        .remove(&request_id);
                    let is_unfiltered = pending.as_ref().is_some_and(|(unfiltered, _)| *unfiltered)
                        || was_unsolicited_unfiltered;
                    if is_unfiltered {
                        state.artifacts = artifacts.clone();
                    } else {
                        for item in &artifacts {
                            upsert_cached_artifact(&mut state.artifacts, item.clone());
                        }
                    }
                    state.artifacts.sort_by(|a, b| {
                        b.updated_at
                            .cmp(&a.updated_at)
                            .then_with(|| a.artifact_id.cmp(&b.artifact_id))
                    });
                    if let Some((_, tx)) = pending {
                        let _ = tx.send(Ok(artifacts));
                    }
                    notify_host = true;
                }
                WireMessage::ArtifactContent {
                    request_id,
                    artifact,
                } => {
                    let latest_rev = artifact
                        .revisions
                        .first()
                        .map(|r| r.version)
                        .unwrap_or(artifact.summary.version);
                    let cached_ver = state
                        .artifacts
                        .iter()
                        .find(|a| a.artifact_id == artifact.summary.artifact_id)
                        .map(|a| a.version)
                        .unwrap_or(0);
                    if artifact.summary.version >= latest_rev
                        && artifact.summary.version >= cached_ver
                    {
                        upsert_cached_artifact(&mut state.artifacts, artifact.summary.clone());
                    }
                    if let Some(tx) = state.pending_artifact_reads.remove(&request_id) {
                        let _ = tx.send(Ok(artifact));
                    }
                    notify_host = true;
                }
                WireMessage::ArtifactUpdated {
                    request_id,
                    artifact,
                } => {
                    upsert_cached_artifact(&mut state.artifacts, artifact.clone());
                    if let Some(rid) = request_id
                        && let Some(tx) = state.pending_artifact_upserts.remove(&rid)
                    {
                        let _ = tx.send(Ok(artifact));
                    }
                    notify_host = true;
                }
                WireMessage::ArtifactDeleted {
                    request_id,
                    artifact_id,
                } => {
                    state.artifacts.retain(|a| a.artifact_id != artifact_id);
                    if let Some(rid) = request_id
                        && let Some(tx) = state.pending_artifact_deletes.remove(&rid)
                    {
                        let _ = tx.send(Ok(artifact_id));
                    }
                    notify_host = true;
                }
                WireMessage::SchedulesSnapshot { schedules, .. } => {
                    state.schedules = schedules;
                    notify_host = true;
                }
                WireMessage::ScheduleCreated { schedule, .. }
                | WireMessage::ScheduleUpdated { schedule } => {
                    if let Some(slot) = state
                        .schedules
                        .iter_mut()
                        .find(|s| s.schedule_id == schedule.schedule_id)
                    {
                        *slot = schedule;
                    } else {
                        state.schedules.insert(0, schedule);
                    }
                    notify_host = true;
                }
                WireMessage::ScheduleDeleted { schedule_id } => {
                    state.schedules.retain(|s| s.schedule_id != schedule_id);
                    state.schedule_runs.remove(&schedule_id);
                    notify_host = true;
                }
                WireMessage::ScheduleRunsSnapshot {
                    schedule_id, runs, ..
                } => {
                    state.schedule_runs.insert(schedule_id, runs);
                    notify_host = true;
                }
                WireMessage::ConnectedHostsSnapshot { hosts, .. } => {
                    state.connected_hosts = hosts;
                    notify_host = true;
                }
                WireMessage::ConnectedHostAdded { host, .. }
                | WireMessage::ConnectedHostUpdated { host } => {
                    if let Some(slot) = state
                        .connected_hosts
                        .iter_mut()
                        .find(|h| h.registration_id == host.registration_id)
                    {
                        *slot = host;
                    } else {
                        state.connected_hosts.push(host);
                    }
                    notify_host = true;
                }
                WireMessage::ConnectedHostRemoved { registration_id } => {
                    state
                        .connected_hosts
                        .retain(|h| h.registration_id != registration_id);
                    notify_host = true;
                }
                WireMessage::OrchestrationConfigSnapshot { config, .. } => {
                    state.orchestration_config = config;
                    notify_host = true;
                }
                WireMessage::KeepAwakeStatusSnapshot { status, .. }
                | WireMessage::KeepAwakeStatusUpdated { status } => {
                    state.orchestration_config.keep_awake_while_running = status.enabled;
                    state.keep_awake_status = Some(status);
                    notify_host = true;
                }
                WireMessage::OrchestrationSkillsSnapshot { skills, .. } => {
                    state.orchestration_skills = skills;
                    notify_host = true;
                }
                WireMessage::ManagedSkillsSnapshot { request_id, skills } => {
                    if let Some((req_cwd, tx)) =
                        state.pending_managed_skills_lists.remove(&request_id)
                    {
                        if req_cwd == state.active_managed_skills_cwd {
                            state.managed_skills = skills.clone();
                            notify_host = true;
                        }
                        let _ = tx.send(Ok(skills));
                    } else if state.background_managed_skills_reqs.remove(&request_id) {
                        // Never let an unsolicited `cwd: None` boot/refresh snapshot clobber an active project-scoped skills list.
                        if state.active_managed_skills_cwd.is_none() {
                            state.managed_skills = skills;
                            notify_host = true;
                        }
                    } else if state.active_managed_skills_cwd.is_none() {
                        state.managed_skills = skills;
                        notify_host = true;
                    }
                }
                WireMessage::ManagedSkillContentResponse {
                    request_id,
                    skill_id,
                    content,
                    etag,
                } => {
                    if let Some(tx) = state.pending_managed_skill_reads.remove(&request_id) {
                        let _ = tx.send(Ok(ManagedSkillContentView {
                            skill_id,
                            content,
                            etag,
                        }));
                    }
                }
                WireMessage::ProviderSubagentsSnapshot {
                    parent_agent_id,
                    subagents,
                    ..
                } => {
                    state.mark_stage_meta_dirty(&parent_agent_id);
                    push_notify_timeline(parent_agent_id.clone());
                    state.provider_subagents.insert(parent_agent_id, subagents);
                    state.enforce_provider_subagent_timeline_bounds(None);
                    notify_host = true;
                }
                WireMessage::ProviderSubagentUpdated { subagent } => {
                    let parent_id = subagent.parent_agent_id.clone();
                    let finished =
                        subagent.status != orbit_protocol::ProviderSubagentStatus::Running;
                    let list = state
                        .provider_subagents
                        .entry(parent_id.clone())
                        .or_default();
                    if let Some(slot) = list
                        .iter_mut()
                        .find(|s| s.subagent_id == subagent.subagent_id)
                    {
                        *slot = subagent;
                    } else {
                        list.push(subagent);
                    }
                    if finished {
                        state.enforce_provider_subagent_timeline_bounds(None);
                    }
                    state.mark_stage_meta_dirty(&parent_id);
                    push_notify_timeline(parent_id);
                    notify_host = true;
                }
                WireMessage::ProviderSubagentTimelineCatchup {
                    parent_agent_id,
                    subagent_id,
                    events,
                    has_more,
                    ..
                } => {
                    let key = (parent_agent_id, subagent_id);
                    let list = state
                        .provider_subagent_timelines
                        .entry(key.clone())
                        .or_default();
                    let prev_max_seq = list.iter().map(|e| e.seq).max().unwrap_or(0);
                    let batch_max_seq = events.iter().map(|e| e.seq).max();
                    let mut seq_to_idx: HashMap<u64, usize> = list
                        .iter()
                        .enumerate()
                        .map(|(idx, e)| (e.seq, idx))
                        .collect();
                    let mut logical_to_idx: HashMap<(&'static str, String), usize> = list
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, e)| {
                            provider_subagent_logical_key(&e.payload).map(|k| (k, idx))
                        })
                        .collect();
                    let mut advanced = false;
                    let mut inserted_any = false;
                    for ev in events {
                        if ev.seq > prev_max_seq {
                            advanced = true;
                        }
                        let lkey = provider_subagent_logical_key(&ev.payload);
                        if let Some(&idx) = seq_to_idx.get(&ev.seq) {
                            list[idx] = ev;
                            if let Some(k) = lkey {
                                logical_to_idx.insert(k, idx);
                            }
                        } else if let Some(ref k) = lkey
                            && let Some(&idx) = logical_to_idx.get(k)
                        {
                            if ev.seq >= list[idx].seq {
                                seq_to_idx.remove(&list[idx].seq);
                                seq_to_idx.insert(ev.seq, idx);
                                if ev.seq > list[idx].seq {
                                    inserted_any = true;
                                }
                                list[idx] = ev;
                            }
                        } else {
                            let idx = list.len();
                            seq_to_idx.insert(ev.seq, idx);
                            if let Some(k) = lkey {
                                logical_to_idx.insert(k, idx);
                            }
                            list.push(ev);
                            inserted_any = true;
                        }
                    }
                    list.sort_by_key(|e| e.seq);
                    if has_more
                        && let Some(next_since) = batch_max_seq
                        && next_since > 0
                        && (advanced || inserted_any)
                    {
                        continue_subagent_catchup =
                            Some((key.0.clone(), key.1.clone(), next_since));
                    }
                    state.touch_provider_subagent_lru(&key);
                    state.enforce_provider_subagent_timeline_bounds(Some(&key));
                    notify_host = true;
                }
                WireMessage::ProviderSubagentTimelineLive { event } => {
                    let key = (event.parent_agent_id.clone(), event.subagent_id.clone());
                    let list = state
                        .provider_subagent_timelines
                        .entry(key.clone())
                        .or_default();
                    let lkey = provider_subagent_logical_key(&event.payload);
                    if let Some(slot) = list.iter_mut().find(|e| e.seq == event.seq) {
                        *slot = event;
                    } else if let Some(ref k) = lkey
                        && let Some(slot) = list
                            .iter_mut()
                            .find(|e| provider_subagent_logical_key(&e.payload).as_ref() == Some(k))
                    {
                        if event.seq >= slot.seq {
                            *slot = event;
                            list.sort_by_key(|e| e.seq);
                        }
                    } else {
                        list.push(event);
                    }
                    state.touch_provider_subagent_lru(&key);
                    state.enforce_provider_subagent_timeline_bounds(Some(&key));
                    notify_host = true;
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
                    rewound_seen_reset = Some((ag_id.clone(), agent.latest_seq));
                    let agent_card = {
                        let prefs_guard = self.thread_prefs.read().ok();
                        let existing_pref = prefs_guard
                            .as_ref()
                            .and_then(|p| p.get(&(host_id.to_string(), ag_id.clone())));
                        let reset_pref = ClientThreadPref {
                            host_id: host_id.to_string(),
                            agent_id: ag_id.clone(),
                            seen_seq: agent.latest_seq,
                            is_pinned: existing_pref.is_some_and(|p| p.is_pinned),
                            pinned_order: existing_pref.map(|p| p.pinned_order).unwrap_or(0),
                            updated_at_ms: existing_pref.map(|p| p.updated_at_ms).unwrap_or(0),
                        };
                        agent_to_card_view(&agent, Some(&reset_pref))
                    };
                    let mut ordered = events;
                    ordered.sort_by_key(|e| e.seq);
                    let last_seq = ordered.last().map(|e| e.seq).unwrap_or(0);
                    let is_complete = last_seq >= agent.latest_seq;
                    state.timelines.insert(ag_id.clone(), ordered.clone());
                    if is_complete {
                        state.caught_up_agents.insert(ag_id.clone());
                    } else {
                        state.caught_up_agents.remove(&ag_id);
                    }
                    state.stage_cache.remove(&ag_id);
                    state.mark_stage_dirty(&ag_id, 0, true);
                    state.rewound_agents.insert(ag_id.clone());
                    state.agents.insert(ag_id.clone(), agent.clone());
                    rewound_to_persist = Some((agent, ordered, restored_files > 0, is_complete));
                    if !is_complete {
                        continue_catchup = Some((ag_id.clone(), last_seq));
                    }
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
                    push_notify_timeline(ag_id);
                }
                WireMessage::TimelineCatchup {
                    agent_id,
                    events,
                    has_more,
                    ..
                } => {
                    let mut ordered = events;
                    ordered.sort_by_key(|e| e.seq);
                    let min_incoming_seq = ordered.first().map(|e| e.seq);
                    let max_seq = ordered.last().map(|e| e.seq);

                    let (is_backfill, min_dirty, min_aux_dirty) = {
                        let list = state.timelines.entry(agent_id.clone()).or_default();
                        let existing_max_seq = list.iter().map(|e| e.seq).max().unwrap_or(0);
                        let is_backfill = min_incoming_seq
                            .is_some_and(|min_s| existing_max_seq > 0 && min_s <= existing_max_seq);

                        let (min_dirty, min_aux_dirty) = if is_backfill {
                            let mut combined = std::mem::take(list);
                            combined.extend(ordered);
                            combined.sort_by_key(|e| (e.seq == 0, e.seq));
                            let mut rebuilt = Vec::with_capacity(combined.len());
                            for ev in combined {
                                coalesce_or_push_timeline(&mut rebuilt, ev);
                            }
                            *list = rebuilt;
                            (None, true)
                        } else {
                            let mut min_d: Option<usize> = None;
                            let mut any_aux = false;
                            for ev in ordered {
                                let idx = coalesce_or_push_timeline(list, ev);
                                any_aux |= payload_affects_auxiliary_state(&list[idx].payload);
                                let d = dirty_index_for_event(list, idx, &list[idx].payload);
                                min_d = Some(min_d.map_or(d, |prev| prev.min(d)));
                            }
                            (min_d, any_aux)
                        };
                        (is_backfill, min_dirty, min_aux_dirty)
                    };

                    if is_backfill {
                        state.rewound_agents.insert(agent_id.clone());
                        state.mark_stage_dirty(&agent_id, 0, true);
                    } else if let Some(d) = min_dirty {
                        state.mark_stage_dirty_with_aux(&agent_id, d, false, min_aux_dirty);
                    }

                    if has_more && let Some(next_since) = max_seq {
                        state.caught_up_agents.remove(&agent_id);
                        continue_catchup = Some((agent_id.clone(), next_since));
                    } else {
                        state.caught_up_agents.insert(agent_id.clone());
                        if let Some(catchup_max) = max_seq {
                            catchup_completed_cursor = Some((agent_id.clone(), catchup_max));
                        }
                    }
                    push_notify_timeline(agent_id);
                }
                WireMessage::TimelineLive { event } => {
                    let agent_id = event.agent_id.clone();
                    let live_seq = event.seq;
                    let is_status_changed =
                        matches!(event.payload, TimelinePayload::StatusChanged { .. });
                    if let TimelinePayload::StatusChanged { ref status } = event.payload
                        && let Some(ag) = state.agents.get_mut(&agent_id)
                    {
                        ag.status = status.clone();
                        notify_fleet = true;
                        notify_host = true;
                    }
                    if live_seq > 0
                        && let Some(ag) = state.agents.get_mut(&agent_id)
                        && live_seq > ag.latest_seq
                    {
                        ag.latest_seq = live_seq;
                    }
                    let list = state.timelines.entry(agent_id.clone()).or_default();
                    let idx = coalesce_or_push_timeline(list, event);
                    let dirty_idx = dirty_index_for_event(list, idx, &list[idx].payload);
                    let aux_dirty = payload_affects_auxiliary_state(&list[idx].payload);
                    state.mark_stage_dirty_with_aux(&agent_id, dirty_idx, false, aux_dirty);
                    if is_status_changed {
                        state.mark_stage_meta_dirty(&agent_id);
                    }
                    // Only advance the persistent contiguous cursor once this connection has
                    // completed TimelineCatchup for `agent_id`; never jump over an un-synced gap.
                    if live_seq > 0 && state.caught_up_agents.contains(&agent_id) {
                        live_event_cursor = Some((agent_id.clone(), live_seq));
                    }
                    push_notify_timeline(agent_id);
                }
                WireMessage::GitStatusResponse { status, .. } => {
                    let cwd = status.cwd.clone();
                    let norm_cwd = crate::sidebar::normalize_project_path(&cwd);
                    state.git_repos_by_cwd.insert(
                        norm_cwd.clone(),
                        GitReposInspectionView {
                            path: norm_cwd,
                            is_git_repo: status.is_git_repo,
                            git_branch: status.branch.clone(),
                            nested_git_repos: status.nested_git_repos.clone(),
                        },
                    );
                    state.git_statuses.insert(cwd.clone(), status);
                    notify_git_cwd = Some(cwd);
                }
                WireMessage::GitReposResponse {
                    request_id,
                    cwd,
                    is_git_repo,
                    git_branch,
                    repos,
                } => {
                    let norm_path = crate::sidebar::normalize_project_path(&cwd);
                    let view = GitReposInspectionView {
                        path: norm_path.clone(),
                        is_git_repo,
                        git_branch,
                        nested_git_repos: repos,
                    };
                    state.git_repos_by_cwd.insert(norm_path, view.clone());
                    if let Some(tx) = state.pending_git_repo_inspections.remove(&request_id) {
                        let _ = tx.send(Ok(view));
                    }
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::HostSyncStateSnapshot {
                    state: sync_state, ..
                } => {
                    state.host_sync_supported = true;
                    state.host_sync_state = sync_state.clone();
                    host_sync_to_apply = Some((sync_state, false));
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::HostSyncStateUpdated { state: sync_state } => {
                    state.host_sync_supported = true;
                    state.host_sync_state = sync_state.clone();
                    host_sync_to_apply = Some((sync_state, true));
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::WorkspaceBoardSnapshot { board, .. }
                | WireMessage::WorkspaceBoardUpdated { board } => {
                    let ws_id = board.workspace_id.clone();
                    let rev = board.revision;
                    if let Some(ws) = state
                        .workspaces
                        .iter_mut()
                        .find(|w| w.workspace_id == ws_id)
                    {
                        ws.board_revision = rev;
                    }
                    state.workspace_boards.insert(ws_id, board);
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::WorkspaceEventsSnapshot {
                    workspace_id,
                    events,
                    ..
                } => {
                    state.workspace_events.insert(workspace_id, events);
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::WorkspaceEventRecorded { event } => {
                    let ws_id = event.workspace_id.clone();
                    let list = state.workspace_events.entry(ws_id).or_default();
                    if !list.iter().any(|existing| existing.seq == event.seq) {
                        list.insert(0, event);
                        if list.len() > 200 {
                            list.truncate(200);
                        }
                    }
                    notify_fleet = true;
                    notify_host = true;
                }
                WireMessage::WorkspaceOrganizeProgress { event } => {
                    let norm_root = crate::sidebar::normalize_project_path(&event.project_root);
                    state
                        .workspace_organize_status
                        .insert(norm_root, event.clone());
                    state
                        .workspace_organize_status
                        .insert(event.project_root.clone(), event);
                    notify_fleet = true;
                    notify_host = true;
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
                WireMessage::WorkspaceSymbolsResponse {
                    request_id,
                    symbols,
                    truncated,
                    ..
                } => {
                    if let Some(tx) = state.pending_symbol_searches.remove(&request_id) {
                        let _ = tx.send(Ok((symbols, truncated)));
                    }
                }
                WireMessage::WorkspaceFileContentResponse {
                    request_id,
                    resolved_path,
                    content_kind,
                    mime_type,
                    content,
                    size_bytes,
                    truncated,
                    etag,
                    mtime_unix_ms,
                    writable,
                    line_ending,
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
                            etag,
                            mtime_unix_ms,
                            writable,
                            line_ending,
                        }));
                    }
                }
                WireMessage::WorkspaceFileWritten {
                    request_id,
                    cwd,
                    path,
                    etag,
                    size_bytes,
                    mtime_unix_ms,
                    conflict,
                } => {
                    if let Some(tx) = state.pending_file_writes.remove(&request_id) {
                        let _ = tx.send(Ok(WorkspaceFileWriteResultView {
                            host_id: state.profile.host_id.clone(),
                            cwd,
                            path,
                            etag,
                            size_bytes,
                            mtime_unix_ms,
                            conflict,
                        }));
                    }
                }
                WireMessage::WorkspaceEntryMutated {
                    request_id,
                    cwd,
                    path,
                    new_path,
                    action,
                } => {
                    if let Some(tx) = state.pending_file_mutations.remove(&request_id) {
                        let _ = tx.send(Ok(WorkspaceEntryMutationResultView {
                            host_id: state.profile.host_id.clone(),
                            cwd,
                            path,
                            new_path,
                            action,
                        }));
                    }
                }
                WireMessage::InstructionFilesSnapshot {
                    request_id,
                    cwd,
                    files,
                    total_tokens,
                    merged_preview,
                } => {
                    if let Some(tx) = state.pending_instruction_lists.remove(&request_id) {
                        let _ = tx.send(Ok(InstructionFilesSnapshotView {
                            host_id: state.profile.host_id.clone(),
                            cwd,
                            files,
                            total_tokens,
                            merged_preview,
                        }));
                    }
                }
                WireMessage::InstructionFileContentResponse {
                    request_id,
                    cwd,
                    path,
                    content,
                    etag,
                    exists,
                } => {
                    if let Some(tx) = state.pending_instruction_reads.remove(&request_id) {
                        let _ = tx.send(Ok(InstructionFileContentView {
                            host_id: state.profile.host_id.clone(),
                            cwd,
                            path,
                            content,
                            etag,
                            exists,
                        }));
                    }
                }
                WireMessage::InstructionFileSavedResponse {
                    request_id,
                    cwd,
                    path,
                    etag,
                    size_bytes,
                    approx_tokens,
                } => {
                    if let Some(tx) = state.pending_instruction_saves.remove(&request_id) {
                        let _ = tx.send(Ok(InstructionFileSavedView {
                            host_id: state.profile.host_id.clone(),
                            cwd,
                            path,
                            etag,
                            size_bytes,
                            approx_tokens,
                        }));
                    }
                }
                WireMessage::PathRulesSnapshot {
                    request_id,
                    cwd,
                    rules,
                    total_always_tokens,
                    token_budget_warning,
                } => {
                    if !state.path_rules_by_cwd.contains_key(&cwd)
                        && state.path_rules_by_cwd.len() >= 24
                        && let Some(evict_key) = state.path_rules_by_cwd.keys().next().cloned()
                    {
                        state.path_rules_by_cwd.remove(&evict_key);
                    }
                    state.path_rules_by_cwd.insert(cwd.clone(), rules.clone());
                    if let Some(tx) = state.pending_path_rules.remove(&request_id) {
                        let _ = tx.send(Ok(PathRulesSnapshotView {
                            host_id: state.profile.host_id.clone(),
                            cwd,
                            rules,
                            total_always_tokens,
                            token_budget_warning,
                        }));
                    }
                }
                WireMessage::MatchedPathRulesResponse {
                    request_id,
                    cwd,
                    matched,
                    injected_markdown,
                    total_tokens,
                } => {
                    if let Some(tx) = state.pending_path_rule_matches.remove(&request_id) {
                        let _ = tx.send(Ok(MatchedPathRulesView {
                            host_id: state.profile.host_id.clone(),
                            cwd,
                            matched,
                            injected_markdown,
                            total_tokens,
                        }));
                    }
                }
                WireMessage::LspServersSnapshot {
                    request_id,
                    servers,
                    ..
                } => {
                    if let Some(tx) = state.pending_lsp_servers.remove(&request_id) {
                        let _ = tx.send(Ok(servers));
                    }
                }
                WireMessage::LspSymbolsResponse {
                    request_id,
                    symbols,
                    ..
                } => {
                    if let Some(tx) = state.pending_lsp_symbols.remove(&request_id) {
                        let _ = tx.send(Ok(symbols));
                    }
                }
                WireMessage::LspDiagnosticsResponse {
                    request_id,
                    diagnostics,
                    ..
                } => {
                    if let Some(tx) = state.pending_lsp_diagnostics.remove(&request_id) {
                        let _ = tx.send(Ok(diagnostics));
                    }
                }
                WireMessage::LspLocationsResponse {
                    request_id,
                    locations,
                    ..
                } => {
                    if let Some(tx) = state.pending_lsp_locations.remove(&request_id) {
                        let _ = tx.send(Ok(locations));
                    }
                }
                WireMessage::UsageInsightsResponse {
                    request_id,
                    snapshot,
                } => {
                    if let Some(tx) = state.pending_usage_insights.remove(&request_id) {
                        let _ = tx.send(Ok(snapshot));
                    }
                }
                WireMessage::ContextOccupancyResponse {
                    request_id,
                    breakdown,
                } => {
                    if let Some(tx) = state.pending_context_occupancy.remove(&request_id) {
                        let _ = tx.send(Ok(breakdown));
                    }
                }
                WireMessage::LifecycleHooksSnapshot {
                    request_id,
                    hooks,
                    recent_executions,
                    project_hooks_trusted,
                } => {
                    if let Some(tx) = state.pending_lifecycle_hooks.remove(&request_id) {
                        let _ = tx.send(Ok(LifecycleHooksSnapshotView {
                            hooks,
                            recent_executions,
                            project_hooks_trusted,
                        }));
                    }
                }
                WireMessage::HookTestResultResponse { request_id, record } => {
                    if let Some(tx) = state.pending_lifecycle_hook_tests.remove(&request_id) {
                        let _ = tx.send(Ok(record));
                    }
                }
                WireMessage::EventTriggersSnapshot {
                    request_id,
                    triggers,
                    recent_deliveries,
                    base_webhook_url,
                } => {
                    if let Some(tx) = state.pending_event_triggers.remove(&request_id) {
                        let _ = tx.send(Ok(EventTriggersSnapshotView {
                            triggers,
                            recent_deliveries,
                            base_webhook_url,
                        }));
                    }
                }
                WireMessage::EventTriggerTestResponse {
                    request_id,
                    delivery,
                } => {
                    if let Some(tx) = state.pending_event_trigger_tests.remove(&request_id) {
                        let _ = tx.send(Ok(delivery));
                    }
                }
                WireMessage::ConditionalTimerCreated { request_id, timer } => {
                    upsert_conditional_timer_record(&mut state.conditional_timers, timer.clone());
                    if let Some(tx) = state.pending_timer_creates.remove(&request_id) {
                        let _ = tx.send(Ok(timer));
                    }
                    notify_host = true;
                }
                WireMessage::ConditionalTimersSnapshot { request_id, timers } => {
                    if state.background_timer_list_reqs.remove(&request_id) {
                        state.conditional_timers = timers.clone();
                        notify_host = true;
                    }
                    if let Some(tx) = state.pending_timer_lists.remove(&request_id) {
                        let _ = tx.send(Ok(timers));
                    }
                }
                WireMessage::ConditionalTimerUpdated { request_id, timer } => {
                    upsert_conditional_timer_record(&mut state.conditional_timers, timer.clone());
                    if let Some(tx) = state.pending_timer_updates.remove(&request_id) {
                        let _ = tx.send(Ok(timer));
                    }
                    notify_host = true;
                }
                WireMessage::ConditionalTimerEvent { timer } => {
                    upsert_conditional_timer_record(&mut state.conditional_timers, timer);
                    notify_host = true;
                }
                WireMessage::GuardianConfigSnapshot {
                    request_id,
                    config,
                    recent_audits,
                } => {
                    if let Some(tx) = state.pending_guardian_configs.remove(&request_id) {
                        let _ = tx.send(Ok(GuardianConfigSnapshotView {
                            config,
                            recent_audits,
                        }));
                    }
                }
                WireMessage::ImChannelsSnapshot {
                    request_id,
                    channels,
                    bindings,
                    base_webhook_url,
                } => {
                    if let Some(tx) = state.pending_im_channels.remove(&request_id) {
                        let _ = tx.send(Ok(ImChannelsSnapshotView {
                            channels,
                            bindings,
                            base_webhook_url,
                        }));
                    }
                }
                WireMessage::ImChannelTestResponse {
                    request_id,
                    ok,
                    detail,
                } => {
                    if let Some(tx) = state.pending_im_channel_tests.remove(&request_id) {
                        let _ = tx.send(Ok(ImChannelTestResultView { ok, detail }));
                    }
                }
                WireMessage::PushDevicesSnapshot {
                    request_id,
                    devices,
                    preferences,
                } => {
                    if let Some(tx) = state.pending_push_snapshots.remove(&request_id) {
                        let _ = tx.send(Ok(PushDevicesSnapshotView {
                            devices,
                            preferences,
                        }));
                    }
                }
                WireMessage::PushTestResponse {
                    request_id,
                    attempted,
                    delivered,
                    detail,
                } => {
                    if let Some(tx) = state.pending_push_tests.remove(&request_id) {
                        let _ = tx.send(Ok(PushTestResultView {
                            attempted,
                            delivered,
                            detail,
                        }));
                    }
                }
                WireMessage::GoalRunsSnapshot { request_id, goals } => {
                    if let Some(tx) = state.pending_goal_lists.remove(&request_id) {
                        let _ = tx.send(Ok(goals.clone()));
                    } else {
                        state.goal_runs = goals;
                        notify_host = true;
                    }
                }
                WireMessage::GoalRunUpdated { request_id, goal } => {
                    upsert_cached_goal(&mut state.goal_runs, goal.clone());
                    if let Some(tx) = state.pending_goal_updates.remove(&request_id) {
                        let _ = tx.send(Ok(goal));
                    }
                    notify_host = true;
                }
                WireMessage::GoalRunEvent { goal } => {
                    upsert_cached_goal(&mut state.goal_runs, goal);
                    notify_host = true;
                }
                WireMessage::MonitorWatchesSnapshot {
                    request_id,
                    monitors,
                } => {
                    if let Some(tx) = state.pending_monitor_lists.remove(&request_id) {
                        let _ = tx.send(Ok(monitors.clone()));
                    } else {
                        state.monitor_watches = monitors;
                        notify_host = true;
                    }
                }
                WireMessage::MonitorWatchUpdated {
                    request_id,
                    monitor,
                } => {
                    upsert_cached_monitor(&mut state.monitor_watches, monitor.clone());
                    if let Some(tx) = state.pending_monitor_updates.remove(&request_id) {
                        let _ = tx.send(Ok(monitor));
                    }
                    notify_host = true;
                }
                WireMessage::MonitorWatchEvent { monitor } => {
                    upsert_cached_monitor(&mut state.monitor_watches, monitor);
                    notify_host = true;
                }
                WireMessage::BrowserSessionsSnapshot {
                    request_id,
                    sessions,
                    policy,
                    chrome_binary,
                } => {
                    if let Some(tx) = state.pending_browser_snapshots.remove(&request_id) {
                        let _ = tx.send(Ok(BrowserSessionsSnapshotView {
                            host_id: host_id.to_string(),
                            sessions,
                            policy,
                            chrome_binary,
                        }));
                    }
                }
                WireMessage::BrowserSessionUpdated {
                    request_id,
                    session,
                } => {
                    if let Some(tx) = state.pending_browser_sessions.remove(&request_id) {
                        let _ = tx.send(Ok(session));
                    }
                }
                WireMessage::BrowserActionResponse {
                    request_id,
                    session,
                    elements,
                    screenshot_base64,
                    text_excerpt,
                    captcha_detected,
                } => {
                    if let Some(tx) = state.pending_browser_actions.remove(&request_id) {
                        let _ = tx.send(Ok(BrowserActionResponseView {
                            host_id: host_id.to_string(),
                            session,
                            elements,
                            screenshot_base64,
                            text_excerpt,
                            captcha_detected,
                        }));
                    }
                }
                WireMessage::WorkflowRunsSnapshot { request_id, runs } => {
                    if let Some(tx) = state.pending_workflow_lists.remove(&request_id) {
                        let _ = tx.send(Ok(runs.clone()));
                    } else {
                        state.workflow_runs = runs;
                        notify_host = true;
                    }
                }
                WireMessage::WorkflowRunUpdated { request_id, run } => {
                    upsert_cached_workflow_run(&mut state.workflow_runs, run.clone());
                    if let Some(tx) = state.pending_workflow_updates.remove(&request_id) {
                        let _ = tx.send(Ok(run));
                    }
                    notify_host = true;
                }
                WireMessage::WorkflowRunEvent { run } => {
                    upsert_cached_workflow_run(&mut state.workflow_runs, run);
                    notify_host = true;
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
                WireMessage::TerminalAttached {
                    request_id,
                    terminal,
                    slot,
                } => {
                    if let Some(tx) = state.pending_upgrade_terminals.remove(&request_id) {
                        let _ = tx.send(Ok(terminal.terminal_id));
                    } else {
                        let slot = slot.max(1);
                        state
                            .slot_terminal_ids
                            .insert(slot, terminal.terminal_id.clone());
                        let _ = state.emulator_slot_mut(slot);
                        state.active_terminal_id = Some(terminal.terminal_id.clone());
                        state.active_terminal_slot = slot;
                        state
                            .terminals
                            .retain(|t| t.terminal_id != terminal.terminal_id);
                        state.terminals.insert(0, terminal);
                        notify_host = true;
                    }
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
                    clear_pending_queue_op_by_req(state, request_id, false);
                    if let Some(tx) = state.pending_daemon_upgrades.remove(&request_id) {
                        let _ = tx.send(Ok(()));
                    }
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
                    if let Some(req_id) = request_id
                        && clear_pending_queue_op_by_req(state, req_id, true)
                    {
                        notify_host = true;
                    }
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
                        && let Some(tx) = state.pending_symbol_searches.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_file_writes.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_file_mutations.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_instruction_lists.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_instruction_reads.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_instruction_saves.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_rewinds.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_forks.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_handoff_previews.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_handoff_executes.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_daemon_upgrades.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_upgrade_terminals.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_git_repo_inspections.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_acp_probes.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_acp_snapshots.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_mcp_snapshots.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_mcp_probes.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some((_, tx)) = state.pending_artifact_lists.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_artifact_reads.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_artifact_upserts.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_path_rules.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_path_rule_matches.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_artifact_deletes.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some((_, tx)) = state.pending_managed_skills_lists.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_managed_skill_reads.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_lsp_servers.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_lsp_symbols.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_lsp_diagnostics.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_lsp_locations.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_usage_insights.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_context_occupancy.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_lifecycle_hooks.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_lifecycle_hook_tests.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_event_triggers.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_event_trigger_tests.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_timer_creates.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_timer_lists.remove(&req_id)
                    {
                        state.background_timer_list_reqs.remove(&req_id);
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_timer_updates.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && state.background_timer_list_reqs.remove(&req_id)
                    {
                        // Ignore unsupported/transient background timer list errors on older daemons.
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_guardian_configs.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_goal_updates.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_goal_lists.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_monitor_updates.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_monitor_lists.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_browser_snapshots.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_browser_sessions.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_browser_actions.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_workflow_updates.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_workflow_lists.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_im_channels.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_im_channel_tests.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_push_snapshots.remove(&req_id)
                    {
                        let _ = tx.send(Err(formatted_err));
                    } else if let Some(req_id) = request_id
                        && let Some(tx) = state.pending_push_tests.remove(&req_id)
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
                                push_notify_timeline(opt_id);
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
                                push_notify_timeline(agent_id);
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
        if !initial_seen_seeds.is_empty()
            && let Ok(mut prefs) = self.thread_prefs.write()
        {
            let now_ms = unix_ms_now() as i64;
            for (ag_id, seed_seq) in initial_seen_seeds {
                let key = (host_id.to_string(), ag_id.clone());
                prefs.entry(key).or_insert_with(|| {
                    let _ = self
                        .store
                        .set_client_thread_seen_seq(host_id, &ag_id, seed_seq, false, now_ms);
                    ClientThreadPref {
                        host_id: host_id.to_string(),
                        agent_id: ag_id,
                        seen_seq: seed_seq,
                        is_pinned: false,
                        pinned_order: 0,
                        updated_at_ms: now_ms,
                    }
                });
            }
        }
        if !server_pref_syncs.is_empty()
            && let Ok(mut prefs) = self.thread_prefs.write()
        {
            let now_ms = unix_ms_now() as i64;
            for (ag_id, server_read_seq, pin_sync) in server_pref_syncs {
                let key = (host_id.to_string(), ag_id.clone());
                let entry = prefs.entry(key).or_insert_with(|| ClientThreadPref {
                    host_id: host_id.to_string(),
                    agent_id: ag_id.clone(),
                    seen_seq: server_read_seq,
                    is_pinned: pin_sync.map(|(p, _)| p).unwrap_or(false),
                    pinned_order: pin_sync.map(|(_, o)| o).unwrap_or(0),
                    updated_at_ms: now_ms,
                });
                let mut dirty = false;
                if server_read_seq > entry.seen_seq {
                    entry.seen_seq = server_read_seq;
                    dirty = true;
                }
                if let Some((srv_pinned, srv_order)) = pin_sync
                    && (entry.is_pinned != srv_pinned
                        || (srv_pinned && srv_order > 0 && entry.pinned_order != srv_order))
                {
                    entry.is_pinned = srv_pinned;
                    entry.pinned_order = if srv_pinned {
                        if srv_order > 0 {
                            srv_order
                        } else {
                            self.pin_order_seq.fetch_add(1, Ordering::SeqCst) as i64
                        }
                    } else {
                        0
                    };
                    dirty = true;
                }
                if dirty {
                    entry.updated_at_ms = entry.updated_at_ms.max(now_ms);
                    let _ = self.store.upsert_client_thread_pref(entry);
                }
            }
        }
        if let Some((sync_state, is_live_update)) = host_sync_to_apply {
            self.reconcile_host_sync_state(host_id, sync_state, is_live_update);
        }
        if let Some((opt_id, real_id)) = migrated_opt_pref {
            let _ = self
                .store
                .migrate_client_thread_pref(host_id, &opt_id, &real_id);
            let mut migrated_pinned = false;
            if let Ok(mut prefs) = self.thread_prefs.write()
                && let Some(mut old_pref) = prefs.remove(&(host_id.to_string(), opt_id))
            {
                migrated_pinned = old_pref.is_pinned;
                let real_key = (host_id.to_string(), real_id.clone());
                if let std::collections::hash_map::Entry::Vacant(e) = prefs.entry(real_key) {
                    old_pref.agent_id = real_id.clone();
                    e.insert(old_pref);
                }
            }
            if migrated_pinned {
                self.dispatch_wire_or_queue(
                    host_id,
                    WireMessage::SetAgentPinned {
                        request_id: Uuid::new_v4(),
                        agent_id: real_id,
                        pinned: true,
                        pinned_order: None,
                    },
                );
            }
        }
        if let Some((rewound_ag_id, rewound_latest_seq)) = rewound_seen_reset {
            let now_ms = unix_ms_now() as i64;
            let _ = self.store.set_client_thread_seen_seq(
                host_id,
                &rewound_ag_id,
                rewound_latest_seq,
                true,
                now_ms,
            );
            if let Ok(mut prefs) = self.thread_prefs.write() {
                let key = (host_id.to_string(), rewound_ag_id.clone());
                let entry = prefs.entry(key).or_insert_with(|| ClientThreadPref {
                    host_id: host_id.to_string(),
                    agent_id: rewound_ag_id,
                    seen_seq: rewound_latest_seq,
                    is_pinned: false,
                    pinned_order: 0,
                    updated_at_ms: now_ms,
                });
                entry.seen_seq = rewound_latest_seq;
                entry.updated_at_ms = now_ms;
            }
        }
        if let Some((ag, ws_opt)) = forked_to_persist {
            if let Some(ref ws) = ws_opt {
                let _ = self.store.upsert_single_replica_workspace(host_id, ws);
            }
            let _ = self.store.upsert_single_replica_agent(host_id, &ag);
        }
        if let Some((ag, evs, files_restored, is_complete)) = rewound_to_persist {
            let _ = self
                .store
                .replace_replica_timeline_for_host(host_id, &ag.agent_id, &evs);
            if is_complete {
                let _ = self
                    .store
                    .set_replica_contiguous_seq(&ag.agent_id, ag.latest_seq);
            }
            let _ = self.store.upsert_single_replica_agent(host_id, &ag);
            if files_restored {
                self.fetch_git_status(host_id, &ag.cwd);
                self.fetch_git_diff(host_id, &ag.cwd, None, false);
            }
        }
        if let Some((ag_id, max_seq)) = catchup_completed_cursor
            && max_seq > 0
        {
            let advanced = self
                .store
                .advance_replica_contiguous_seq(&ag_id, max_seq, true)
                .unwrap_or(false);
            if !advanced
                && let Ok(mut guard) = self.hosts.write()
                && let Some(h) = guard.get_mut(host_id)
            {
                h.caught_up_agents.remove(&ag_id);
            }
        }
        if let Some((ag_id, live_seq)) = live_event_cursor
            && live_seq > 0
        {
            let advanced = self
                .store
                .advance_replica_contiguous_seq(&ag_id, live_seq, true)
                .unwrap_or(false);
            if !advanced
                && let Ok(mut guard) = self.hosts.write()
                && let Some(h) = guard.get_mut(host_id)
            {
                h.caught_up_agents.remove(&ag_id);
            }
        }
        for (stale_ag_id, should_resubscribe) in stale_replica_agents {
            let _ = self
                .store
                .replace_replica_timeline_for_host(host_id, &stale_ag_id, &[]);
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
        if let Some((parent_agent_id, subagent_id, since_seq)) = continue_subagent_catchup {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::SubscribeProviderSubagent {
                    request_id: Uuid::new_v4(),
                    parent_agent_id,
                    subagent_id,
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
        for agent_id in notify_timeline_agents {
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
                    let slot = slot.max(1);
                    state.emulator_slot_mut(slot).process(&data);
                    raw_out = Some((slot, data));
                }
                TerminalBinaryFrame::Snapshot { slot, data } => {
                    let slot = slot.max(1);
                    let emu = state.emulator_slot_mut(slot);
                    emu.clear();
                    emu.process(&data);
                    raw_out = Some((slot, data));
                }
                TerminalBinaryFrame::Resize { slot, cols, rows } => {
                    let slot = slot.max(1);
                    state.emulator_slot_mut(slot).resize(cols, rows);
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

    fn reconcile_host_sync_state(
        &self,
        host_id: &str,
        sync_state: HostSyncState,
        is_live_update: bool,
    ) {
        let now_ms = unix_ms_now() as i64;
        // 1. Reconcile saved projects for this host.
        let mut to_seed_to_daemon: Vec<HostProjectEntry> = Vec::new();
        if let Ok(mut projs) = self.projects.write() {
            if !is_live_update && sync_state.projects.is_empty() && sync_state.updated_at_ms == 0 {
                // First connect to a freshly upgraded daemon: seed existing client-local projects upward
                // so upgrading users never lose their saved Desktop sidebar projects.
                for p in projs.values().filter(|p| p.host_id == host_id) {
                    to_seed_to_daemon.push(HostProjectEntry {
                        project_id: p.project_id.clone(),
                        name: p.name.clone(),
                        directories: p.directories.clone(),
                        sort_order: 0,
                        hidden: false,
                        updated_at_ms: p.updated_at_ms,
                    });
                }
            } else {
                let incoming_ids: HashSet<&str> = sync_state
                    .projects
                    .iter()
                    .map(|p| p.project_id.as_str())
                    .collect();
                let stale_ids: Vec<String> = projs
                    .iter()
                    .filter(|(_, p)| {
                        p.host_id == host_id && !incoming_ids.contains(p.project_id.as_str())
                    })
                    .map(|(id, _)| id.clone())
                    .collect();
                for stale_id in stale_ids {
                    projs.remove(&stale_id);
                    let _ = self.store.delete_client_project(&stale_id);
                }
                for entry in &sync_state.projects {
                    let stored = StoredClientProject {
                        project_id: entry.project_id.clone(),
                        host_id: host_id.to_string(),
                        name: entry.name.clone(),
                        directories: entry.directories.clone(),
                        updated_at_ms: entry.updated_at_ms.max(now_ms.min(1)),
                    };
                    let _ = self.store.upsert_client_project(&stored);
                    projs.insert(
                        entry.project_id.clone(),
                        ClientProjectView {
                            project_id: stored.project_id,
                            host_id: stored.host_id,
                            name: stored.name,
                            directories: stored.directories,
                            updated_at_ms: stored.updated_at_ms,
                        },
                    );
                }
            }
        }
        for seed in to_seed_to_daemon {
            self.dispatch_wire_or_queue(
                host_id,
                WireMessage::UpsertHostProject {
                    request_id: Uuid::new_v4(),
                    project: seed,
                },
            );
        }

        // 2. Reconcile manual project order for this host.
        if is_live_update || !sync_state.project_order.is_empty() {
            let host_project_ids: HashSet<String> = self
                .projects
                .read()
                .ok()
                .map(|g| {
                    g.values()
                        .filter(|p| p.host_id == host_id)
                        .map(|p| p.project_id.clone())
                        .collect()
                })
                .unwrap_or_default();
            let host_prefix = format!("{host_id}::");
            let belongs_to_host = |key: &str| -> bool {
                host_project_ids.contains(key) || key.starts_with(&host_prefix)
            };
            if let Ok(mut order) = self.project_order.write() {
                let first_pos = order.iter().position(|k| belongs_to_host(k));
                let mut others: Vec<String> = order
                    .iter()
                    .filter(|k| !belongs_to_host(k))
                    .cloned()
                    .collect();
                let insert_at = first_pos.unwrap_or(others.len()).min(others.len());
                let mut merged = Vec::with_capacity(others.len() + sync_state.project_order.len());
                merged.extend(others.drain(..insert_at));
                for k in sync_state.project_order {
                    if !merged.contains(&k) {
                        merged.push(k);
                    }
                }
                for k in others {
                    if !merged.contains(&k) {
                        merged.push(k);
                    }
                }
                if *order != merged {
                    *order = merged;
                    if let Ok(json) = serde_json::to_string(&*order) {
                        let _ = self.store.set_client_kv("project_order", &json, now_ms);
                    }
                }
            }
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

    /// Sends immediately if connected, or queues into `pending_outbox` without requiring `&Arc<Self>`,
    /// so `&self` sync mutations (read state, pins, projects, drag order) converge reliably across reconnects.
    fn dispatch_wire_or_queue(&self, host_id: &str, msg: WireMessage) {
        let mut live_handle = None;
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ref handle) = h.session {
                live_handle = Some(handle.clone());
            } else {
                h.pending_outbox.push(msg.clone());
            }
        }
        if let Some(handle) = live_handle {
            let hosts = Arc::clone(&self.hosts);
            let owned_host = host_id.to_string();
            self.runtime.spawn(async move {
                if handle.send_wire(msg.clone()).await.is_err()
                    && let Ok(mut guard) = hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_outbox.push(msg);
                }
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
            WireMessage::GetHostSyncState {
                request_id: Uuid::new_v4(),
            },
        );
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
                include_archived: true,
                include_background: false,
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::ListAgents {
                request_id: Uuid::new_v4(),
                workspace_id: None,
                include_archived: true,
                include_background: false,
                parent_agent_id: None,
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
            WireMessage::ListProfiles {
                request_id: Uuid::new_v4(),
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::ListAcpAgents {
                request_id: Uuid::new_v4(),
            },
        );
        let list_artifacts_req_id = Uuid::new_v4();
        if let Ok(mut hosts) = self.hosts.write()
            && let Some(h) = hosts.get_mut(host_id)
        {
            if h.unsolicited_unfiltered_artifact_reqs.len() >= 64 {
                h.unsolicited_unfiltered_artifact_reqs.clear();
            }
            h.unsolicited_unfiltered_artifact_reqs
                .insert(list_artifacts_req_id);
        }
        self.dispatch_wire(
            host_id,
            WireMessage::ListArtifacts {
                request_id: list_artifacts_req_id,
                workspace_id: None,
                agent_id: None,
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::ListSchedules {
                request_id: Uuid::new_v4(),
                include_completed: true,
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::ListConnectedHosts {
                request_id: Uuid::new_v4(),
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::GetOrchestrationConfig {
                request_id: Uuid::new_v4(),
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::GetKeepAwakeStatus {
                request_id: Uuid::new_v4(),
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::ListOrchestrationSkills {
                request_id: Uuid::new_v4(),
            },
        );
        let bg_skills_req_id = Uuid::new_v4();
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if h.background_managed_skills_reqs.len() >= 64 {
                h.background_managed_skills_reqs.clear();
            }
            h.background_managed_skills_reqs.insert(bg_skills_req_id);
        }
        self.dispatch_wire(
            host_id,
            WireMessage::ListManagedSkills {
                request_id: bg_skills_req_id,
                cwd: None,
            },
        );
        self.dispatch_wire(
            host_id,
            WireMessage::ListPendingPermissions {
                request_id: Uuid::new_v4(),
                agent_id: None,
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
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode,
            initial_prompt,
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch,
            workspace_id: None,
            handle: None,
            auto_cluster: None,
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
            acp_agent_id: None,
            model,
            effort: None,
            fast_mode: None,
            mode,
            initial_prompt,
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch,
            workspace_id: None,
            handle: None,
            auto_cluster: None,
        })
    }

    pub fn create_agent_with_config(self: &Arc<Self>, req: CreateAgentRequest<'_>) -> String {
        self.create_agent_with_profile(req, None)
    }

    pub fn create_agent_with_profile(
        self: &Arc<Self>,
        req: CreateAgentRequest<'_>,
        profile_id: Option<String>,
    ) -> String {
        let clean_branch = req.worktree_branch.map(str::trim).filter(|b| !b.is_empty());
        let use_auto_worktree = clean_branch == Some("auto");
        let explicit_ws = req
            .workspace_id
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(ToOwned::to_owned);
        let use_worktree = use_auto_worktree || clean_branch.is_some();

        let req_id = Uuid::new_v4();
        let opt_id = format!("opt_{req_id}");
        let now = Utc::now();
        let clean_model = req.model.filter(|m| !m.trim().is_empty());
        let clean_acp_agent_id = req
            .acp_agent_id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty());
        let clean_handle = req
            .handle
            .map(|h| h.trim().trim_start_matches('@').to_string())
            .filter(|h| !h.is_empty());
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
            let opt_fast_mode = req.fast_mode.unwrap_or_else(|| {
                profile_id
                    .as_deref()
                    .and_then(|pid| {
                        h.profiles
                            .iter()
                            .find(|p| p.profile_id == pid || p.name == pid)
                            .map(|p| p.fast_mode)
                    })
                    .unwrap_or(false)
            });
            let opt_agent = AgentSummary {
                agent_id: opt_id.clone(),
                workspace_id: explicit_ws
                    .clone()
                    .unwrap_or_else(|| format!("ws_{opt_id}")),
                cwd: req.cwd.to_string(),
                title,
                provider: req.provider,
                model: clean_model.clone(),
                effort: req.effort,
                fast_mode: opt_fast_mode,
                mode: req.mode.clone(),
                status: AgentStatus::Initializing,
                parent_agent_id: None,
                parent_host_id: None,
                profile_id: profile_id.clone(),
                background: false,
                auto_archive: false,
                attention: AttentionState::None,
                archived_at: None,
                last_activity_at: Some(now),
                queued_prompts: Vec::new(),
                labels: HashMap::new(),
                latest_seq: 0,
                read_seq: 0,
                is_pinned: false,
                pinned_order: 0,
                handle: clean_handle.clone(),
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: None,
                forked_at_seq: None,
                acp_agent_id: clean_acp_agent_id.clone(),
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
            h.mark_stage_dirty(&opt_id, 0, true);
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
                fast_mode: req.fast_mode,
                mode: req.mode,
                initial_prompt: req.initial_prompt.to_string(),
                initial_display_prompt: req.initial_display_prompt,
                initial_badges: req.initial_badges,
                worktree: use_worktree,
                workspace_id: explicit_ws,
                parent_agent_id: None,
                parent_host_id: None,
                profile_id,
                background: false,
                auto_archive: false,
                notify_on_finish: None,
                idempotency_key: None,
                base_ref: None,
                worktree_mode: WorktreeCreateMode::BranchOff,
                branch: clean_branch.filter(|b| *b != "auto").map(ToOwned::to_owned),
                handle: clean_handle,
                auto_cluster: req.auto_cluster,
                acp_agent_id: clean_acp_agent_id,
            },
        );

        opt_id
    }

    fn safe_catchup_since_seq(&self, agent_id: &str, mem_max_seq: u64) -> u64 {
        let contiguous_seq = self.store.get_replica_contiguous_seq(agent_id).unwrap_or(0);
        // Only perform incremental catchup when a contiguous replica cursor has been confirmed;
        // otherwise start from `0` so any historical gap from a prior interrupted sync self-heals.
        if contiguous_seq == 0 {
            0
        } else {
            contiguous_seq.min(mem_max_seq)
        }
    }

    pub fn open_agent_timeline(self: &Arc<Self>, host_id: &str, agent_id: &str) {
        if agent_id.starts_with("opt_") {
            return;
        }
        let mem_max_seq = if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.caught_up_agents.remove(agent_id);
            self.hydrate_timeline_if_missing(h, agent_id);
            h.timelines
                .get(agent_id)
                .and_then(|list| list.iter().map(|e| e.seq).max())
                .unwrap_or(0)
        } else {
            0
        };
        let since_seq = self.safe_catchup_since_seq(agent_id, mem_max_seq);
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
        self.send_prompt_with_behavior(
            host_id,
            agent_id,
            prompt,
            display_prompt,
            badges,
            ActiveTurnBehavior::Steer,
        );
    }

    pub fn send_prompt_with_behavior(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        prompt: &str,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
        active_turn_behavior: ActiveTurnBehavior,
    ) {
        let now = Utc::now();
        let req_id = Uuid::new_v4();
        let visible_text = display_prompt.clone().unwrap_or_else(|| prompt.to_string());
        let opt_msg_id = format!("u_{}", Uuid::new_v4().simple());
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ag) = h.agents.get_mut(agent_id) {
                ag.status = AgentStatus::Running {
                    turn_id: "pending".into(),
                };
                ag.archived_at = None;
                ag.updated_at = now;
            }
            if !agent_id.starts_with("opt_") {
                h.agent_req_by_req_id.insert(req_id, agent_id.to_string());
            }
            let list = h.timelines.entry(agent_id.to_string()).or_default();
            let push_idx = list.len();
            list.push(TimelineEvent {
                seq: 0,
                epoch: 1,
                agent_id: agent_id.to_string(),
                turn_id: None,
                timestamp: now,
                payload: TimelinePayload::UserPrompt {
                    message_id: opt_msg_id.clone(),
                    text: visible_text,
                    badges: badges.clone(),
                },
            });
            h.mark_stage_dirty(agent_id, push_idx, false);
            h.mark_stage_meta_dirty(agent_id);
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
                active_turn_behavior,
                message_id: Some(opt_msg_id),
                caller_agent_id: None,
                notify_on_finish: None,
            },
        );
    }

    pub fn enqueue_prompt(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        prompt: &str,
        display_prompt: Option<String>,
        badges: Vec<PromptBadge>,
        queue_id: Option<String>,
    ) -> String {
        let qid = queue_id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("q_{}", Uuid::new_v4().simple()));
        let now = Utc::now();
        let req_id = Uuid::new_v4();
        let visible_text = display_prompt.clone().unwrap_or_else(|| prompt.to_string());

        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            let qv = QueuedPromptView {
                queue_id: qid.clone(),
                display_text: visible_text,
                badges: badges.clone(),
                created_at: now,
            };
            if !ag.queued_prompts.iter().any(|q| q.queue_id == qid) {
                ag.queued_prompts.push(qv.clone());
            }
            if let Some(rems) = h.pending_queue_removals.get_mut(agent_id) {
                rems.retain(|(_, rem_id)| rem_id != &qid);
            }
            h.pending_queue_adds
                .entry(agent_id.to_string())
                .or_default()
                .push((req_id, qv));
            h.mark_stage_meta_dirty(agent_id);
        }

        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });

        self.dispatch_wire_queued(
            host_id,
            WireMessage::EnqueuePrompt {
                request_id: req_id,
                agent_id: agent_id.to_string(),
                prompt: prompt.to_string(),
                display_prompt,
                badges,
                queue_id: Some(qid.clone()),
            },
        );
        qid
    }

    pub fn remove_queued_prompt(self: &Arc<Self>, host_id: &str, agent_id: &str, queue_id: &str) {
        let req_id = Uuid::new_v4();
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            ag.queued_prompts.retain(|q| q.queue_id != queue_id);
            if let Some(adds) = h.pending_queue_adds.get_mut(agent_id) {
                adds.retain(|(_, qv)| qv.queue_id != queue_id);
            }
            h.pending_queue_removals
                .entry(agent_id.to_string())
                .or_default()
                .push((req_id, queue_id.to_string()));
            h.mark_stage_meta_dirty(agent_id);
        }

        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });

        self.dispatch_wire_queued(
            host_id,
            WireMessage::RemoveQueuedPrompt {
                request_id: req_id,
                agent_id: agent_id.to_string(),
                queue_id: queue_id.to_string(),
            },
        );
    }

    pub fn flush_queued_prompt_now(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        queue_id: &str,
    ) {
        let req_id = Uuid::new_v4();
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            ag.queued_prompts.retain(|q| q.queue_id != queue_id);
            if let Some(adds) = h.pending_queue_adds.get_mut(agent_id) {
                adds.retain(|(_, qv)| qv.queue_id != queue_id);
            }
            h.pending_queue_removals
                .entry(agent_id.to_string())
                .or_default()
                .push((req_id, queue_id.to_string()));
            h.mark_stage_meta_dirty(agent_id);
        }

        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });

        self.dispatch_wire_queued(
            host_id,
            WireMessage::FlushQueuedPromptNow {
                request_id: req_id,
                agent_id: agent_id.to_string(),
                queue_id: queue_id.to_string(),
            },
        );
    }

    pub fn reorder_queued_prompts(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        queue_ids: Vec<String>,
    ) {
        let req_id = Uuid::new_v4();
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
            && ag.queued_prompts.len() >= 2
        {
            let mut by_id: std::collections::HashMap<String, orbit_protocol::QueuedPromptView> = ag
                .queued_prompts
                .drain(..)
                .map(|q| (q.queue_id.clone(), q))
                .collect();
            let mut reordered = Vec::with_capacity(by_id.len());
            for qid in &queue_ids {
                if let Some(item) = by_id.remove(qid) {
                    reordered.push(item);
                }
            }
            // Append any concurrently received items not present in `queue_ids`
            let mut remaining: Vec<_> = by_id.into_values().collect();
            remaining.sort_by_key(|q| q.created_at);
            reordered.extend(remaining);
            ag.queued_prompts = reordered;
            h.pending_queue_reorders
                .insert(agent_id.to_string(), (req_id, queue_ids.clone()));
            h.mark_stage_meta_dirty(agent_id);
        }

        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });

        self.dispatch_wire_queued(
            host_id,
            WireMessage::ReorderQueuedPrompts {
                request_id: req_id,
                agent_id: agent_id.to_string(),
                queue_ids,
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
            h.mark_stage_meta_dirty(agent_id);
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
            h.mark_stage_meta_dirty(agent_id);
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
            h.mark_stage_meta_dirty(agent_id);
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
        self.switch_agent_provider_with_config(host_id, agent_id, provider, None, None, None);
    }

    pub fn switch_agent_provider_with_config(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        provider: ProviderKind,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
        acp_agent_id: Option<String>,
    ) {
        let req_id = Uuid::new_v4();
        let clean_model = model.filter(|m| !m.trim().is_empty());
        let clean_acp_id = if provider == ProviderKind::Acp {
            acp_agent_id
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        } else {
            None
        };
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ag) = h.agents.get_mut(agent_id) {
                ag.provider = provider;
                ag.model = clean_model.clone();
                ag.effort = effort;
                ag.acp_agent_id = clean_acp_id.clone();
            }
            h.mark_stage_meta_dirty(agent_id);
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
                    acp_agent_id: clean_acp_id,
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
            if agent_id.starts_with("opt_") {
                h.agents.remove(agent_id);
                h.timelines.remove(agent_id);
                h.stage_cache.remove(agent_id);
                h.stage_dirty.remove(agent_id);
            } else if let Some(ag) = h.agents.get_mut(agent_id) {
                ag.archived_at = Some(Utc::now());
                ag.attention = AttentionState::None;
                ag.status = AgentStatus::Closed;
            }
        }
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

    pub fn unarchive_agent(self: &Arc<Self>, host_id: &str, agent_id: &str) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            ag.archived_at = None;
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::UnarchiveAgent {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
            },
        );
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn delete_agent(self: &Arc<Self>, host_id: &str, agent_id: &str) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.agents.remove(agent_id);
            h.timelines.remove(agent_id);
            h.stage_cache.remove(agent_id);
            h.stage_dirty.remove(agent_id);
        }
        if let Ok(mut prefs) = self.thread_prefs.write() {
            prefs.remove(&(host_id.to_string(), agent_id.to_string()));
        }
        let _ = self.store.delete_client_thread_pref(host_id, agent_id);
        let _ = self.store.delete_replica_agent(host_id, agent_id);
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::DeleteAgent {
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

    pub fn detach_agent(self: &Arc<Self>, host_id: &str, agent_id: &str) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            ag.parent_agent_id = None;
            ag.parent_host_id = None;
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::DetachAgent {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
            },
        );
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn mark_agent_read(self: &Arc<Self>, host_id: &str, agent_id: &str) {
        self.mark_agent_seen(host_id, agent_id, 0);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_agent(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        title: Option<String>,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
        mode: Option<SessionMode>,
        labels: Option<HashMap<String, String>>,
    ) {
        self.update_agent_with_handle(host_id, agent_id, title, model, effort, mode, labels, None);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_agent_with_handle(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        title: Option<String>,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
        mode: Option<SessionMode>,
        labels: Option<HashMap<String, String>>,
        handle: Option<String>,
    ) {
        self.update_agent_full(
            host_id, agent_id, title, model, effort, None, mode, labels, handle,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_agent_full(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        title: Option<String>,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
        fast_mode: Option<bool>,
        mode: Option<SessionMode>,
        labels: Option<HashMap<String, String>>,
        handle: Option<String>,
    ) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            if let Some(ag) = h.agents.get_mut(agent_id) {
                if let Some(ref t) = title {
                    ag.title = t.clone();
                }
                if let Some(ref m) = model {
                    ag.model = Some(m.clone());
                }
                if let Some(e) = effort {
                    ag.effort = Some(e);
                }
                if let Some(fm) = fast_mode {
                    ag.fast_mode = fm;
                }
                if let Some(ref md) = mode {
                    ag.mode = md.clone();
                }
                if let Some(ref h_val) = handle {
                    let clean = h_val.trim().trim_start_matches('@');
                    if !clean.is_empty() {
                        ag.handle = Some(clean.to_string());
                    }
                }
            }
            h.mark_stage_meta_dirty(agent_id);
        }
        if !agent_id.starts_with("opt_") {
            self.dispatch_wire_queued(
                host_id,
                WireMessage::UpdateAgent {
                    request_id: Uuid::new_v4(),
                    agent_id: agent_id.to_string(),
                    title,
                    model,
                    effort,
                    fast_mode,
                    mode,
                    labels,
                    handle,
                    task_summary: None,
                },
            );
        }
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn set_agent_fast_mode(self: &Arc<Self>, host_id: &str, agent_id: &str, fast_mode: bool) {
        self.update_agent_full(
            host_id,
            agent_id,
            None,
            None,
            None,
            Some(fast_mode),
            None,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_agent_identity(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        handle: Option<String>,
        title: Option<String>,
        task_summary: Option<String>,
        lock_handle: bool,
        lock_title: bool,
    ) {
        let mut labels: HashMap<String, String> = HashMap::new();
        if lock_handle {
            labels.insert("orbit.handle_locked".into(), "1".into());
        }
        if lock_title {
            labels.insert("orbit.title_locked".into(), "1".into());
        }
        let clean_handle = handle.map(|h| h.trim().trim_start_matches('@').to_string());
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            if let Some(ref t) = title
                && !t.trim().is_empty()
            {
                ag.title = t.trim().to_string();
            }
            if let Some(ref hv) = clean_handle
                && !hv.is_empty()
            {
                ag.handle = Some(hv.clone());
            }
            if let Some(ref ts) = task_summary {
                let tr = ts.trim();
                ag.task_summary = (!tr.is_empty()).then(|| tr.to_string());
            }
            for (k, v) in &labels {
                ag.labels.insert(k.clone(), v.clone());
            }
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::UpdateAgent {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
                title,
                model: None,
                effort: None,
                fast_mode: None,
                mode: None,
                labels: (!labels.is_empty()).then_some(labels),
                handle: clean_handle,
                task_summary,
            },
        );
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    // --- Git Repository Discovery & Mission Workspace Orchestration ---

    pub fn request_git_repos_inspection(
        self: &Arc<Self>,
        host_id: &str,
        path: &str,
        extra_dirs: Vec<String>,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::InspectGitRepos {
                request_id: Uuid::new_v4(),
                cwd: path.to_string(),
                extra_dirs,
            },
        );
    }

    pub fn git_repos_inspection(
        &self,
        host_id: &str,
        path: &str,
    ) -> Option<GitReposInspectionView> {
        let norm = crate::sidebar::normalize_project_path(path);
        self.hosts.read().ok().and_then(|g| {
            let h = g.get(host_id)?;
            h.git_repos_by_cwd
                .get(&norm)
                .or_else(|| {
                    h.git_repos_by_cwd
                        .iter()
                        .find(|(k, _)| cwd_paths_equivalent(k, &norm))
                        .map(|(_, v)| v)
                })
                .cloned()
        })
    }

    pub async fn inspect_git_repos_on_host(
        self: &Arc<Self>,
        host_id: &str,
        path: &str,
        extra_dirs: Vec<String>,
    ) -> Result<GitReposInspectionView, String> {
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
            if let Some(cached) = self.git_repos_inspection(host_id, path) {
                return Ok(cached);
            }
            return Err("Host is disconnected".into());
        };

        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let owned_path = path.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_git_repo_inspections.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::InspectGitRepos {
                    request_id: req_id,
                    cwd: owned_path,
                    extra_dirs,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_git_repo_inspections.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send InspectGitRepos: {e}")));
                return;
            }
            let outcome = match tokio::time::timeout(Duration::from_secs(6), ack_rx).await {
                Ok(Ok(Ok(view))) => Ok(view),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while inspecting git repos".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_git_repo_inspections.remove(&req_id);
                    }
                    Err("Timed out waiting for daemon to inspect git repositories".to_string())
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "git inspection task cancelled".to_string())?
    }

    pub fn inspect_git_repos_blocking(
        self: &Arc<Self>,
        host_id: &str,
        path: &str,
        extra_dirs: Vec<String>,
    ) -> Result<GitReposInspectionView, String> {
        self.runtime
            .block_on(self.inspect_git_repos_on_host(host_id, path, extra_dirs))
    }

    pub fn fetch_workspace_board(self: &Arc<Self>, host_id: &str, workspace_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::GetWorkspaceBoard {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
            },
        );
    }

    pub fn workspace_board(
        &self,
        host_id: &str,
        workspace_id: &str,
    ) -> Option<WorkspaceBoardSnapshot> {
        self.hosts
            .read()
            .ok()
            .and_then(|g| g.get(host_id)?.workspace_boards.get(workspace_id).cloned())
    }

    pub fn update_workspace_board(
        self: &Arc<Self>,
        host_id: &str,
        workspace_id: &str,
        content: String,
        section: Option<String>,
        updated_by: Option<String>,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::UpdateWorkspaceBoard {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
                content,
                section,
                updated_by,
            },
        );
    }

    pub fn fetch_workspace_events(
        self: &Arc<Self>,
        host_id: &str,
        workspace_id: &str,
        limit: Option<u32>,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ListWorkspaceEvents {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
                limit,
            },
        );
    }

    pub fn workspace_events(&self, host_id: &str, workspace_id: &str) -> Vec<WorkspaceEventRecord> {
        self.hosts
            .read()
            .ok()
            .and_then(|g| g.get(host_id)?.workspace_events.get(workspace_id).cloned())
            .unwrap_or_default()
    }

    pub fn update_workspace_goal(
        self: &Arc<Self>,
        host_id: &str,
        workspace_id: &str,
        goal: String,
    ) {
        let trimmed = goal.trim();
        let next_goal = (!trimmed.is_empty()).then(|| trimmed.to_string());
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ws) = h
                .workspaces
                .iter_mut()
                .find(|w| w.workspace_id == workspace_id)
        {
            ws.mission_goal = next_goal.clone();
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::UpdateWorkspaceGoal {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
                title: None,
                mission_goal: Some(goal),
            },
        );
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn move_agent_to_workspace(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        target_workspace_id: Option<String>,
    ) {
        self.move_agent_to_workspace_v2(host_id, agent_id, target_workspace_id, None, false);
    }

    pub fn move_agent_to_workspace_v2(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        target_workspace_id: Option<String>,
        new_workspace_title: Option<String>,
        make_unclassified: bool,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::MoveAgentToWorkspace {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
                target_workspace_id,
                new_workspace_title,
                make_unclassified,
            },
        );
    }

    pub fn organize_project_workspaces(
        self: &Arc<Self>,
        host_id: &str,
        project_root: &str,
        agent_ids: Vec<String>,
        auto_apply: bool,
    ) {
        let req_id = Uuid::new_v4();
        let running = WorkspaceOrganizeEvent {
            request_id: Some(req_id),
            project_root: project_root.to_string(),
            phase: orbit_protocol::WorkspaceOrganizePhase::Running,
            message: "正在触发 AI 上下文理解与 Workspace 归类…".into(),
            created_workspaces: 0,
            grouped_agents: 0,
            summarized_agents: 0,
        };
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            let norm = crate::sidebar::normalize_project_path(project_root);
            h.workspace_organize_status.insert(norm, running.clone());
            h.workspace_organize_status
                .insert(project_root.to_string(), running);
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::OrganizeProjectWorkspaces {
                request_id: req_id,
                project_root: project_root.to_string(),
                agent_ids,
                auto_apply,
            },
        );
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn summarize_agent(self: &Arc<Self>, host_id: &str, agent_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::SummarizeAgent {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
            },
        );
    }

    pub fn merge_workspaces(
        self: &Arc<Self>,
        host_id: &str,
        source_workspace_id: &str,
        target_workspace_id: &str,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::MergeWorkspaces {
                request_id: Uuid::new_v4(),
                source_workspace_id: source_workspace_id.to_string(),
                target_workspace_id: target_workspace_id.to_string(),
            },
        );
    }

    pub fn dismiss_cluster_suggestion(self: &Arc<Self>, host_id: &str, agent_id: &str) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ag) = h.agents.get_mut(agent_id)
        {
            ag.cluster_suggestion = None;
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::DismissClusterSuggestion {
                request_id: Uuid::new_v4(),
                agent_id: agent_id.to_string(),
            },
        );
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn spawn_workspace_peer(self: &Arc<Self>, req: SpawnWorkspacePeerRequest) -> String {
        self.create_agent_with_profile(
            CreateAgentRequest {
                host_id: &req.host_id,
                cwd: &req.cwd,
                extra_dirs: req.extra_dirs,
                provider: req.provider,
                acp_agent_id: req.acp_agent_id,
                model: req.model,
                effort: req.effort,
                fast_mode: req.fast_mode,
                mode: req.mode,
                initial_prompt: &req.initial_prompt,
                initial_display_prompt: None,
                initial_badges: Vec::new(),
                worktree_branch: req.worktree_branch.as_deref(),
                workspace_id: Some(&req.workspace_id),
                handle: req.handle,
                auto_cluster: Some(false),
            },
            req.profile_id,
        )
    }

    // --- Workspace Orchestration & Scripts ---

    #[allow(clippy::too_many_arguments)]
    pub fn create_workspace_v2(
        self: &Arc<Self>,
        host_id: &str,
        source_cwd: &str,
        title: Option<String>,
        isolation: WorkspaceIsolation,
        worktree_mode: WorktreeCreateMode,
        worktree_branch: Option<String>,
        base_ref: Option<String>,
        pr_number: Option<u32>,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::CreateWorkspace {
                request_id: Uuid::new_v4(),
                source_cwd: source_cwd.to_string(),
                title,
                worktree_branch,
                base_ref,
                isolation,
                worktree_mode,
                pr_number,
                parent_workspace_id: None,
                background: false,
                idempotency_key: None,
                is_curated: Some(true),
                mission_goal: None,
                project_root: None,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_mission_workspace(
        self: &Arc<Self>,
        host_id: &str,
        source_cwd: &str,
        title: String,
        mission_goal: Option<String>,
        isolation: WorkspaceIsolation,
        worktree_branch: Option<String>,
        base_ref: Option<String>,
    ) {
        let clean_title = title.trim();
        let final_title = (!clean_title.is_empty()).then(|| clean_title.to_string());
        let clean_goal = mission_goal
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        self.dispatch_wire_queued(
            host_id,
            WireMessage::CreateWorkspace {
                request_id: Uuid::new_v4(),
                source_cwd: source_cwd.to_string(),
                title: final_title,
                worktree_branch,
                base_ref,
                isolation,
                worktree_mode: WorktreeCreateMode::BranchOff,
                pr_number: None,
                parent_workspace_id: None,
                background: false,
                idempotency_key: None,
                is_curated: Some(true),
                mission_goal: clean_goal,
                project_root: Some(source_cwd.to_string()),
            },
        );
    }

    pub fn rename_workspace(self: &Arc<Self>, host_id: &str, workspace_id: &str, title: &str) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
            && let Some(ws) = h
                .workspaces
                .iter_mut()
                .find(|w| w.workspace_id == workspace_id)
        {
            ws.title = title.to_string();
            ws.title_locked = true;
            ws.is_curated = true;
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::RenameWorkspace {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
                title: title.to_string(),
            },
        );
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn archive_workspace(self: &Arc<Self>, host_id: &str, workspace_id: &str) {
        self.archive_workspace_v2(host_id, workspace_id, false);
    }

    pub fn archive_workspace_v2(
        self: &Arc<Self>,
        host_id: &str,
        workspace_id: &str,
        keep_agents_unclassified: bool,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ArchiveWorkspace {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
                keep_agents_unclassified,
            },
        );
    }

    pub fn unarchive_workspace(self: &Arc<Self>, host_id: &str, workspace_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::UnarchiveWorkspace {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
            },
        );
    }

    pub fn list_workspace_scripts(self: &Arc<Self>, host_id: &str, workspace_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ListWorkspaceScripts {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
            },
        );
    }

    pub fn start_workspace_script(
        self: &Arc<Self>,
        host_id: &str,
        workspace_id: &str,
        script_name: &str,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::StartWorkspaceScript {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
                script_name: script_name.to_string(),
            },
        );
    }

    pub fn stop_workspace_script(
        self: &Arc<Self>,
        host_id: &str,
        workspace_id: &str,
        script_name: &str,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::StopWorkspaceScript {
                request_id: Uuid::new_v4(),
                workspace_id: workspace_id.to_string(),
                script_name: script_name.to_string(),
            },
        );
    }

    // --- Profiles, Schedules, Connected Hosts, Orchestration Config & Skills ---

    pub fn upsert_profile(self: &Arc<Self>, host_id: &str, profile: AgentProfile) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::UpsertProfile {
                request_id: Uuid::new_v4(),
                profile,
            },
        );
    }

    pub fn delete_profile(self: &Arc<Self>, host_id: &str, profile_id: &str) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.profiles.retain(|p| p.profile_id != profile_id);
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::DeleteProfile {
                request_id: Uuid::new_v4(),
                profile_id: profile_id.to_string(),
            },
        );
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    // --- First-Class Artifacts & SEP-1865 MCP Apps ---

    pub fn cached_artifacts(
        &self,
        host_id: &str,
        workspace_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Vec<ArtifactSummary> {
        let ws = workspace_id.map(str::trim).filter(|s| !s.is_empty());
        let ag = agent_id.map(str::trim).filter(|s| !s.is_empty());
        self.hosts
            .read()
            .ok()
            .and_then(|guard| {
                guard.get(host_id).map(|h| {
                    h.artifacts
                        .iter()
                        .filter(|a| ws.is_none_or(|w| a.workspace_id == w))
                        .filter(|a| ag.is_none_or(|id| a.agent_id.as_deref() == Some(id)))
                        .cloned()
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    pub async fn list_artifacts_async(
        self: &Arc<Self>,
        host_id: &str,
        workspace_id: Option<String>,
        agent_id: Option<String>,
    ) -> Result<Vec<ArtifactSummary>, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Ok(self.cached_artifacts(
                host_id,
                workspace_id.as_deref(),
                agent_id.as_deref(),
            ));
        };

        let unfiltered = workspace_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none()
            && agent_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_none();
        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_artifact_lists
                            .insert(req_id, (unfiltered, ack_tx));
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ListArtifacts {
                    request_id: req_id,
                    workspace_id,
                    agent_id,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_artifact_lists.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ListArtifacts: {e}")));
                return;
            }
            let outcome = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(Ok(list))) => Ok(list),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while listing artifacts".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_artifact_lists.remove(&req_id);
                    }
                    Err("Timed out waiting for artifacts snapshot".to_string())
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "list_artifacts task cancelled".to_string())?
    }

    pub fn list_artifacts_blocking(
        self: &Arc<Self>,
        host_id: &str,
        workspace_id: Option<String>,
        agent_id: Option<String>,
    ) -> Result<Vec<ArtifactSummary>, String> {
        let core = Arc::clone(self);
        let host = host_id.to_string();
        self.runtime.block_on(async move {
            core.list_artifacts_async(&host, workspace_id, agent_id)
                .await
        })
    }

    pub async fn get_artifact_async(
        self: &Arc<Self>,
        host_id: &str,
        artifact_id: &str,
        version: Option<u32>,
    ) -> Result<ArtifactDetail, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected".into());
        };

        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let owned_id = artifact_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_artifact_reads.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::GetArtifact {
                    request_id: req_id,
                    artifact_id: owned_id,
                    version,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_artifact_reads.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send GetArtifact: {e}")));
                return;
            }
            let outcome = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(Ok(detail))) => Ok(detail),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while loading artifact".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_artifact_reads.remove(&req_id);
                    }
                    Err("Timed out waiting for artifact content".to_string())
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "get_artifact task cancelled".to_string())?
    }

    pub fn get_artifact_blocking(
        self: &Arc<Self>,
        host_id: &str,
        artifact_id: &str,
        version: Option<u32>,
    ) -> Result<ArtifactDetail, String> {
        let core = Arc::clone(self);
        let host = host_id.to_string();
        let aid = artifact_id.to_string();
        self.runtime
            .block_on(async move { core.get_artifact_async(&host, &aid, version).await })
    }

    pub async fn upsert_artifact_async(
        self: &Arc<Self>,
        req: UpsertArtifactRequest,
    ) -> Result<ArtifactSummary, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(&req.host_id)
                .ok_or_else(|| format!("host `{}` not found", req.host_id))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected".into());
        };

        let core = Arc::clone(self);
        let owned_host = req.host_id.clone();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_artifact_upserts.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::UpsertArtifact {
                    request_id: req_id,
                    artifact_id: req.artifact_id,
                    workspace_id: req.workspace_id,
                    agent_id: req.agent_id,
                    title: req.title,
                    kind: req.kind,
                    language: req.language,
                    mime_type: req.mime_type,
                    resource_uri: req.resource_uri,
                    content: req.content,
                    summary_note: req.summary_note,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_artifact_upserts.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send UpsertArtifact: {e}")));
                return;
            }
            let outcome = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(Ok(summary))) => Ok(summary),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while saving artifact".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_artifact_upserts.remove(&req_id);
                    }
                    Err("Timed out waiting for artifact update".to_string())
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "upsert_artifact task cancelled".to_string())?
    }

    pub fn upsert_artifact_blocking(
        self: &Arc<Self>,
        req: UpsertArtifactRequest,
    ) -> Result<ArtifactSummary, String> {
        let core = Arc::clone(self);
        self.runtime
            .block_on(async move { core.upsert_artifact_async(req).await })
    }

    pub async fn delete_artifact_async(
        self: &Arc<Self>,
        host_id: &str,
        artifact_id: &str,
    ) -> Result<String, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected".into());
        };

        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let owned_id = artifact_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_artifact_deletes.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::DeleteArtifact {
                    request_id: req_id,
                    artifact_id: owned_id,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_artifact_deletes.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send DeleteArtifact: {e}")));
                return;
            }
            let outcome = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(Ok(deleted_id))) => Ok(deleted_id),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while deleting artifact".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_artifact_deletes.remove(&req_id);
                    }
                    Err("Timed out waiting for artifact deletion".to_string())
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "delete_artifact task cancelled".to_string())?
    }

    pub fn delete_artifact_blocking(
        self: &Arc<Self>,
        host_id: &str,
        artifact_id: &str,
    ) -> Result<String, String> {
        let core = Arc::clone(self);
        let host = host_id.to_string();
        let aid = artifact_id.to_string();
        self.runtime
            .block_on(async move { core.delete_artifact_async(&host, &aid).await })
    }

    // --- Universal ACP Agent Registry & Live Probing ---

    pub fn list_acp_agents(&self, host_id: &str) -> Vec<AcpAgentAvailability> {
        self.hosts
            .read()
            .ok()
            .and_then(|guard| guard.get(host_id).map(|h| h.acp_agents.clone()))
            .unwrap_or_default()
    }

    pub fn refresh_acp_agents(self: &Arc<Self>, host_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ListAcpAgents {
                request_id: Uuid::new_v4(),
            },
        );
    }

    async fn send_acp_snapshot_rpc(
        self: &Arc<Self>,
        host_id: &str,
        make_msg: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        timeout_secs: u64,
    ) -> Result<crate::contract::AcpRegistryRefreshView, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected".into());
        };

        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_acp_snapshots.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session.send_wire(make_msg(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_acp_snapshots.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ACP RPC: {e}")));
                return;
            }
            let outcome =
                match tokio::time::timeout(Duration::from_secs(timeout_secs), ack_rx).await {
                    Ok(Ok(Ok(view))) => Ok(view),
                    Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                    Ok(Err(_)) => Err("Host disconnected while updating ACP registry".to_string()),
                    Err(_) => {
                        if let Ok(mut guard) = core.hosts.write()
                            && let Some(h) = guard.get_mut(&owned_host)
                        {
                            h.pending_acp_snapshots.remove(&req_id);
                        }
                        Err("Timed out waiting for daemon ACP registry response".to_string())
                    }
                };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "ACP registry task cancelled".to_string())?
    }

    pub async fn upsert_custom_acp_agent_async(
        self: &Arc<Self>,
        host_id: &str,
        spec: AcpAgentSpec,
    ) -> Result<Vec<AcpAgentAvailability>, String> {
        self.send_acp_snapshot_rpc(
            host_id,
            move |request_id| WireMessage::UpsertCustomAcpAgent { request_id, spec },
            10,
        )
        .await
        .map(|v| v.agents)
    }

    pub fn upsert_custom_acp_agent_blocking(
        self: &Arc<Self>,
        host_id: &str,
        spec: AcpAgentSpec,
    ) -> Result<Vec<AcpAgentAvailability>, String> {
        self.runtime
            .block_on(self.upsert_custom_acp_agent_async(host_id, spec))
    }

    pub async fn delete_custom_acp_agent_async(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
    ) -> Result<Vec<AcpAgentAvailability>, String> {
        let owned_id = agent_id.to_string();
        self.send_acp_snapshot_rpc(
            host_id,
            move |request_id| WireMessage::DeleteCustomAcpAgent {
                request_id,
                agent_id: owned_id,
            },
            10,
        )
        .await
        .map(|v| v.agents)
    }

    pub fn delete_custom_acp_agent_blocking(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
    ) -> Result<Vec<AcpAgentAvailability>, String> {
        self.runtime
            .block_on(self.delete_custom_acp_agent_async(host_id, agent_id))
    }

    pub async fn refresh_acp_registry_async(
        self: &Arc<Self>,
        host_id: &str,
        registry_url: Option<String>,
    ) -> Result<crate::contract::AcpRegistryRefreshView, String> {
        self.send_acp_snapshot_rpc(
            host_id,
            move |request_id| WireMessage::RefreshAcpRegistry {
                request_id,
                registry_url,
            },
            20,
        )
        .await
    }

    pub fn refresh_acp_registry_blocking(
        self: &Arc<Self>,
        host_id: &str,
        registry_url: Option<String>,
    ) -> Result<crate::contract::AcpRegistryRefreshView, String> {
        self.runtime
            .block_on(self.refresh_acp_registry_async(host_id, registry_url))
    }

    pub async fn test_acp_agent_async(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        cwd: Option<String>,
    ) -> Result<AcpProbeResult, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected".into());
        };

        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let owned_id = agent_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_acp_probes.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::TestAcpAgent {
                    request_id: req_id,
                    agent_id: owned_id,
                    cwd,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_acp_probes.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send TestAcpAgent: {e}")));
                return;
            }
            let outcome = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(Ok(res))) => Ok(res),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while probing ACP agent".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_acp_probes.remove(&req_id);
                    }
                    Err("Timed out waiting for ACP handshake probe".to_string())
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "ACP probe task cancelled".to_string())?
    }

    pub fn test_acp_agent_blocking(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        cwd: Option<String>,
    ) -> Result<AcpProbeResult, String> {
        self.runtime
            .block_on(self.test_acp_agent_async(host_id, agent_id, cwd))
    }

    // --- External MCP Server Manager (stdio + Streamable HTTP/SSE + live probe) ---

    async fn send_external_mcp_snapshot_rpc(
        self: &Arc<Self>,
        host_id: &str,
        make_msg: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        timeout_secs: u64,
    ) -> Result<Vec<ExternalMcpServerRecord>, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected".into());
        };

        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_mcp_snapshots.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session.send_wire(make_msg(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_mcp_snapshots.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send MCP server RPC: {e}")));
                return;
            }
            let outcome =
                match tokio::time::timeout(Duration::from_secs(timeout_secs), ack_rx).await {
                    Ok(Ok(Ok(servers))) => Ok(servers),
                    Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                    Ok(Err(_)) => Err("Host disconnected while updating MCP servers".to_string()),
                    Err(_) => {
                        if let Ok(mut guard) = core.hosts.write()
                            && let Some(h) = guard.get_mut(&owned_host)
                        {
                            h.pending_mcp_snapshots.remove(&req_id);
                        }
                        Err("Timed out waiting for daemon MCP servers response".to_string())
                    }
                };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "External MCP server task cancelled".to_string())?
    }

    pub async fn list_external_mcp_servers_async(
        self: &Arc<Self>,
        host_id: &str,
        cwd: Option<String>,
    ) -> Result<Vec<ExternalMcpServerRecord>, String> {
        self.send_external_mcp_snapshot_rpc(
            host_id,
            move |request_id| WireMessage::ListExternalMcpServers { request_id, cwd },
            10,
        )
        .await
    }

    pub async fn upsert_external_mcp_server_async(
        self: &Arc<Self>,
        req: UpsertExternalMcpServerRequest,
    ) -> Result<Vec<ExternalMcpServerRecord>, String> {
        let host_id = req.host_id.clone();
        self.send_external_mcp_snapshot_rpc(
            &host_id,
            move |request_id| WireMessage::UpsertExternalMcpServer {
                request_id,
                scope: req.scope,
                cwd: req.cwd,
                name: req.name,
                transport: req.transport,
                env: req.env,
                headers: req.headers,
                keep_existing_secrets: req.keep_existing_secrets,
                enabled: req.enabled,
            },
            10,
        )
        .await
    }

    pub async fn toggle_external_mcp_server_async(
        self: &Arc<Self>,
        host_id: &str,
        server_id: &str,
        enabled: bool,
        cwd: Option<String>,
    ) -> Result<Vec<ExternalMcpServerRecord>, String> {
        let owned_id = server_id.to_string();
        self.send_external_mcp_snapshot_rpc(
            host_id,
            move |request_id| WireMessage::ToggleExternalMcpServer {
                request_id,
                server_id: owned_id,
                enabled,
                cwd,
            },
            10,
        )
        .await
    }

    pub async fn delete_external_mcp_server_async(
        self: &Arc<Self>,
        host_id: &str,
        server_id: &str,
        cwd: Option<String>,
    ) -> Result<Vec<ExternalMcpServerRecord>, String> {
        let owned_id = server_id.to_string();
        self.send_external_mcp_snapshot_rpc(
            host_id,
            move |request_id| WireMessage::DeleteExternalMcpServer {
                request_id,
                server_id: owned_id,
                cwd,
            },
            10,
        )
        .await
    }

    pub async fn probe_external_mcp_server_async(
        self: &Arc<Self>,
        host_id: &str,
        server_id: &str,
        cwd: Option<String>,
    ) -> Result<McpProbeResult, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected".into());
        };

        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let owned_id = server_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_mcp_probes.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ProbeExternalMcpServer {
                    request_id: req_id,
                    server_id: owned_id,
                    cwd,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_mcp_probes.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ProbeExternalMcpServer: {e}")));
                return;
            }
            let outcome = match tokio::time::timeout(Duration::from_secs(12), ack_rx).await {
                Ok(Ok(Ok(res))) => Ok(res),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while probing MCP server".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_mcp_probes.remove(&req_id);
                    }
                    Err("Timed out waiting for MCP server probe".to_string())
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "MCP probe task cancelled".to_string())?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_schedule(
        self: &Arc<Self>,
        host_id: &str,
        name: String,
        cron_expr: String,
        timezone: Option<String>,
        prompt: String,
        target: ScheduleTarget,
        max_runs: Option<u32>,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::CreateSchedule {
                request_id: Uuid::new_v4(),
                name,
                cron_expr,
                timezone,
                prompt,
                target,
                max_runs,
                expires_at: None,
                created_by_agent: None,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_schedule(
        self: &Arc<Self>,
        host_id: &str,
        schedule_id: &str,
        name: Option<String>,
        cron_expr: Option<String>,
        timezone: Option<String>,
        prompt: Option<String>,
        max_runs: Option<u32>,
        archive_on_finish: Option<bool>,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::UpdateSchedule {
                request_id: Uuid::new_v4(),
                schedule_id: schedule_id.to_string(),
                name,
                cron_expr,
                timezone,
                prompt,
                max_runs,
                expires_at: None,
                archive_on_finish,
            },
        );
    }

    pub fn pause_schedule(self: &Arc<Self>, host_id: &str, schedule_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::PauseSchedule {
                request_id: Uuid::new_v4(),
                schedule_id: schedule_id.to_string(),
            },
        );
    }

    pub fn resume_schedule(self: &Arc<Self>, host_id: &str, schedule_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ResumeSchedule {
                request_id: Uuid::new_v4(),
                schedule_id: schedule_id.to_string(),
            },
        );
    }

    pub fn run_schedule_once(self: &Arc<Self>, host_id: &str, schedule_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::RunScheduleOnce {
                request_id: Uuid::new_v4(),
                schedule_id: schedule_id.to_string(),
            },
        );
    }

    pub fn delete_schedule(self: &Arc<Self>, host_id: &str, schedule_id: &str) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.schedules.retain(|s| s.schedule_id != schedule_id);
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::DeleteSchedule {
                request_id: Uuid::new_v4(),
                schedule_id: schedule_id.to_string(),
            },
        );
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn fetch_schedule_runs(
        self: &Arc<Self>,
        host_id: &str,
        schedule_id: &str,
        limit: Option<u32>,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ListScheduleRuns {
                request_id: Uuid::new_v4(),
                schedule_id: schedule_id.to_string(),
                limit,
            },
        );
    }

    pub fn schedule_runs(&self, host_id: &str, schedule_id: &str) -> Vec<ScheduleRunRecord> {
        self.hosts
            .read()
            .ok()
            .and_then(|g| g.get(host_id)?.schedule_runs.get(schedule_id).cloned())
            .unwrap_or_default()
    }

    pub fn add_connected_host(
        self: &Arc<Self>,
        host_id: &str,
        label: Option<String>,
        pairing_uri: &str,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::AddConnectedHost {
                request_id: Uuid::new_v4(),
                label,
                pairing_uri: pairing_uri.to_string(),
            },
        );
    }

    pub fn update_connected_host(
        self: &Arc<Self>,
        host_id: &str,
        registration_id: &str,
        label: Option<String>,
        enabled: Option<bool>,
    ) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::UpdateConnectedHost {
                request_id: Uuid::new_v4(),
                registration_id: registration_id.to_string(),
                label,
                enabled,
            },
        );
    }

    pub fn remove_connected_host(self: &Arc<Self>, host_id: &str, registration_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::RemoveConnectedHost {
                request_id: Uuid::new_v4(),
                registration_id: registration_id.to_string(),
            },
        );
    }

    /// 1-click Mesh Sync: generates pairing URIs for all other saved hosts in the user's fleet
    /// and registers them as Daemon-to-Daemon Connected Hosts on `target_host_id`.
    pub fn sync_fleet_connected_hosts(self: &Arc<Self>, target_host_id: &str) -> u32 {
        let peers: Vec<(String, String)> = {
            let Ok(guard) = self.hosts.read() else {
                return 0;
            };
            guard
                .values()
                .filter(|h| h.profile.host_id != target_host_id)
                .filter_map(|h| {
                    let mut endpoints = h.profile.direct_endpoints.clone();
                    if let Some(ref custom) = h.profile.custom_direct_endpoint
                        && !endpoints.contains(custom)
                    {
                        endpoints.insert(0, custom.clone());
                    }
                    // Strip loopback-only endpoints when syncing across different machines if another route exists,
                    // preventing remote daemons from dialing a local process on `127.0.0.1`.
                    let non_loopback: Vec<String> = endpoints
                        .iter()
                        .filter(|ep| {
                            let lower = ep.to_ascii_lowercase();
                            !lower.contains("://127.0.0.1")
                                && !lower.contains("://localhost")
                                && !lower.contains("://[::1]")
                        })
                        .cloned()
                        .collect();
                    if !non_loopback.is_empty() || h.profile.relay_url.is_some() {
                        endpoints = non_loopback;
                    }
                    let pairing_secret = if h.is_local {
                        crate::desktop_ops::read_local_daemon_pairing_secret()
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or_else(|| h.profile.pairing_secret.clone())
                    } else {
                        h.profile.pairing_secret.clone()
                    };
                    let offer = PairingOffer {
                        host_id: h.profile.host_id.clone(),
                        hostname: h.profile.hostname.clone(),
                        daemon_pubkey_b64: h.profile.daemon_pubkey_b64.clone(),
                        direct_endpoints: endpoints,
                        relay_url: h.profile.relay_url.clone(),
                        pairing_secret,
                    };
                    let uri = offer.to_qr_uri().ok()?;
                    Some((h.profile.effective_display_name(), uri))
                })
                .collect()
        };
        let count = peers.len() as u32;
        for (label, uri) in peers {
            self.add_connected_host(target_host_id, Some(label), &uri);
        }
        count
    }

    pub fn update_orchestration_config(
        self: &Arc<Self>,
        host_id: &str,
        config: OrchestrationConfig,
    ) {
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.orchestration_config = config.clone();
            if let Some(ref mut ka) = h.keep_awake_status {
                ka.enabled = config.keep_awake_while_running;
                if !ka.enabled {
                    ka.active = false;
                }
            }
        }
        self.dispatch_wire_queued(
            host_id,
            WireMessage::UpdateOrchestrationConfig {
                request_id: Uuid::new_v4(),
                config,
            },
        );
        self.emit(|revision| CoreNotification::HostChanged {
            revision,
            host_id: host_id.to_string(),
        });
    }

    pub fn sync_orchestration_skills(self: &Arc<Self>, host_id: &str, skill_ids: Vec<String>) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::SyncOrchestrationSkills {
                request_id: Uuid::new_v4(),
                skill_ids,
            },
        );
    }

    fn normalize_managed_skills_cwd(cwd: Option<&str>) -> Option<String> {
        cwd.map(str::trim)
            .filter(|s| !s.is_empty() && *s != "~")
            .map(ToString::to_string)
    }

    async fn request_managed_skills_snapshot(
        self: &Arc<Self>,
        host_id: &str,
        cwd: Option<String>,
        build_wire: impl FnOnce(Uuid, Option<String>) -> WireMessage + Send + 'static,
    ) -> Result<Vec<ManagedSkillRecord>, String> {
        let normalized_cwd = Self::normalize_managed_skills_cwd(cwd.as_deref());
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
            return Err("Host is disconnected; reconnect to manage skills".to_string());
        };

        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.active_managed_skills_cwd = normalized_cwd.clone();
                        h.pending_managed_skills_lists
                            .insert(req_id, (normalized_cwd.clone(), ack_tx));
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session.send_wire(build_wire(req_id, normalized_cwd)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_managed_skills_lists.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send skills RPC: {e}")));
                return;
            }
            let outcome = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(Ok(skills))) => Ok(skills),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while updating skills".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_managed_skills_lists.remove(&req_id);
                    }
                    Err("Timed out waiting for managed skills snapshot".to_string())
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "managed skills request cancelled".to_string())?
    }

    pub async fn list_managed_skills_async(
        self: &Arc<Self>,
        host_id: &str,
        cwd: Option<String>,
    ) -> Result<Vec<ManagedSkillRecord>, String> {
        self.request_managed_skills_snapshot(host_id, cwd, move |request_id, cwd| {
            WireMessage::ListManagedSkills { request_id, cwd }
        })
        .await
    }

    pub async fn read_managed_skill_async(
        self: &Arc<Self>,
        host_id: &str,
        skill_id: &str,
        cwd: Option<String>,
    ) -> Result<ManagedSkillContentView, String> {
        let normalized_cwd = Self::normalize_managed_skills_cwd(cwd.as_deref());
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
            return Err("Host is disconnected; reconnect to read skill".to_string());
        };

        let core = Arc::clone(self);
        let owned_host = host_id.to_string();
        let owned_skill_id = skill_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let reg_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host) {
                    Some(h) if h.session.is_some() => {
                        h.pending_managed_skill_reads.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected".to_string()),
                    None => Some(format!("host `{owned_host}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = reg_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ReadManagedSkill {
                    request_id: req_id,
                    skill_id: owned_skill_id,
                    cwd: normalized_cwd,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host)
                {
                    h.pending_managed_skill_reads.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ReadManagedSkill: {e}")));
                return;
            }
            let outcome = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(Ok(content))) => Ok(content),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while reading skill".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host)
                    {
                        h.pending_managed_skill_reads.remove(&req_id);
                    }
                    Err("Timed out waiting for managed skill content".to_string())
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "read_managed_skill task cancelled".to_string())?
    }

    pub async fn upsert_managed_skill_async(
        self: &Arc<Self>,
        req: UpsertManagedSkillRequest,
    ) -> Result<Vec<ManagedSkillRecord>, String> {
        let host_id = req.host_id.clone();
        let cwd = req.cwd.clone();
        self.request_managed_skills_snapshot(&host_id, cwd, move |request_id, cwd| {
            WireMessage::UpsertManagedSkill {
                request_id,
                scope: req.scope,
                cwd,
                slug: req.slug,
                name: req.name,
                description: req.description,
                content: req.content,
                enabled: req.enabled,
                expected_etag: req.expected_etag,
            }
        })
        .await
    }

    pub async fn toggle_managed_skill_async(
        self: &Arc<Self>,
        host_id: &str,
        skill_id: &str,
        enabled: bool,
        cwd: Option<String>,
    ) -> Result<Vec<ManagedSkillRecord>, String> {
        let owned_skill_id = skill_id.to_string();
        self.request_managed_skills_snapshot(host_id, cwd, move |request_id, cwd| {
            WireMessage::ToggleManagedSkill {
                request_id,
                skill_id: owned_skill_id,
                enabled,
                cwd,
            }
        })
        .await
    }

    pub async fn delete_managed_skill_async(
        self: &Arc<Self>,
        host_id: &str,
        skill_id: &str,
        cwd: Option<String>,
    ) -> Result<Vec<ManagedSkillRecord>, String> {
        let owned_skill_id = skill_id.to_string();
        self.request_managed_skills_snapshot(host_id, cwd, move |request_id, cwd| {
            WireMessage::DeleteManagedSkill {
                request_id,
                skill_id: owned_skill_id,
                cwd,
            }
        })
        .await
    }

    pub async fn import_managed_skill_async(
        self: &Arc<Self>,
        req: ImportManagedSkillRequest,
    ) -> Result<Vec<ManagedSkillRecord>, String> {
        let host_id = req.host_id.clone();
        let cwd = req.cwd.clone();
        self.request_managed_skills_snapshot(&host_id, cwd, move |request_id, cwd| {
            WireMessage::ImportManagedSkill {
                request_id,
                scope: req.scope,
                cwd,
                source_markdown_or_path: req.source_markdown_or_path,
                slug_override: req.slug_override,
            }
        })
        .await
    }

    pub fn list_provider_subagents(self: &Arc<Self>, host_id: &str, parent_agent_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ListProviderSubagents {
                request_id: Uuid::new_v4(),
                parent_agent_id: parent_agent_id.to_string(),
            },
        );
    }

    pub fn open_provider_subagent_timeline(
        self: &Arc<Self>,
        host_id: &str,
        parent_agent_id: &str,
        subagent_id: &str,
    ) {
        let key = (parent_agent_id.to_string(), subagent_id.to_string());
        let since_seq = self
            .hosts
            .write()
            .ok()
            .and_then(|mut g| {
                let host_state = g.get_mut(host_id)?;
                host_state.active_provider_subagent = Some(key.clone());
                host_state.subscribed_provider_subagents.insert(key.clone());
                if host_state.provider_subagent_timelines.contains_key(&key) {
                    host_state.touch_provider_subagent_lru(&key);
                }
                host_state
                    .provider_subagent_timelines
                    .get(&key)
                    .and_then(|list| list.iter().map(|e| e.seq).max())
            })
            .unwrap_or(0);
        self.dispatch_wire_queued(
            host_id,
            WireMessage::SubscribeProviderSubagent {
                request_id: Uuid::new_v4(),
                parent_agent_id: parent_agent_id.to_string(),
                subagent_id: subagent_id.to_string(),
                since_seq,
            },
        );
    }

    pub fn provider_subagent_timeline(
        &self,
        host_id: &str,
        parent_agent_id: &str,
        subagent_id: &str,
    ) -> Vec<ProviderSubagentEvent> {
        self.hosts
            .read()
            .ok()
            .and_then(|g| {
                g.get(host_id)?
                    .provider_subagent_timelines
                    .get(&(parent_agent_id.to_string(), subagent_id.to_string()))
                    .cloned()
            })
            .unwrap_or_default()
    }

    /// Projects a provider-native subagent's `TimelinePayload` stream through the exact same
    /// markdown / thinking / tool-coalescing stage pipeline as primary conversations.
    pub fn provider_subagent_turns(
        &self,
        host_id: &str,
        parent_agent_id: &str,
        subagent_id: &str,
        fallback_prompt: Option<&str>,
    ) -> Vec<StageTurnView> {
        let Ok(mut hosts) = self.hosts.write() else {
            return Vec::new();
        };
        let Some(host_state) = hosts.get_mut(host_id) else {
            return Vec::new();
        };

        let cwd = host_state
            .agents
            .get(parent_agent_id)
            .map(|a| a.cwd.clone());
        let key = (parent_agent_id.to_string(), subagent_id.to_string());
        let raw_events = host_state
            .provider_subagent_timelines
            .get(&key)
            .cloned()
            .unwrap_or_default();
        if !raw_events.is_empty() {
            host_state.touch_provider_subagent_lru(&key);
            host_state.enforce_provider_subagent_timeline_bounds(Some(&key));
        }
        let descriptor = host_state
            .provider_subagents
            .get(parent_agent_id)
            .and_then(|list| list.iter().find(|ps| ps.subagent_id == subagent_id))
            .cloned();

        let starts_with_prompt = raw_events
            .first()
            .is_some_and(|ev| matches!(ev.payload, TimelinePayload::UserPrompt { .. }));

        let resolved_prompt = fallback_prompt
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                descriptor.as_ref().and_then(|d| {
                    let sub = d.subtitle.as_deref()?.trim();
                    if sub.is_empty()
                        || sub.eq_ignore_ascii_case("antigravity")
                        || sub.eq_ignore_ascii_case("codex")
                        || sub.eq_ignore_ascii_case("claude")
                        || sub.eq_ignore_ascii_case("claude code")
                    {
                        None
                    } else {
                        Some(sub.to_string())
                    }
                })
            });

        let mut timeline_events: Vec<TimelineEvent> =
            Vec::with_capacity(raw_events.len() + usize::from(!starts_with_prompt));

        if !starts_with_prompt && let Some(prompt_text) = resolved_prompt {
            let prompt_ts = raw_events
                .first()
                .map(|ev| ev.timestamp)
                .or_else(|| descriptor.as_ref().map(|d| d.created_at))
                .unwrap_or_else(Utc::now);
            timeline_events.push(TimelineEvent {
                seq: 0,
                epoch: 1,
                agent_id: subagent_id.to_string(),
                turn_id: None,
                timestamp: prompt_ts,
                payload: TimelinePayload::UserPrompt {
                    message_id: format!("sub_prompt_{subagent_id}"),
                    text: prompt_text,
                    badges: Vec::new(),
                },
            });
        }

        for ev in raw_events {
            timeline_events.push(TimelineEvent {
                seq: ev.seq,
                epoch: 1,
                agent_id: subagent_id.to_string(),
                turn_id: None,
                timestamp: ev.timestamp,
                payload: ev.payload,
            });
        }

        if timeline_events.is_empty() {
            return Vec::new();
        }

        let (all_resolved_perms, all_resolved_questions) = collect_resolved_gates(&timeline_events);
        let cache_scope_id = format!("subagent:{parent_agent_id}:{subagent_id}");
        let mut items: Vec<TimelineItemView> = Vec::with_capacity(timeline_events.len());
        for ev in &timeline_events {
            if let Some(item) = render_timeline_event_view(
                &cache_scope_id,
                cwd.as_deref(),
                ev,
                &all_resolved_perms,
                &all_resolved_questions,
                &mut host_state.markdown_cache,
                &mut host_state.streaming_markdown_cache,
            ) {
                items.push(item);
            }
        }

        if let Some(d) = descriptor.as_ref() {
            let terminal_status = match d.status {
                orbit_protocol::ProviderSubagentStatus::Running => None,
                orbit_protocol::ProviderSubagentStatus::Completed => {
                    Some(ToolCallStatus::Completed)
                }
                orbit_protocol::ProviderSubagentStatus::Failed
                | orbit_protocol::ProviderSubagentStatus::Canceled => Some(ToolCallStatus::Failed),
            };
            if let Some(status) = terminal_status {
                stage::settle_timeline_items_for_finished_session(&mut items, status);
            }
        }

        stage::process_timeline_turns_slice(&items, 0, 0, cwd.as_deref()).turns
    }

    pub fn refresh_pending_permissions(self: &Arc<Self>, host_id: &str) {
        self.dispatch_wire_queued(
            host_id,
            WireMessage::ListPendingPermissions {
                request_id: Uuid::new_v4(),
                agent_id: None,
            },
        );
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
            let slot = h.active_terminal_slot.max(1);
            let emu = h.emulator_slot_mut(slot);
            emu.clear();
            emu.resize(cols, rows);
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
            let prev_id = h.slot_terminal_ids.get(&slot).map(String::as_str);
            if prev_id != Some(terminal_id) {
                h.emulator_slot_mut(slot).clear();
            }
            h.slot_terminal_ids.insert(slot, terminal_id.to_string());
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
        let slot = slot.max(1);
        let mut handle_opt = None;
        if let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(host_id)
        {
            h.emulator_slot_mut(slot).resize(cols, rows);
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
                            (aid.clone(), self.safe_catchup_since_seq(aid, max_seq))
                        })
                        .collect();
                    for (aid, _) in &resubs {
                        h.caught_up_agents.remove(aid);
                    }
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
                    core.handle_unexpected_disconnect(&host_id, None);
                    return;
                }
                let _ = handle
                    .send_wire(WireMessage::GetHostSyncState {
                        request_id: Uuid::new_v4(),
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListWorkspaces {
                        request_id: Uuid::new_v4(),
                        include_archived: true,
                        include_background: false,
                    })
                    .await;
                let _ = handle
                    .send_wire(WireMessage::ListAgents {
                        request_id: Uuid::new_v4(),
                        workspace_id: None,
                        include_archived: true,
                        include_background: false,
                        parent_agent_id: None,
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
        self.schedule_cloud_auto_sync(true);
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
                    core.handle_unexpected_disconnect(&host_id, None);
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

    /// Verify that `cwd` resolves to (or inside) a registered workspace, agent `cwd`,
    /// discovered project root, or sidebar project directory on the given local host.
    /// Rejects unregistered roots, `"~"`, `$HOME`, and filesystem root `"/"`.
    pub fn is_registered_local_cwd(&self, host_id: &str, cwd: &str) -> bool {
        let trimmed = cwd.trim();
        if trimmed.is_empty() || trimmed == "~" {
            return false;
        }
        let resolve_dir = |raw: &str| -> Option<std::path::PathBuf> {
            let t = raw.trim();
            if t.is_empty() || t == "~" {
                return None;
            }
            let expanded = if let Some(rest) = t.strip_prefix("~/") {
                crate::desktop_ops::resolve_user_home()?.join(rest)
            } else {
                std::path::PathBuf::from(t)
            };
            let canon = std::fs::canonicalize(&expanded).ok()?;
            if !canon.is_dir() || canon.parent().is_none() {
                return None;
            }
            Some(canon)
        };

        let Some(canon_cwd) = resolve_dir(trimmed) else {
            return false;
        };
        let canon_home =
            crate::desktop_ops::resolve_user_home().and_then(|h| std::fs::canonicalize(h).ok());
        if canon_home.as_ref().is_some_and(|home| canon_cwd == *home) {
            return false;
        }

        let matches_registered_root = |raw: &str| -> bool {
            let Some(canon_root) = resolve_dir(raw) else {
                return false;
            };
            if canon_home.as_ref().is_some_and(|home| canon_root == *home) {
                return false;
            }
            canon_cwd == canon_root || canon_cwd.starts_with(&canon_root)
        };

        if let Ok(hosts) = self.hosts.read() {
            let Some(h) = hosts.get(host_id) else {
                return false;
            };
            if !h.is_local {
                return false;
            }
            if h.workspaces.iter().any(|ws| {
                matches_registered_root(&ws.cwd)
                    || ws
                        .source_cwd
                        .as_deref()
                        .is_some_and(&matches_registered_root)
                    || ws
                        .project_root
                        .as_deref()
                        .is_some_and(&matches_registered_root)
            }) {
                return true;
            }
            if h.agents.values().any(|ag| matches_registered_root(&ag.cwd)) {
                return true;
            }
            if h.discovered_projects
                .iter()
                .any(|proj| matches_registered_root(&proj.path))
            {
                return true;
            }
        } else {
            return false;
        }

        if let Ok(projects) = self.projects.read() {
            return projects.values().any(|proj| {
                proj.host_id == host_id
                    && proj
                        .directories
                        .iter()
                        .any(|dir| matches_registered_root(dir))
            });
        }
        false
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

    /// Fork/branch an existing conversation on `req.host_id` (same directory or fresh git worktree).
    pub async fn fork_agent_on_host(
        self: &Arc<Self>,
        req: ForkAgentRequest,
    ) -> Result<ForkAgentResultView, String> {
        if req.source_agent_id.starts_with("opt_") {
            return Err("Wait for the session to finish initializing before forking".into());
        }
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
            return Err("Host is disconnected; reconnect to fork this session".into());
        };

        let core = Arc::clone(self);
        let owned_host_id = req.host_id.clone();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&owned_host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_forks.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some("Host disconnected before forking session".to_string()),
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
                .send_wire(WireMessage::ForkAgent {
                    request_id: req_id,
                    source_agent_id: req.source_agent_id,
                    up_to_seq: req.up_to_seq,
                    target: req.target,
                    title: req.title,
                    initial_prompt: req.initial_prompt,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host_id)
                {
                    h.pending_forks.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send fork request: {e}")));
                return;
            }

            let outcome = match tokio::time::timeout(Duration::from_secs(60), ack_rx).await {
                Ok(Ok(Ok(view))) => Ok(view),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while forking session".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host_id)
                    {
                        h.pending_forks.remove(&req_id);
                    }
                    Err(
                        "Timed out waiting for daemon to fork session (ensure orbit daemon is up to date)"
                            .to_string(),
                    )
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "Fork task terminated unexpectedly".to_string())?
    }

    /// Non-mutating dry-run preview of a bidirectional Worktree <-> Local handoff on `req.host_id`.
    pub async fn preview_worktree_handoff_on_host(
        self: &Arc<Self>,
        req: PreviewWorktreeHandoffRequest,
    ) -> Result<WorktreeHandoffPreview, String> {
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
            return Err("Host is disconnected; reconnect to preview worktree handoff".into());
        };

        let core = Arc::clone(self);
        let owned_host_id = req.host_id.clone();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_handoff_previews.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before previewing handoff".to_string()),
                    None => Some(format!("host `{owned_host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session
                .send_wire(WireMessage::PreviewWorktreeHandoff {
                    request_id: req_id,
                    workspace_id: req.workspace_id,
                    direction: req.direction,
                    target_workspace_id: req.target_workspace_id,
                    include_committed: req.include_committed,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host_id)
                {
                    h.pending_handoff_previews.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send handoff preview request: {e}")));
                return;
            }

            let outcome = match tokio::time::timeout(Duration::from_secs(30), ack_rx).await {
                Ok(Ok(Ok(preview))) => Ok(preview),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while previewing handoff".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host_id)
                    {
                        h.pending_handoff_previews.remove(&req_id);
                    }
                    Err(
                        "Timed out waiting for daemon handoff preview (ensure orbit daemon is up to date)"
                            .to_string(),
                    )
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "Handoff preview task terminated unexpectedly".to_string())?
    }

    /// Execute a backup-protected bidirectional Worktree <-> Local handoff on `req.host_id`.
    pub async fn execute_worktree_handoff_on_host(
        self: &Arc<Self>,
        req: ExecuteWorktreeHandoffRequest,
    ) -> Result<WorktreeHandoffOutcome, String> {
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
            return Err("Host is disconnected; reconnect to execute worktree handoff".into());
        };

        let core = Arc::clone(self);
        let owned_host_id = req.host_id.clone();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&owned_host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_handoff_executes.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before executing handoff".to_string()),
                    None => Some(format!("host `{owned_host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session
                .send_wire(WireMessage::ExecuteWorktreeHandoff {
                    request_id: req_id,
                    workspace_id: req.workspace_id,
                    direction: req.direction,
                    mode: req.mode,
                    target_workspace_id: req.target_workspace_id,
                    include_committed: req.include_committed,
                    branch: req.branch,
                    allow_three_way_conflicts: req.allow_three_way_conflicts,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host_id)
                {
                    h.pending_handoff_executes.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send handoff execute request: {e}")));
                return;
            }

            let outcome = match tokio::time::timeout(Duration::from_secs(60), ack_rx).await {
                Ok(Ok(Ok(result))) => Ok(result),
                Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                Ok(Err(_)) => Err("Host disconnected while executing handoff".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host_id)
                    {
                        h.pending_handoff_executes.remove(&req_id);
                    }
                    Err(
                        "Timed out waiting for daemon handoff execution (ensure orbit daemon is up to date)"
                            .to_string(),
                    )
                }
            };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "Handoff execute task terminated unexpectedly".to_string())?
    }

    /// Upgrade a remote Orbit daemon directly over the existing E2EE WebSocket/Relay channel
    /// (works even without SSH access!).
    ///
    /// - Protocol v4+: sends `WireMessage::UpgradeDaemon` so the remote daemon downloads, verifies,
    ///   atomically installs, and hot-restarts itself (`orbit shim` keeps all agents alive).
    /// - Pre-v4 fallback: creates a hidden one-shot remote PTY (`CreateTerminal` + `SendTerminalKeys`)
    ///   to run `install.sh && orbit daemon up; exit` automatically with zero user copy-pasting.
    pub async fn upgrade_host_daemon_over_wire(
        self: &Arc<Self>,
        host_id: &str,
        target_version: Option<String>,
    ) -> Result<String, String> {
        let (session, daemon_pv) = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            let session = h.session.clone().ok_or_else(|| {
                format!(
                    "Host `{}` is currently offline",
                    h.profile.effective_display_name()
                )
            })?;
            (session, h.daemon_protocol_version.unwrap_or(0))
        };

        let core = Arc::clone(self);
        let owned_host_id = host_id.to_string();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel::<Result<String, String>>();

        self.runtime.spawn(async move {
            if daemon_pv >= 4 {
                let req_id = Uuid::new_v4();
                let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
                {
                    let Ok(mut guard) = core.hosts.write() else {
                        let _ = res_tx.send(Err("host state lock poisoned".to_string()));
                        return;
                    };
                    let Some(h) = guard.get_mut(&owned_host_id) else {
                        let _ = res_tx.send(Err(format!("host `{owned_host_id}` removed")));
                        return;
                    };
                    h.pending_daemon_upgrades.insert(req_id, ack_tx);
                }

                if let Err(e) = session
                    .send_wire(WireMessage::UpgradeDaemon {
                        request_id: req_id,
                        target_version,
                    })
                    .await
                {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host_id)
                    {
                        h.pending_daemon_upgrades.remove(&req_id);
                    }
                    let _ = res_tx.send(Err(format!("Failed to send upgrade request: {e}")));
                    return;
                }

                let outcome = match tokio::time::timeout(Duration::from_secs(95), ack_rx).await {
                    Ok(Ok(Ok(()))) => {
                        tokio::time::sleep(Duration::from_millis(1800)).await;
                        core.connect_host_async(owned_host_id.clone());
                        Ok("Daemon upgraded and restarting".to_string())
                    }
                    Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                    // Connection drop right after daemon starts restarting is expected & healthy
                    Ok(Err(_)) => {
                        tokio::time::sleep(Duration::from_millis(1800)).await;
                        core.connect_host_async(owned_host_id.clone());
                        Ok("Daemon restarting with update".to_string())
                    }
                    Err(_) => {
                        if let Ok(mut guard) = core.hosts.write()
                            && let Some(h) = guard.get_mut(&owned_host_id)
                        {
                            h.pending_daemon_upgrades.remove(&req_id);
                        }
                        Err("Timed out waiting for remote daemon self-upgrade".to_string())
                    }
                };
                let _ = res_tx.send(outcome);
                return;
            }

            // Pre-v4 remote daemon fallback: execute installer + `orbit daemon up` over a one-shot remote PTY
            let req_id = Uuid::new_v4();
            let (term_tx, term_rx) = tokio::sync::oneshot::channel();
            {
                let Ok(mut guard) = core.hosts.write() else {
                    let _ = res_tx.send(Err("host state lock poisoned".to_string()));
                    return;
                };
                let Some(h) = guard.get_mut(&owned_host_id) else {
                    let _ = res_tx.send(Err(format!("host `{owned_host_id}` removed")));
                    return;
                };
                h.pending_upgrade_terminals.insert(req_id, term_tx);
            }

            if let Err(e) = session
                .send_wire(WireMessage::CreateTerminal {
                    request_id: req_id,
                    cwd: "~".to_string(),
                    title: Some("Orbit Auto-Update".to_string()),
                    cols: 120,
                    rows: 30,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host_id)
                {
                    h.pending_upgrade_terminals.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("Failed to open remote update shell: {e}")));
                return;
            }

            let terminal_id = match tokio::time::timeout(Duration::from_secs(12), term_rx).await {
                Ok(Ok(Ok(tid))) => tid,
                Ok(Ok(Err(rpc_err))) => {
                    let _ = res_tx.send(Err(rpc_err));
                    return;
                }
                Ok(Err(_)) => {
                    let _ = res_tx.send(Err("Host disconnected before update started".to_string()));
                    return;
                }
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&owned_host_id)
                    {
                        h.pending_upgrade_terminals.remove(&req_id);
                    }
                    let _ = res_tx.send(Err("Timed out starting remote update shell".to_string()));
                    return;
                }
            };

            let cmd = "export PATH=\"$HOME/.local/bin:/usr/local/bin:/opt/homebrew/bin:$PATH\" && curl -fsSL https://raw.githubusercontent.com/danielgaoyy/orbit/main/deploy/install.sh | sh && orbit daemon up; exit";
            if let Err(e) = session
                .send_wire(WireMessage::SendTerminalKeys {
                    request_id: Uuid::new_v4(),
                    terminal_id,
                    text: Some(cmd.to_string()),
                    keys: vec!["Enter".to_string()],
                })
                .await
            {
                let _ = res_tx.send(Err(format!("Failed to dispatch remote update command: {e}")));
                return;
            }

            // Give the remote host a few seconds to download, swap, and restart `orbit daemon up`
            tokio::time::sleep(Duration::from_secs(6)).await;
            core.connect_host_async(owned_host_id);
            let _ = res_tx.send(Ok("Remote daemon update triggered".to_string()));
        });

        res_rx
            .await
            .map_err(|_| "Remote upgrade task terminated unexpectedly".to_string())?
    }

    pub fn upgrade_host_daemon_over_wire_blocking(
        self: &Arc<Self>,
        host_id: &str,
        target_version: Option<String>,
    ) -> Result<String, String> {
        self.runtime
            .block_on(self.upgrade_host_daemon_over_wire(host_id, target_version))
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

        // Fast path for the local host: read directly from disk (with identical canonicalized
        // workspace & managed-root containment) to avoid WebSocket + Noise_NK + Zstd round-trips
        // and outside-cwd daemon rejection retries when clicking file/artifact links in chat.
        // Only advertise `writable: true` when a live local daemon session is connected to save edits.
        if is_local {
            let owned_host = host_id.to_string();
            let owned_cwd = cwd.to_string();
            let owned_path = path.to_string();
            let has_session = session.is_some();
            let local_res = tokio::task::spawn_blocking(move || {
                let raw = read_local_workspace_file_with_writable(
                    &owned_cwd,
                    &owned_path,
                    None,
                    has_session,
                )?;
                Ok::<WorkspaceFileView, String>(build_workspace_file_view(
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
                        etag: raw.etag,
                        mtime_unix_ms: raw.mtime_unix_ms,
                        writable: raw.writable,
                        line_ending: raw.line_ending,
                    },
                ))
            })
            .await
            .map_err(|_| "local file read task cancelled".to_string())?;

            match local_res {
                Ok(view) => return Ok(view),
                Err(err) if !has_session => return Err(err),
                Err(_) => {
                    // Fall through to connected daemon RPC (e.g. for custom daemon-side attachment roots)
                }
            }
        }

        let Some(session) = session else {
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
                    max_bytes: None,
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

            // Generous timeout on remote hosts for up to 25 MiB multimodal media previews
            let timeout_secs = if is_local { 8 } else { 45 };
            let raw_res = match tokio::time::timeout(Duration::from_secs(timeout_secs), ack_rx).await
            {
                Ok(Ok(Ok(raw))) => Ok(raw),
                Ok(Ok(Err(rpc_err))) => {
                    if is_local && rpc_err.contains("outside the workspace directory") {
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
                                etag: local.etag,
                                mtime_unix_ms: local.mtime_unix_ms,
                                writable: local.writable,
                                line_ending: local.line_ending,
                            }),
                            _ => Err(rpc_err),
                        }
                    } else {
                        Err(rpc_err)
                    }
                }
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
                                etag: local.etag,
                                mtime_unix_ms: local.mtime_unix_ms,
                                writable: local.writable,
                                line_ending: local.line_ending,
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

    /// Fuzzy-search workspace code symbols on any connected local or remote Orbit host.
    pub async fn search_workspace_symbols_on_host(
        self: &Arc<Self>,
        req: crate::contract::SearchWorkspaceSymbolsRequest,
    ) -> Result<crate::contract::WorkspaceSymbolsView, String> {
        let (session, is_local) = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&req.host_id)
                .ok_or_else(|| format!("host `{}` not found", req.host_id))?;
            (h.session.clone(), h.is_local)
        };

        let Some(session) = session else {
            return Ok(crate::contract::WorkspaceSymbolsView {
                host_id: req.host_id,
                cwd: req.cwd,
                symbols: Vec::new(),
                truncated: false,
            });
        };

        let core = Arc::clone(self);
        let owned_host_id = req.host_id.clone();
        let owned_cwd = req.cwd.clone();
        let owned_query = req.query.clone();
        let limit = if req.limit == 0 {
            30
        } else {
            req.limit.min(100)
        };
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&owned_host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_symbol_searches.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some("Host disconnected before searching symbols".to_string()),
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
                .send_wire(WireMessage::SearchWorkspaceSymbols {
                    request_id: req_id,
                    cwd: owned_cwd.clone(),
                    query: owned_query,
                    limit,
                })
                .await;

            if let Err(e) = send_res {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&owned_host_id)
                {
                    h.pending_symbol_searches.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to request symbol search: {e}")));
                return;
            }

            let timeout_secs = if is_local { 6 } else { 15 };
            let outcome =
                match tokio::time::timeout(Duration::from_secs(timeout_secs), ack_rx).await {
                    Ok(Ok(Ok((symbols, truncated)))) => Ok(crate::contract::WorkspaceSymbolsView {
                        host_id: owned_host_id,
                        cwd: owned_cwd,
                        symbols,
                        truncated,
                    }),
                    Ok(Ok(Err(rpc_err))) => Err(rpc_err),
                    Ok(Err(_)) => Err("Host disconnected while searching symbols".to_string()),
                    Err(_) => {
                        if let Ok(mut guard) = core.hosts.write()
                            && let Some(h) = guard.get_mut(&owned_host_id)
                        {
                            h.pending_symbol_searches.remove(&req_id);
                        }
                        Err("Timed out waiting for workspace symbol search".to_string())
                    }
                };
            let _ = res_tx.send(outcome);
        });

        res_rx
            .await
            .map_err(|_| "symbol search task cancelled".to_string())?
    }

    /// Atomically write UTF-8 content to a workspace file on any connected host with optimistic concurrency.
    pub async fn write_workspace_file_on_host(
        self: &Arc<Self>,
        req: WriteWorkspaceFileRequest,
    ) -> Result<WorkspaceFileWriteResultView, String> {
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
            return Err("Host is disconnected; reconnect to save files".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&req.host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_file_writes.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some("Host disconnected before saving file".to_string()),
                        None => Some(format!("host `{}` removed", req.host_id)),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            let send_res = session
                .send_wire(WireMessage::WriteWorkspaceFile {
                    request_id: req_id,
                    cwd: req.cwd,
                    path: req.path,
                    content: req.content,
                    expected_etag: req.expected_etag,
                    force: req.force,
                })
                .await;

            if let Err(e) = send_res {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&req.host_id)
                {
                    h.pending_file_writes.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send file write: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while saving file".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&req.host_id)
                    {
                        h.pending_file_writes.remove(&req_id);
                    }
                    Err(
                        "Timed out waiting for daemon to save file (ensure orbit daemon is up to date)"
                            .to_string(),
                    )
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| "file write task cancelled".to_string())?
    }

    async fn dispatch_path_rules_snapshot_on_host(
        self: &Arc<Self>,
        host_id: String,
        build_wire: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        op_label: &'static str,
    ) -> Result<PathRulesSnapshotView, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };

        let Some(session) = session else {
            return Err(format!("Host is disconnected; reconnect to {op_label}"));
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_path_rules.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some(format!("Host disconnected before {op_label}")),
                        None => Some(format!("host `{host_id}` removed")),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session.send_wire(build_wire(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_path_rules.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to {op_label}: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected while trying to {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_path_rules.remove(&req_id);
                    }
                    Err(format!(
                        "Timed out waiting for daemon to {op_label} (ensure orbit daemon is up to date)"
                    ))
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    async fn dispatch_lifecycle_hooks_snapshot_on_host<F>(
        self: &Arc<Self>,
        host_id: String,
        build_wire: F,
        op_label: &'static str,
    ) -> Result<LifecycleHooksSnapshotView, String>
    where
        F: FnOnce(Uuid) -> WireMessage + Send + 'static,
    {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err(format!("Host is disconnected; reconnect to {op_label}"));
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_lifecycle_hooks.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some(format!("Host disconnected before {op_label}")),
                        None => Some(format!("host `{host_id}` removed")),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session.send_wire(build_wire(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_lifecycle_hooks.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to {op_label}: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected while trying to {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_lifecycle_hooks.remove(&req_id);
                    }
                    Err(format!(
                        "Timed out waiting for daemon to {op_label} (ensure orbit daemon is up to date)"
                    ))
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    pub async fn list_lifecycle_hooks_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: Option<String>,
    ) -> Result<LifecycleHooksSnapshotView, String> {
        self.dispatch_lifecycle_hooks_snapshot_on_host(
            host_id.to_string(),
            move |request_id| WireMessage::ListLifecycleHooks { request_id, cwd },
            "list lifecycle hooks",
        )
        .await
    }

    pub async fn upsert_lifecycle_hook_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: Option<String>,
        hook: LifecycleHookRule,
    ) -> Result<LifecycleHooksSnapshotView, String> {
        self.dispatch_lifecycle_hooks_snapshot_on_host(
            host_id.to_string(),
            move |request_id| WireMessage::UpsertLifecycleHook {
                request_id,
                cwd,
                hook,
            },
            "save lifecycle hook",
        )
        .await
    }

    pub async fn delete_lifecycle_hook_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: Option<String>,
        hook_id: String,
        scope: HookScope,
    ) -> Result<LifecycleHooksSnapshotView, String> {
        self.dispatch_lifecycle_hooks_snapshot_on_host(
            host_id.to_string(),
            move |request_id| WireMessage::DeleteLifecycleHook {
                request_id,
                cwd,
                hook_id,
                scope,
            },
            "delete lifecycle hook",
        )
        .await
    }

    pub async fn set_project_hooks_trust_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: String,
        trusted: bool,
    ) -> Result<LifecycleHooksSnapshotView, String> {
        self.dispatch_lifecycle_hooks_snapshot_on_host(
            host_id.to_string(),
            move |request_id| WireMessage::SetProjectHooksTrust {
                request_id,
                cwd,
                trusted,
            },
            "update project hooks trust",
        )
        .await
    }

    pub async fn test_lifecycle_hook_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: Option<String>,
        hook_id: String,
        sample_payload: serde_json::Value,
    ) -> Result<HookExecutionRecord, String> {
        let host_id = host_id.to_string();
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to test lifecycle hook".to_string());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_lifecycle_hook_tests.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some("Host disconnected before test lifecycle hook".to_string()),
                        None => Some(format!("host `{host_id}` removed")),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session
                .send_wire(WireMessage::TestLifecycleHook {
                    request_id: req_id,
                    cwd,
                    hook_id,
                    sample_payload,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_lifecycle_hook_tests.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to test lifecycle hook: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(35), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => {
                    Err("Host disconnected while waiting for hook dry-run test".to_string())
                }
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_lifecycle_hook_tests.remove(&req_id);
                    }
                    Err("Timed out waiting for lifecycle hook test result".to_string())
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| "test lifecycle hook task cancelled".to_string())?
    }

    async fn dispatch_event_triggers_snapshot_on_host<F>(
        self: &Arc<Self>,
        host_id: String,
        build_msg: F,
        op_label: &'static str,
    ) -> Result<EventTriggersSnapshotView, String>
    where
        F: FnOnce(Uuid) -> WireMessage + Send + 'static,
    {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err(format!("Host is disconnected; reconnect to {op_label}"));
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_event_triggers.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some(format!("Host disconnected before {op_label}")),
                        None => Some(format!("host `{host_id}` removed")),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session.send_wire(build_msg(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_event_triggers.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to {op_label}: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected while waiting to {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_event_triggers.remove(&req_id);
                    }
                    Err(format!(
                        "Timed out waiting for daemon to {op_label} (ensure orbit daemon is up to date)"
                    ))
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    pub async fn list_event_triggers_on_host(
        self: &Arc<Self>,
        host_id: &str,
    ) -> Result<EventTriggersSnapshotView, String> {
        self.dispatch_event_triggers_snapshot_on_host(
            host_id.to_string(),
            move |request_id| WireMessage::ListEventTriggers { request_id },
            "list event triggers",
        )
        .await
    }

    pub async fn upsert_event_trigger_on_host(
        self: &Arc<Self>,
        req: UpsertEventTriggerRequest,
    ) -> Result<EventTriggersSnapshotView, String> {
        let host_id = req.host_id;
        self.dispatch_event_triggers_snapshot_on_host(
            host_id,
            move |request_id| WireMessage::UpsertEventTrigger {
                request_id,
                trigger_id: req.trigger_id,
                name: req.name,
                source: req.source,
                event_filters: req.event_filters,
                prompt_template: req.prompt_template,
                target: req.target,
                secret: req.secret,
                keep_existing_secret: req.keep_existing_secret,
                github_poll: req.github_poll,
                github_token: req.github_token,
                keep_existing_token: req.keep_existing_token,
                enabled: req.enabled,
            },
            "save event trigger",
        )
        .await
    }

    pub async fn delete_event_trigger_on_host(
        self: &Arc<Self>,
        host_id: &str,
        trigger_id: String,
    ) -> Result<EventTriggersSnapshotView, String> {
        self.dispatch_event_triggers_snapshot_on_host(
            host_id.to_string(),
            move |request_id| WireMessage::DeleteEventTrigger {
                request_id,
                trigger_id,
            },
            "delete event trigger",
        )
        .await
    }

    pub async fn test_event_trigger_on_host(
        self: &Arc<Self>,
        host_id: &str,
        trigger_id: String,
        sample_event_type: String,
        sample_payload: serde_json::Value,
    ) -> Result<EventTriggerDelivery, String> {
        let host_id = host_id.to_string();
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?
                .session
                .clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to test event trigger".to_string());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_event_trigger_tests.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some("Host disconnected before test event trigger".to_string()),
                        None => Some(format!("host `{host_id}` removed")),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session
                .send_wire(WireMessage::TestEventTrigger {
                    request_id: req_id,
                    trigger_id,
                    sample_event_type,
                    sample_payload,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_event_trigger_tests.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to test event trigger: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => {
                    Err("Host disconnected while waiting for event trigger test".to_string())
                }
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_event_trigger_tests.remove(&req_id);
                    }
                    Err("Timed out waiting for event trigger test result".to_string())
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| "test event trigger task cancelled".to_string())?
    }

    pub async fn list_path_rules_on_host(
        self: &Arc<Self>,
        host_id: String,
        cwd: String,
    ) -> Result<PathRulesSnapshotView, String> {
        self.dispatch_path_rules_snapshot_on_host(
            host_id,
            move |request_id| WireMessage::ListPathRules { request_id, cwd },
            "list path rules",
        )
        .await
    }

    pub async fn upsert_path_rule_on_host(
        self: &Arc<Self>,
        req: UpsertPathRuleRequest,
    ) -> Result<PathRulesSnapshotView, String> {
        self.dispatch_path_rules_snapshot_on_host(
            req.host_id,
            move |request_id| WireMessage::UpsertPathRule {
                request_id,
                cwd: req.cwd,
                scope: req.scope,
                name: req.name,
                description: req.description,
                trigger_mode: req.trigger_mode,
                globs: req.globs,
                enabled: req.enabled,
                body: req.body,
                expected_etag: req.expected_etag,
            },
            "save path rule",
        )
        .await
    }

    pub async fn delete_path_rule_on_host(
        self: &Arc<Self>,
        req: DeletePathRuleRequest,
    ) -> Result<PathRulesSnapshotView, String> {
        self.dispatch_path_rules_snapshot_on_host(
            req.host_id,
            move |request_id| WireMessage::DeletePathRule {
                request_id,
                cwd: req.cwd,
                path: req.path,
            },
            "delete path rule",
        )
        .await
    }

    pub async fn match_path_rules_on_host(
        self: &Arc<Self>,
        req: MatchPathRulesRequest,
    ) -> Result<MatchedPathRulesView, String> {
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
            return Err("Host is disconnected; reconnect to match path rules".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&req.host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_path_rule_matches.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some("Host disconnected before matching path rules".to_string()),
                        None => Some(format!("host `{}` removed", req.host_id)),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            let send_res = session
                .send_wire(WireMessage::MatchPathRules {
                    request_id: req_id,
                    cwd: req.cwd,
                    active_paths: req.active_paths,
                    manual_rule_names: req.manual_rule_names,
                })
                .await;

            if let Err(e) = send_res {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&req.host_id)
                {
                    h.pending_path_rule_matches.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to match path rules: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while matching path rules".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&req.host_id)
                    {
                        h.pending_path_rule_matches.remove(&req_id);
                    }
                    Err(
                        "Timed out waiting for daemon to match path rules (ensure orbit daemon is up to date)"
                            .to_string(),
                    )
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| "match path rules task cancelled".to_string())?
    }

    /// Arm a new one-shot conditional timer on any connected host.
    pub async fn create_conditional_timer_on_host(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        duration_seconds: u64,
        prompt: String,
        condition: TimerCondition,
    ) -> Result<ConditionalTimerRecord, String> {
        let host_id = host_id.to_string();
        let agent_id = agent_id.to_string();
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to create a timer".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_timer_creates.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before creating timer".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::CreateConditionalTimer {
                    request_id: req_id,
                    agent_id,
                    duration_seconds,
                    prompt,
                    condition,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_timer_creates.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to create conditional timer: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while creating timer".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_timer_creates.remove(&req_id);
                    }
                    Err("Timed out waiting for daemon to create timer".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "create conditional timer task cancelled".to_string())?
    }

    /// List armed (and optionally resolved) one-shot conditional timers on any connected host.
    pub async fn list_conditional_timers_on_host(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: Option<String>,
        include_resolved: bool,
    ) -> Result<Vec<ConditionalTimerRecord>, String> {
        let host_id = host_id.to_string();
        let update_host_cache = agent_id.is_none() && include_resolved;
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to list timers".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_timer_lists.insert(req_id, ack_tx);
                        if update_host_cache {
                            h.background_timer_list_reqs.insert(req_id);
                        }
                        None
                    }
                    Some(_) => Some("Host disconnected before listing timers".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ListConditionalTimers {
                    request_id: req_id,
                    agent_id,
                    include_resolved,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_timer_lists.remove(&req_id);
                    h.background_timer_list_reqs.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to list conditional timers: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while listing timers".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_timer_lists.remove(&req_id);
                        h.background_timer_list_reqs.remove(&req_id);
                    }
                    Err("Timed out waiting for daemon to list timers".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "list conditional timers task cancelled".to_string())?
    }

    /// Manually cancel an armed conditional timer on any connected host.
    pub async fn cancel_conditional_timer_on_host(
        self: &Arc<Self>,
        host_id: &str,
        timer_id: &str,
    ) -> Result<ConditionalTimerRecord, String> {
        self.dispatch_conditional_timer_update_rpc(
            host_id.to_string(),
            {
                let timer_id = timer_id.to_string();
                move |request_id| WireMessage::CancelConditionalTimer {
                    request_id,
                    timer_id,
                }
            },
            "cancel conditional timer",
        )
        .await
    }

    /// Immediately fire an armed conditional timer on any connected host.
    pub async fn fire_conditional_timer_now_on_host(
        self: &Arc<Self>,
        host_id: &str,
        timer_id: &str,
    ) -> Result<ConditionalTimerRecord, String> {
        self.dispatch_conditional_timer_update_rpc(
            host_id.to_string(),
            {
                let timer_id = timer_id.to_string();
                move |request_id| WireMessage::FireConditionalTimerNow {
                    request_id,
                    timer_id,
                }
            },
            "fire conditional timer",
        )
        .await
    }

    async fn dispatch_conditional_timer_update_rpc(
        self: &Arc<Self>,
        host_id: String,
        build_wire: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        op_label: &'static str,
    ) -> Result<ConditionalTimerRecord, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err(format!("Host is disconnected; reconnect to {op_label}"));
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_timer_updates.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some(format!("Host disconnected before {op_label}")),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session.send_wire(build_wire(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_timer_updates.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to {op_label}: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected while trying to {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_timer_updates.remove(&req_id);
                    }
                    Err(format!("Timed out waiting for daemon to {op_label}"))
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    /// Fetch Guardian auto-approval policy and recent audit entries from any connected host.
    pub async fn get_guardian_config_on_host(
        self: &Arc<Self>,
        host_id: &str,
    ) -> Result<GuardianConfigSnapshotView, String> {
        self.dispatch_guardian_config_rpc(
            host_id.to_string(),
            |request_id| WireMessage::GetGuardianConfig { request_id },
            "load Guardian configuration",
            Duration::from_secs(15),
        )
        .await
    }

    /// Persist updated Guardian auto-approval policy on any connected host.
    pub async fn update_guardian_config_on_host(
        self: &Arc<Self>,
        host_id: &str,
        config: GuardianPolicyConfig,
    ) -> Result<GuardianConfigSnapshotView, String> {
        self.dispatch_guardian_config_rpc(
            host_id.to_string(),
            move |request_id| WireMessage::UpdateGuardianConfig { request_id, config },
            "update Guardian configuration",
            Duration::from_secs(15),
        )
        .await
    }

    async fn dispatch_guardian_config_rpc(
        self: &Arc<Self>,
        host_id: String,
        build_wire: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        op_label: &'static str,
        timeout: Duration,
    ) -> Result<GuardianConfigSnapshotView, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err(format!("Host is disconnected; reconnect to {op_label}"));
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_guardian_configs.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some(format!("Host disconnected before {op_label}")),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session.send_wire(build_wire(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_guardian_configs.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to {op_label}: {e}")));
                return;
            }
            let out = match tokio::time::timeout(timeout, ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected while trying to {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_guardian_configs.remove(&req_id);
                    }
                    Err(format!("Timed out waiting for daemon to {op_label}"))
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    /// List configured bidirectional IM channels and active thread bindings on any connected host.
    pub async fn list_im_channels_on_host(
        self: &Arc<Self>,
        host_id: &str,
    ) -> Result<ImChannelsSnapshotView, String> {
        self.dispatch_im_channels_rpc(
            host_id.to_string(),
            |request_id| WireMessage::ListImChannels { request_id },
            "load IM channels",
            Duration::from_secs(15),
        )
        .await
    }

    /// Create or update an IM channel on any connected host.
    pub async fn upsert_im_channel_on_host(
        self: &Arc<Self>,
        host_id: &str,
        req: UpsertImChannelRequest,
    ) -> Result<ImChannelsSnapshotView, String> {
        self.dispatch_im_channels_rpc(
            host_id.to_string(),
            move |request_id| WireMessage::UpsertImChannel {
                request_id,
                channel_id: req.channel_id,
                name: req.name,
                platform: req.platform,
                enabled: req.enabled,
                app_id: req.app_id,
                primary_secret: req.primary_secret,
                signing_secret: req.signing_secret,
                keep_existing_secrets: req.keep_existing_secrets,
                allowed_chat_ids: req.allowed_chat_ids,
                allowed_user_ids: req.allowed_user_ids,
                default_workspace_cwd: req.default_workspace_cwd,
                default_provider: req.default_provider,
                default_mode: req.default_mode,
                forward_approvals: req.forward_approvals,
            },
            "save IM channel",
            Duration::from_secs(15),
        )
        .await
    }

    /// Delete an IM channel and its bindings on any connected host.
    pub async fn delete_im_channel_on_host(
        self: &Arc<Self>,
        host_id: &str,
        channel_id: String,
    ) -> Result<ImChannelsSnapshotView, String> {
        self.dispatch_im_channels_rpc(
            host_id.to_string(),
            move |request_id| WireMessage::DeleteImChannel {
                request_id,
                channel_id,
            },
            "delete IM channel",
            Duration::from_secs(15),
        )
        .await
    }

    /// Send a test message through an IM channel on any connected host.
    pub async fn test_im_channel_on_host(
        self: &Arc<Self>,
        host_id: &str,
        channel_id: String,
        target_chat_id: String,
        message: String,
    ) -> Result<ImChannelTestResultView, String> {
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
            return Err("Host is disconnected; reconnect to test IM channel".to_string());
        };

        let host_id_owned = host_id.to_string();
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id_owned) {
                    Some(h) if h.session.is_some() => {
                        h.pending_im_channel_tests.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before test IM channel".to_string()),
                    None => Some(format!("host `{host_id_owned}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::TestImChannel {
                    request_id: req_id,
                    channel_id,
                    target_chat_id,
                    message,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id_owned)
                {
                    h.pending_im_channel_tests.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to test IM channel: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while testing IM channel".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id_owned)
                    {
                        h.pending_im_channel_tests.remove(&req_id);
                    }
                    Err("Timed out waiting for daemon to test IM channel".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "test IM channel task cancelled".to_string())?
    }

    async fn dispatch_im_channels_rpc(
        self: &Arc<Self>,
        host_id: String,
        build_wire: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        op_label: &'static str,
        timeout: Duration,
    ) -> Result<ImChannelsSnapshotView, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err(format!("Host is disconnected; reconnect to {op_label}"));
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_im_channels.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some(format!("Host disconnected before {op_label}")),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session.send_wire(build_wire(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_im_channels.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to {op_label}: {e}")));
                return;
            }
            let out = match tokio::time::timeout(timeout, ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected while trying to {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_im_channels.remove(&req_id);
                    }
                    Err(format!("Timed out waiting for daemon to {op_label}"))
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    /// List registered mobile push devices and push preferences on any connected host.
    pub async fn list_push_devices_on_host(
        self: &Arc<Self>,
        host_id: &str,
    ) -> Result<PushDevicesSnapshotView, String> {
        self.dispatch_push_snapshot_rpc(
            host_id.to_string(),
            |request_id| WireMessage::ListPushDevices { request_id },
            "load push notification settings",
            Duration::from_secs(15),
        )
        .await
    }

    /// Register or update a mobile push device token on any connected host.
    pub async fn register_push_device_on_host(
        self: &Arc<Self>,
        host_id: &str,
        req: RegisterPushDeviceRequest,
    ) -> Result<PushDevicesSnapshotView, String> {
        self.dispatch_push_snapshot_rpc(
            host_id.to_string(),
            move |request_id| WireMessage::RegisterPushDevice {
                request_id,
                device_id: req.device_id,
                platform: req.platform,
                device_name: req.device_name,
                device_token: req.device_token,
                sandbox: req.sandbox,
            },
            "register push device",
            Duration::from_secs(15),
        )
        .await
    }

    /// Unregister a mobile push device on any connected host.
    pub async fn unregister_push_device_on_host(
        self: &Arc<Self>,
        host_id: &str,
        device_id: String,
    ) -> Result<PushDevicesSnapshotView, String> {
        self.dispatch_push_snapshot_rpc(
            host_id.to_string(),
            move |request_id| WireMessage::UnregisterPushDevice {
                request_id,
                device_id,
            },
            "unregister push device",
            Duration::from_secs(15),
        )
        .await
    }

    /// Update push preferences and optional gateway auth token on any connected host.
    pub async fn update_push_preferences_on_host(
        self: &Arc<Self>,
        host_id: &str,
        req: UpdatePushPreferencesRequest,
    ) -> Result<PushDevicesSnapshotView, String> {
        self.dispatch_push_snapshot_rpc(
            host_id.to_string(),
            move |request_id| WireMessage::UpdatePushPreferences {
                request_id,
                preferences: req.preferences,
                gateway_auth_token: req.gateway_auth_token,
                keep_existing_gateway_token: req.keep_existing_gateway_token,
            },
            "update push preferences",
            Duration::from_secs(15),
        )
        .await
    }

    /// Trigger a test push notification on any connected host.
    pub async fn test_push_notification_on_host(
        self: &Arc<Self>,
        host_id: &str,
        device_id: Option<String>,
    ) -> Result<PushTestResultView, String> {
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
            return Err("Host is disconnected; reconnect to test push notification".to_string());
        };

        let host_id_owned = host_id.to_string();
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id_owned) {
                    Some(h) if h.session.is_some() => {
                        h.pending_push_tests.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before test push notification".to_string()),
                    None => Some(format!("host `{host_id_owned}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::TestPushNotification {
                    request_id: req_id,
                    device_id,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id_owned)
                {
                    h.pending_push_tests.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send test push: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while sending test push".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id_owned)
                    {
                        h.pending_push_tests.remove(&req_id);
                    }
                    Err("Timed out waiting for daemon to send test push".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "test push notification task cancelled".to_string())?
    }

    pub fn list_push_devices_blocking(
        self: &Arc<Self>,
        host_id: &str,
    ) -> Result<PushDevicesSnapshotView, String> {
        let core = Arc::clone(self);
        let host = host_id.to_string();
        self.runtime
            .block_on(async move { core.list_push_devices_on_host(&host).await })
    }

    pub fn register_push_device_blocking(
        self: &Arc<Self>,
        host_id: &str,
        req: RegisterPushDeviceRequest,
    ) -> Result<PushDevicesSnapshotView, String> {
        let core = Arc::clone(self);
        let host = host_id.to_string();
        self.runtime
            .block_on(async move { core.register_push_device_on_host(&host, req).await })
    }

    pub fn unregister_push_device_blocking(
        self: &Arc<Self>,
        host_id: &str,
        device_id: &str,
    ) -> Result<PushDevicesSnapshotView, String> {
        let core = Arc::clone(self);
        let host = host_id.to_string();
        let dev = device_id.to_string();
        self.runtime
            .block_on(async move { core.unregister_push_device_on_host(&host, dev).await })
    }

    async fn dispatch_push_snapshot_rpc(
        self: &Arc<Self>,
        host_id: String,
        build_wire: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        op_label: &'static str,
        timeout: Duration,
    ) -> Result<PushDevicesSnapshotView, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err(format!("Host is disconnected; reconnect to {op_label}"));
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_push_snapshots.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some(format!("Host disconnected before {op_label}")),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session.send_wire(build_wire(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_push_snapshots.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to {op_label}: {e}")));
                return;
            }
            let out = match tokio::time::timeout(timeout, ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected while trying to {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_push_snapshots.remove(&req_id);
                    }
                    Err(format!("Timed out waiting for daemon to {op_label}"))
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    /// List discovered and active LSP language servers for a workspace on any connected host.
    pub async fn list_lsp_servers_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
    ) -> Result<Vec<LspServerStatus>, String> {
        self.dispatch_lsp_servers_rpc(
            host_id.to_string(),
            {
                let cwd = cwd.to_string();
                move |request_id| WireMessage::ListLspServers { request_id, cwd }
            },
            "list LSP servers",
            Duration::from_secs(20),
        )
        .await
    }

    /// Restart (or start fresh) a language server for a workspace on any connected host.
    pub async fn restart_lsp_server_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
        server_id: &str,
    ) -> Result<Vec<LspServerStatus>, String> {
        self.dispatch_lsp_servers_rpc(
            host_id.to_string(),
            {
                let cwd = cwd.to_string();
                let server_id = server_id.to_string();
                move |request_id| WireMessage::RestartLspServer {
                    request_id,
                    cwd,
                    server_id,
                }
            },
            "restart LSP server",
            Duration::from_secs(20),
        )
        .await
    }

    async fn dispatch_lsp_servers_rpc(
        self: &Arc<Self>,
        host_id: String,
        build_wire: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        op_label: &'static str,
        timeout: Duration,
    ) -> Result<Vec<LspServerStatus>, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err(format!("Host is disconnected; reconnect to {op_label}"));
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_lsp_servers.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some(format!("Host disconnected before {op_label}")),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session.send_wire(build_wire(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_lsp_servers.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to {op_label}: {e}")));
                return;
            }
            let out = match tokio::time::timeout(timeout, ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected while trying to {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_lsp_servers.remove(&req_id);
                    }
                    Err(format!("Timed out waiting for daemon to {op_label}"))
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    /// Query workspace or document symbols via LSP on any connected host.
    pub async fn lsp_query_symbols_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
        query: &str,
        path: Option<String>,
        limit: u32,
    ) -> Result<Vec<LspSymbolInfo>, String> {
        let host_id = host_id.to_string();
        let cwd = cwd.to_string();
        let query = query.to_string();
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to query LSP symbols".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_lsp_symbols.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before querying LSP symbols".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::LspQuerySymbols {
                    request_id: req_id,
                    cwd,
                    query,
                    path,
                    limit,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_lsp_symbols.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send LSP symbol query: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while querying LSP symbols".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_lsp_symbols.remove(&req_id);
                    }
                    Err("Timed out waiting for LSP symbol query".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "LSP symbol query task cancelled".to_string())?
    }

    /// Fetch LSP diagnostics for a file or workspace on any connected host.
    pub async fn lsp_get_diagnostics_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
        path: Option<String>,
    ) -> Result<Vec<LspDiagnostic>, String> {
        let host_id = host_id.to_string();
        let cwd = cwd.to_string();
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to inspect diagnostics".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_lsp_diagnostics.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before fetching diagnostics".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::LspGetDiagnostics {
                    request_id: req_id,
                    cwd,
                    path,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_lsp_diagnostics.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send LSP diagnostics request: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while fetching diagnostics".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_lsp_diagnostics.remove(&req_id);
                    }
                    Err("Timed out waiting for LSP diagnostics".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "LSP diagnostics task cancelled".to_string())?
    }

    /// Resolve Go-to-Definition locations at a 1-based `(line, col)` on any connected host.
    pub async fn lsp_goto_definition_on_host(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
        path: &str,
        line: u32,
        col: u32,
    ) -> Result<Vec<LspLocation>, String> {
        let host_id = host_id.to_string();
        let cwd = cwd.to_string();
        let path = path.to_string();
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to resolve definitions".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_lsp_locations.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before resolving definition".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::LspGotoDefinition {
                    request_id: req_id,
                    cwd,
                    path,
                    line,
                    col,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_lsp_locations.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!(
                    "failed to send LSP goto-definition request: {e}"
                )));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while resolving definition".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_lsp_locations.remove(&req_id);
                    }
                    Err("Timed out waiting for LSP definition".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "LSP goto-definition task cancelled".to_string())?
    }

    /// Fetch host-wide cost/token usage insights and quota depletion forecasts for any connected host.
    pub async fn get_usage_insights_on_host(
        self: &Arc<Self>,
        host_id: &str,
        days: u32,
    ) -> Result<UsageInsightsSnapshot, String> {
        let host_id = host_id.to_string();
        let days = days.clamp(1, 90);
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to inspect usage insights".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_usage_insights.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before fetching usage insights".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::GetUsageInsights {
                    request_id: req_id,
                    days,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_usage_insights.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send usage insights request: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while fetching usage insights".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_usage_insights.remove(&req_id);
                    }
                    Err("Timed out waiting for usage insights".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "usage insights task cancelled".to_string())?
    }

    /// Fetch live context-window occupancy breakdown for an agent on any connected host.
    pub async fn get_context_occupancy_on_host(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
    ) -> Result<ContextOccupancyBreakdown, String> {
        let host_id = host_id.to_string();
        let agent_id = agent_id.to_string();
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };
        let Some(session) = session else {
            return Err("Host is disconnected; reconnect to inspect context occupancy".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_context_occupancy.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => {
                        Some("Host disconnected before fetching context occupancy".to_string())
                    }
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::GetContextOccupancy {
                    request_id: req_id,
                    agent_id,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_context_occupancy.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!(
                    "failed to send context occupancy request: {e}"
                )));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while fetching context occupancy".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_context_occupancy.remove(&req_id);
                    }
                    Err("Timed out waiting for context occupancy".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "context occupancy task cancelled".to_string())?
    }

    pub async fn start_goal_run_on_host(
        self: &Arc<Self>,
        req: StartGoalRunRequest,
    ) -> Result<GoalRunRecord, String> {
        let host_id = req.host_id;
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_goal_updates.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before starting goal run".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::StartGoalRun {
                    request_id: req_id,
                    agent_id: req.agent_id,
                    objective: req.objective,
                    criteria: req.criteria,
                    verify_command: req.verify_command,
                    max_iterations: req.max_iterations,
                    max_duration_secs: req.max_duration_secs,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_goal_updates.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send StartGoalRun: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while starting goal run".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_goal_updates.remove(&req_id);
                    }
                    Err("Timed out waiting for StartGoalRun reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "StartGoalRun task cancelled".to_string())?
    }

    pub async fn list_goal_runs_on_host(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: Option<String>,
    ) -> Result<Vec<GoalRunRecord>, String> {
        let host_id = host_id.to_string();
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let is_unfiltered = agent_id.is_none();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_goal_lists.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before listing goal runs".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ListGoalRuns {
                    request_id: req_id,
                    agent_id,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_goal_lists.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ListGoalRuns: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(Ok(goals))) => {
                    if is_unfiltered {
                        let updated = if let Ok(mut guard) = core.hosts.write()
                            && let Some(h) = guard.get_mut(&host_id)
                        {
                            h.goal_runs = goals.clone();
                            true
                        } else {
                            false
                        };
                        if updated {
                            core.emit(|revision| CoreNotification::HostChanged {
                                revision,
                                host_id: host_id.clone(),
                            });
                        }
                    }
                    Ok(goals)
                }
                Ok(Ok(Err(e))) => Err(e),
                Ok(Err(_)) => Err("Host disconnected while listing goal runs".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_goal_lists.remove(&req_id);
                    }
                    Err("Timed out waiting for ListGoalRuns reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "ListGoalRuns task cancelled".to_string())?
    }

    pub async fn control_goal_run_on_host(
        self: &Arc<Self>,
        req: ControlGoalRunRequest,
    ) -> Result<GoalRunRecord, String> {
        let host_id = req.host_id;
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_goal_updates.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before controlling goal run".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ControlGoalRun {
                    request_id: req_id,
                    goal_id: req.goal_id,
                    action: req.action,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_goal_updates.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ControlGoalRun: {e}")));
                return;
            }
            // VerifyNow can run up to verify timeout; allow generous wait
            let out = match tokio::time::timeout(Duration::from_secs(130), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while controlling goal run".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_goal_updates.remove(&req_id);
                    }
                    Err("Timed out waiting for ControlGoalRun reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "ControlGoalRun task cancelled".to_string())?
    }

    pub async fn start_monitor_watch_on_host(
        self: &Arc<Self>,
        req: StartMonitorWatchRequest,
    ) -> Result<MonitorWatchRecord, String> {
        let host_id = req.host_id;
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_monitor_updates.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before starting monitor".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::StartMonitorWatch {
                    request_id: req_id,
                    agent_id: req.agent_id,
                    name: req.name,
                    kind: req.kind,
                    expires_in_secs: req.expires_in_secs,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_monitor_updates.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send StartMonitorWatch: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while starting monitor".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_monitor_updates.remove(&req_id);
                    }
                    Err("Timed out waiting for StartMonitorWatch reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "StartMonitorWatch task cancelled".to_string())?
    }

    pub async fn list_monitor_watches_on_host(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: Option<String>,
    ) -> Result<Vec<MonitorWatchRecord>, String> {
        let host_id = host_id.to_string();
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let is_unfiltered = agent_id.is_none();
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_monitor_lists.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before listing monitors".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ListMonitorWatches {
                    request_id: req_id,
                    agent_id,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_monitor_lists.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ListMonitorWatches: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(Ok(monitors))) => {
                    if is_unfiltered {
                        let updated = if let Ok(mut guard) = core.hosts.write()
                            && let Some(h) = guard.get_mut(&host_id)
                        {
                            h.monitor_watches = monitors.clone();
                            true
                        } else {
                            false
                        };
                        if updated {
                            core.emit(|revision| CoreNotification::HostChanged {
                                revision,
                                host_id: host_id.clone(),
                            });
                        }
                    }
                    Ok(monitors)
                }
                Ok(Ok(Err(e))) => Err(e),
                Ok(Err(_)) => Err("Host disconnected while listing monitors".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_monitor_lists.remove(&req_id);
                    }
                    Err("Timed out waiting for ListMonitorWatches reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "ListMonitorWatches task cancelled".to_string())?
    }

    pub async fn stop_monitor_watch_on_host(
        self: &Arc<Self>,
        req: StopMonitorWatchRequest,
    ) -> Result<MonitorWatchRecord, String> {
        let host_id = req.host_id;
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_monitor_updates.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before stopping monitor".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::StopMonitorWatch {
                    request_id: req_id,
                    monitor_id: req.monitor_id,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_monitor_updates.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send StopMonitorWatch: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while stopping monitor".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_monitor_updates.remove(&req_id);
                    }
                    Err("Timed out waiting for StopMonitorWatch reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "StopMonitorWatch task cancelled".to_string())?
    }

    pub async fn list_browser_sessions_on_host(
        self: &Arc<Self>,
        host_id: &str,
    ) -> Result<BrowserSessionsSnapshotView, String> {
        let host_id = host_id.to_string();
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_browser_snapshots.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => {
                        Some("Host disconnected before listing browser sessions".to_string())
                    }
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ListBrowserSessions {
                    request_id: req_id,
                    include_screenshots: true,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_browser_snapshots.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ListBrowserSessions: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while listing browser sessions".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_browser_snapshots.remove(&req_id);
                    }
                    Err("Timed out waiting for ListBrowserSessions reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "ListBrowserSessions task cancelled".to_string())?
    }

    pub async fn create_browser_session_on_host(
        self: &Arc<Self>,
        req: CreateBrowserSessionRequest,
    ) -> Result<BrowserSessionSummary, String> {
        let host_id = req.host_id;
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_browser_sessions.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => {
                        Some("Host disconnected before creating browser session".to_string())
                    }
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::CreateBrowserSession {
                    request_id: req_id,
                    agent_id: req.agent_id,
                    initial_url: req.initial_url,
                    headless: req.headless,
                    viewport_width: req.viewport_width,
                    viewport_height: req.viewport_height,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_browser_sessions.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send CreateBrowserSession: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(30), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while creating browser session".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_browser_sessions.remove(&req_id);
                    }
                    Err("Timed out waiting for CreateBrowserSession reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "CreateBrowserSession task cancelled".to_string())?
    }

    pub async fn execute_browser_action_on_host(
        self: &Arc<Self>,
        req: ExecuteBrowserActionRequest,
    ) -> Result<BrowserActionResponseView, String> {
        let host_id = req.host_id;
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_browser_actions.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => {
                        Some("Host disconnected before executing browser action".to_string())
                    }
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ExecuteBrowserAction {
                    request_id: req_id,
                    session_id: req.session_id,
                    action: req.action,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_browser_actions.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ExecuteBrowserAction: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(30), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while executing browser action".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_browser_actions.remove(&req_id);
                    }
                    Err("Timed out waiting for ExecuteBrowserAction reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "ExecuteBrowserAction task cancelled".to_string())?
    }

    pub async fn close_browser_session_on_host(
        self: &Arc<Self>,
        host_id: &str,
        session_id: &str,
    ) -> Result<BrowserSessionsSnapshotView, String> {
        let host_id = host_id.to_string();
        let session_id = session_id.to_string();
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_browser_snapshots.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before closing browser session".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::CloseBrowserSession {
                    request_id: req_id,
                    session_id,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_browser_snapshots.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send CloseBrowserSession: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while closing browser session".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_browser_snapshots.remove(&req_id);
                    }
                    Err("Timed out waiting for CloseBrowserSession reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "CloseBrowserSession task cancelled".to_string())?
    }

    pub async fn update_browser_policy_on_host(
        self: &Arc<Self>,
        host_id: &str,
        policy: BrowserPolicyConfig,
    ) -> Result<BrowserSessionsSnapshotView, String> {
        let host_id = host_id.to_string();
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_browser_snapshots.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before updating browser policy".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::UpdateBrowserPolicy {
                    request_id: req_id,
                    policy,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_browser_snapshots.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send UpdateBrowserPolicy: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while updating browser policy".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_browser_snapshots.remove(&req_id);
                    }
                    Err("Timed out waiting for UpdateBrowserPolicy reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "UpdateBrowserPolicy task cancelled".to_string())?
    }

    async fn dispatch_workflow_update_rpc(
        self: &Arc<Self>,
        host_id: String,
        build_wire: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        op_label: &'static str,
        timeout_secs: u64,
    ) -> Result<WorkflowRunRecord, String> {
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_workflow_updates.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some(format!("Host disconnected before {op_label}")),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session.send_wire(build_wire(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_workflow_updates.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send {op_label}: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(timeout_secs), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected during {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_workflow_updates.remove(&req_id);
                    }
                    Err(format!("Timed out waiting for {op_label} reply"))
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    pub async fn start_batch_fanout_on_host(
        self: &Arc<Self>,
        req: StartBatchFanoutRequest,
    ) -> Result<WorkflowRunRecord, String> {
        self.dispatch_workflow_update_rpc(
            req.host_id,
            move |request_id| WireMessage::StartBatchFanout {
                request_id,
                name: req.name,
                cwd: req.cwd,
                csv_content: req.csv_content,
                prompt_template: req.prompt_template,
                provider: req.provider,
                model: req.model,
                mode: req.mode,
                max_concurrency: req.max_concurrency.unwrap_or(3).clamp(1, 8),
                isolated_worktree: req.isolated_worktree,
            },
            "StartBatchFanout",
            30,
        )
        .await
    }

    pub async fn start_dag_workflow_on_host(
        self: &Arc<Self>,
        req: StartDagWorkflowRequest,
    ) -> Result<WorkflowRunRecord, String> {
        self.dispatch_workflow_update_rpc(
            req.host_id,
            move |request_id| WireMessage::StartDagWorkflow {
                request_id,
                name: req.name,
                cwd: req.cwd,
                steps: req.steps,
                default_provider: req.default_provider,
                default_mode: req.default_mode,
                max_concurrency: req.max_concurrency.unwrap_or(3).clamp(1, 8),
            },
            "StartDagWorkflow",
            30,
        )
        .await
    }

    pub async fn start_best_of_n_on_host(
        self: &Arc<Self>,
        req: StartBestOfNRequest,
    ) -> Result<WorkflowRunRecord, String> {
        self.dispatch_workflow_update_rpc(
            req.host_id,
            move |request_id| WireMessage::StartBestOfN {
                request_id,
                name: req.name,
                cwd: req.cwd,
                prompt: req.prompt,
                candidates: req.candidates,
                mode: req.mode,
                verify_command: req.verify_command,
            },
            "StartBestOfN",
            30,
        )
        .await
    }

    pub async fn list_workflow_runs_on_host(
        self: &Arc<Self>,
        host_id: &str,
        include_completed: bool,
    ) -> Result<Vec<WorkflowRunRecord>, String> {
        let host_id = host_id.to_string();
        let session = self.require_online_session(&host_id)?;
        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();
        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_workflow_lists.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before listing workflow runs".to_string()),
                    None => Some(format!("host `{host_id}` removed")),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };
            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }
            if let Err(e) = session
                .send_wire(WireMessage::ListWorkflowRuns {
                    request_id: req_id,
                    include_completed,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_workflow_lists.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to send ListWorkflowRuns: {e}")));
                return;
            }
            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(Ok(runs))) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        if include_completed {
                            h.workflow_runs = runs.clone();
                        } else {
                            for run in &runs {
                                upsert_cached_workflow_run(&mut h.workflow_runs, run.clone());
                            }
                        }
                    }
                    core.emit(|revision| CoreNotification::HostChanged {
                        revision,
                        host_id: host_id.clone(),
                    });
                    Ok(runs)
                }
                Ok(Ok(Err(e))) => Err(e),
                Ok(Err(_)) => Err("Host disconnected while listing workflow runs".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_workflow_lists.remove(&req_id);
                    }
                    Err("Timed out waiting for ListWorkflowRuns reply".to_string())
                }
            };
            let _ = res_tx.send(out);
        });
        res_rx
            .await
            .map_err(|_| "ListWorkflowRuns task cancelled".to_string())?
    }

    pub async fn cancel_workflow_run_on_host(
        self: &Arc<Self>,
        req: CancelWorkflowRunRequest,
    ) -> Result<WorkflowRunRecord, String> {
        self.dispatch_workflow_update_rpc(
            req.host_id,
            move |request_id| WireMessage::CancelWorkflowRun {
                request_id,
                run_id: req.run_id,
            },
            "CancelWorkflowRun",
            20,
        )
        .await
    }

    pub async fn select_best_of_n_winner_on_host(
        self: &Arc<Self>,
        req: SelectBestOfNWinnerRequest,
    ) -> Result<WorkflowRunRecord, String> {
        self.dispatch_workflow_update_rpc(
            req.host_id,
            move |request_id| WireMessage::SelectBestOfNWinner {
                request_id,
                run_id: req.run_id,
                winner_step_id: req.winner_step_id,
                apply_to_workspace: req.apply_to_workspace,
            },
            "SelectBestOfNWinner",
            45,
        )
        .await
    }

    fn require_online_session(&self, host_id: &str) -> Result<HostSessionHandle, String> {
        let guard = self
            .hosts
            .read()
            .map_err(|_| "host state lock poisoned".to_string())?;
        let h = guard
            .get(host_id)
            .ok_or_else(|| format!("host `{host_id}` not found"))?;
        h.session
            .clone()
            .ok_or_else(|| format!("host `{host_id}` is currently offline"))
    }

    async fn dispatch_workspace_entry_mutation_on_host(
        self: &Arc<Self>,
        host_id: String,
        build_wire: impl FnOnce(Uuid) -> WireMessage + Send + 'static,
        op_label: &'static str,
    ) -> Result<WorkspaceEntryMutationResultView, String> {
        let session = {
            let guard = self
                .hosts
                .read()
                .map_err(|_| "host state lock poisoned".to_string())?;
            let h = guard
                .get(&host_id)
                .ok_or_else(|| format!("host `{host_id}` not found"))?;
            h.session.clone()
        };

        let Some(session) = session else {
            return Err(format!("Host is disconnected; reconnect to {op_label}"));
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = {
                match core.hosts.write() {
                    Ok(mut guard) => match guard.get_mut(&host_id) {
                        Some(h) if h.session.is_some() => {
                            h.pending_file_mutations.insert(req_id, ack_tx);
                            None
                        }
                        Some(_) => Some(format!("Host disconnected before {op_label}")),
                        None => Some(format!("host `{host_id}` removed")),
                    },
                    Err(_) => Some("host state lock poisoned".to_string()),
                }
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session.send_wire(build_wire(req_id)).await {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&host_id)
                {
                    h.pending_file_mutations.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to {op_label}: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err(format!("Host disconnected while trying to {op_label}")),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&host_id)
                    {
                        h.pending_file_mutations.remove(&req_id);
                    }
                    Err(format!(
                        "Timed out waiting for daemon to {op_label} (ensure orbit daemon is up to date)"
                    ))
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| format!("{op_label} task cancelled"))?
    }

    pub async fn create_workspace_entry_on_host(
        self: &Arc<Self>,
        req: CreateWorkspaceEntryRequest,
    ) -> Result<WorkspaceEntryMutationResultView, String> {
        let host_id = req.host_id;
        let cwd = req.cwd;
        let path = req.path;
        let is_dir = req.is_dir;
        let initial_content = req.initial_content;
        self.dispatch_workspace_entry_mutation_on_host(
            host_id,
            move |request_id| WireMessage::CreateWorkspaceEntry {
                request_id,
                cwd,
                path,
                is_dir,
                initial_content,
            },
            "create workspace entry",
        )
        .await
    }

    pub async fn rename_workspace_entry_on_host(
        self: &Arc<Self>,
        req: RenameWorkspaceEntryRequest,
    ) -> Result<WorkspaceEntryMutationResultView, String> {
        let host_id = req.host_id;
        let cwd = req.cwd;
        let old_path = req.old_path;
        let new_path = req.new_path;
        self.dispatch_workspace_entry_mutation_on_host(
            host_id,
            move |request_id| WireMessage::RenameWorkspaceEntry {
                request_id,
                cwd,
                old_path,
                new_path,
            },
            "rename workspace entry",
        )
        .await
    }

    pub async fn delete_workspace_entry_on_host(
        self: &Arc<Self>,
        req: DeleteWorkspaceEntryRequest,
    ) -> Result<WorkspaceEntryMutationResultView, String> {
        let host_id = req.host_id;
        let cwd = req.cwd;
        let path = req.path;
        self.dispatch_workspace_entry_mutation_on_host(
            host_id,
            move |request_id| WireMessage::DeleteWorkspaceEntry {
                request_id,
                cwd,
                path,
            },
            "delete workspace entry",
        )
        .await
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

    pub fn rewind_agent_blocking(
        self: &Arc<Self>,
        host_id: &str,
        agent_id: &str,
        to_before_seq: u64,
        restore_files: bool,
    ) -> Result<RewindAgentResultView, String> {
        self.runtime.block_on(self.rewind_agent_on_host(
            host_id,
            agent_id,
            to_before_seq,
            restore_files,
        ))
    }

    pub fn fork_agent_blocking(
        self: &Arc<Self>,
        req: ForkAgentRequest,
    ) -> Result<ForkAgentResultView, String> {
        self.runtime.block_on(self.fork_agent_on_host(req))
    }

    pub fn preview_worktree_handoff_blocking(
        self: &Arc<Self>,
        req: PreviewWorktreeHandoffRequest,
    ) -> Result<WorktreeHandoffPreview, String> {
        self.runtime
            .block_on(self.preview_worktree_handoff_on_host(req))
    }

    pub fn execute_worktree_handoff_blocking(
        self: &Arc<Self>,
        req: ExecuteWorktreeHandoffRequest,
    ) -> Result<WorktreeHandoffOutcome, String> {
        self.runtime
            .block_on(self.execute_worktree_handoff_on_host(req))
    }

    pub fn read_workspace_file_blocking(
        self: &Arc<Self>,
        host_id: &str,
        cwd: &str,
        path: &str,
    ) -> Result<WorkspaceFileView, String> {
        self.runtime
            .block_on(self.read_workspace_file_on_host(host_id, cwd, path))
    }

    pub fn search_workspace_symbols_blocking(
        self: &Arc<Self>,
        req: crate::contract::SearchWorkspaceSymbolsRequest,
    ) -> Result<crate::contract::WorkspaceSymbolsView, String> {
        self.runtime
            .block_on(self.search_workspace_symbols_on_host(req))
    }

    pub fn write_workspace_file_blocking(
        self: &Arc<Self>,
        req: WriteWorkspaceFileRequest,
    ) -> Result<WorkspaceFileWriteResultView, String> {
        self.runtime
            .block_on(self.write_workspace_file_on_host(req))
    }

    pub fn create_workspace_entry_blocking(
        self: &Arc<Self>,
        req: CreateWorkspaceEntryRequest,
    ) -> Result<WorkspaceEntryMutationResultView, String> {
        self.runtime
            .block_on(self.create_workspace_entry_on_host(req))
    }

    pub fn rename_workspace_entry_blocking(
        self: &Arc<Self>,
        req: RenameWorkspaceEntryRequest,
    ) -> Result<WorkspaceEntryMutationResultView, String> {
        self.runtime
            .block_on(self.rename_workspace_entry_on_host(req))
    }

    pub fn delete_workspace_entry_blocking(
        self: &Arc<Self>,
        req: DeleteWorkspaceEntryRequest,
    ) -> Result<WorkspaceEntryMutationResultView, String> {
        self.runtime
            .block_on(self.delete_workspace_entry_on_host(req))
    }

    /// Discover the full project instruction hierarchy (`CLAUDE.md` / `AGENTS.md` / `AGENTS.override.md` / etc.)
    /// for `cwd` on any connected host.
    pub async fn list_instruction_files_on_host(
        self: &Arc<Self>,
        req: ListInstructionFilesRequest,
    ) -> Result<InstructionFilesSnapshotView, String> {
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
            return Err("Host is disconnected; reconnect to inspect project instructions".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&req.host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_instruction_lists.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before listing instructions".to_string()),
                    None => Some(format!("host `{}` removed", req.host_id)),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session
                .send_wire(WireMessage::ListInstructionFiles {
                    request_id: req_id,
                    cwd: req.cwd,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&req.host_id)
                {
                    h.pending_instruction_lists.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to list instruction files: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while listing instructions".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&req.host_id)
                    {
                        h.pending_instruction_lists.remove(&req_id);
                    }
                    Err(
                        "Timed out waiting for daemon to list project instructions (ensure orbit daemon is up to date)"
                            .to_string(),
                    )
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| "list instructions task cancelled".to_string())?
    }

    /// Read an allowlisted instruction file on any connected host.
    pub async fn read_instruction_file_on_host(
        self: &Arc<Self>,
        req: ReadInstructionFileRequest,
    ) -> Result<InstructionFileContentView, String> {
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
            return Err("Host is disconnected; reconnect to read instruction file".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&req.host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_instruction_reads.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before reading instruction file".to_string()),
                    None => Some(format!("host `{}` removed", req.host_id)),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session
                .send_wire(WireMessage::ReadInstructionFile {
                    request_id: req_id,
                    cwd: req.cwd,
                    path: req.path,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&req.host_id)
                {
                    h.pending_instruction_reads.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to read instruction file: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(15), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while reading instruction file".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&req.host_id)
                    {
                        h.pending_instruction_reads.remove(&req_id);
                    }
                    Err(
                        "Timed out waiting for daemon to read instruction file (ensure orbit daemon is up to date)"
                            .to_string(),
                    )
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| "read instruction file task cancelled".to_string())?
    }

    /// Atomically save an allowlisted instruction file on any connected host with optimistic etag concurrency.
    pub async fn save_instruction_file_on_host(
        self: &Arc<Self>,
        req: SaveInstructionFileRequest,
    ) -> Result<InstructionFileSavedView, String> {
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
            return Err("Host is disconnected; reconnect to save instruction file".into());
        };

        let core = Arc::clone(self);
        let (res_tx, res_rx) = tokio::sync::oneshot::channel();

        self.runtime.spawn(async move {
            let req_id = Uuid::new_v4();
            let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

            let register_err = match core.hosts.write() {
                Ok(mut guard) => match guard.get_mut(&req.host_id) {
                    Some(h) if h.session.is_some() => {
                        h.pending_instruction_saves.insert(req_id, ack_tx);
                        None
                    }
                    Some(_) => Some("Host disconnected before saving instruction file".to_string()),
                    None => Some(format!("host `{}` removed", req.host_id)),
                },
                Err(_) => Some("host state lock poisoned".to_string()),
            };

            if let Some(err) = register_err {
                let _ = res_tx.send(Err(err));
                return;
            }

            if let Err(e) = session
                .send_wire(WireMessage::SaveInstructionFile {
                    request_id: req_id,
                    cwd: req.cwd,
                    path: req.path,
                    content: req.content,
                    expected_etag: req.expected_etag,
                })
                .await
            {
                if let Ok(mut guard) = core.hosts.write()
                    && let Some(h) = guard.get_mut(&req.host_id)
                {
                    h.pending_instruction_saves.remove(&req_id);
                }
                let _ = res_tx.send(Err(format!("failed to save instruction file: {e}")));
                return;
            }

            let out = match tokio::time::timeout(Duration::from_secs(20), ack_rx).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => Err("Host disconnected while saving instruction file".to_string()),
                Err(_) => {
                    if let Ok(mut guard) = core.hosts.write()
                        && let Some(h) = guard.get_mut(&req.host_id)
                    {
                        h.pending_instruction_saves.remove(&req_id);
                    }
                    Err(
                        "Timed out waiting for daemon to save instruction file (ensure orbit daemon is up to date)"
                            .to_string(),
                    )
                }
            };
            let _ = res_tx.send(out);
        });

        res_rx
            .await
            .map_err(|_| "save instruction file task cancelled".to_string())?
    }

    pub fn list_instruction_files_blocking(
        self: &Arc<Self>,
        req: ListInstructionFilesRequest,
    ) -> Result<InstructionFilesSnapshotView, String> {
        self.runtime
            .block_on(self.list_instruction_files_on_host(req))
    }

    pub fn read_instruction_file_blocking(
        self: &Arc<Self>,
        req: ReadInstructionFileRequest,
    ) -> Result<InstructionFileContentView, String> {
        self.runtime
            .block_on(self.read_instruction_file_on_host(req))
    }

    pub fn save_instruction_file_blocking(
        self: &Arc<Self>,
        req: SaveInstructionFileRequest,
    ) -> Result<InstructionFileSavedView, String> {
        self.runtime
            .block_on(self.save_instruction_file_on_host(req))
    }

    pub fn ask_side_question_with_callback<F>(
        self: &Arc<Self>,
        req: AskSideQuestionRequest,
        on_event: F,
    ) -> Result<String, String>
    where
        F: Fn(SideQuestionStreamEvent) + Send + Sync + 'static,
    {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let request_id = self
            .runtime
            .block_on(self.ask_side_question_on_host(req, tx))?;
        self.runtime.spawn(async move {
            while let Some(ev) = rx.recv().await {
                on_event(ev);
            }
        });
        Ok(request_id)
    }

    // --- Zero-Server E2EE Cloud Sync (GitHub Device Flow + Secret Gist) ---

    fn cloud_client(&self) -> GitHubCloudClient {
        self.cloud_sync_client
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn set_cloud_sync_github_bases_for_test(
        &self,
        web_base: impl Into<String>,
        api_base: impl Into<String>,
    ) {
        if let Ok(mut g) = self.cloud_sync_client.write() {
            *g = GitHubCloudClient::with_bases(web_base, api_base);
        }
    }

    fn record_cloud_meta_mutation(
        &self,
        f: impl FnOnce(&mut crate::cloud_sync::CloudSyncLocalMeta),
    ) {
        let mut meta = load_sync_meta_from_store(&self.store);
        f(&mut meta);
        let _ = save_sync_meta_to_store(&self.store, &meta);
    }

    /// Inspect current Cloud Sync state, GitHub account identity, and last sync summary.
    pub fn cloud_sync_status(&self) -> CloudSyncStatusView {
        let cfg = load_sync_config_from_store(&self.store);
        cfg.to_status_view(self.cloud_sync_in_flight.load(Ordering::SeqCst))
    }

    /// Generate a high-entropy human-readable Recovery Key (`orbit-XXXX-XXXX-XXXX-XXXX-XXXX`).
    pub fn cloud_sync_generate_passphrase(&self) -> String {
        generate_sync_passphrase()
    }

    /// Persist (or clear) a custom GitHub OAuth App Client ID for Device Flow.
    pub fn cloud_sync_set_github_client_id(
        &self,
        client_id: Option<String>,
    ) -> Result<CloudSyncStatusView, ClientCoreError> {
        let mut cfg = load_sync_config_from_store(&self.store);
        cfg.github_client_id = client_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        save_sync_config_to_store(&self.store, &cfg)?;
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(cfg.to_status_view(self.cloud_sync_in_flight.load(Ordering::SeqCst)))
    }

    /// Toggle automatic background Cloud Sync when hosts/projects/preferences change.
    pub fn cloud_sync_set_auto_sync(
        &self,
        enabled: bool,
    ) -> Result<CloudSyncStatusView, ClientCoreError> {
        let mut cfg = load_sync_config_from_store(&self.store);
        cfg.auto_sync_enabled = enabled;
        save_sync_config_to_store(&self.store, &cfg)?;
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(cfg.to_status_view(self.cloud_sync_in_flight.load(Ordering::SeqCst)))
    }

    /// Start GitHub OAuth 2.0 Device Authorization Flow (`scope=gist`, zero backend server).
    pub fn cloud_sync_start_github_device_flow(
        &self,
        client_id_override: Option<String>,
    ) -> Result<GitHubDeviceFlowStartView, ClientCoreError> {
        let mut cfg = load_sync_config_from_store(&self.store);
        if let Some(cid) = client_id_override
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            cfg.github_client_id = Some(cid.to_string());
            save_sync_config_to_store(&self.store, &cfg)?;
        }
        let cid = resolve_github_client_id(
            client_id_override.as_deref(),
            cfg.github_client_id.as_deref(),
        )
        .ok_or(CloudSyncError::MissingClientId)?;
        let client = self.cloud_client();
        let started = self.runtime.block_on(client.start_device_flow(&cid))?;
        Ok(started)
    }

    /// Poll GitHub's Device Flow endpoint once. When authorized, immediately verifies the user profile
    /// and checks whether an existing Orbit Encrypted Vault Secret Gist already exists.
    pub fn cloud_sync_poll_github_device_flow(
        &self,
        device_code: &str,
        client_id_override: Option<String>,
    ) -> Result<CloudSyncDevicePollView, ClientCoreError> {
        let cfg = load_sync_config_from_store(&self.store);
        let cid = resolve_github_client_id(
            client_id_override.as_deref(),
            cfg.github_client_id.as_deref(),
        )
        .ok_or(CloudSyncError::MissingClientId)?;
        let client = self.cloud_client();
        let outcome = self
            .runtime
            .block_on(client.poll_device_flow_once(&cid, device_code))?;

        match outcome {
            DevicePollOutcome::Pending => Ok(CloudSyncDevicePollView {
                status: CloudSyncDevicePollStatus::Pending,
                interval_secs: None,
                error: None,
                sync_status: None,
            }),
            DevicePollOutcome::SlowDown { interval_secs } => Ok(CloudSyncDevicePollView {
                status: CloudSyncDevicePollStatus::SlowDown,
                interval_secs: Some(interval_secs),
                error: None,
                sync_status: None,
            }),
            DevicePollOutcome::Expired => Ok(CloudSyncDevicePollView {
                status: CloudSyncDevicePollStatus::Expired,
                interval_secs: None,
                error: Some("GitHub verification code expired — please start again.".into()),
                sync_status: None,
            }),
            DevicePollOutcome::Denied => Ok(CloudSyncDevicePollView {
                status: CloudSyncDevicePollStatus::Denied,
                interval_secs: None,
                error: Some("GitHub authorization was cancelled.".into()),
                sync_status: None,
            }),
            DevicePollOutcome::Authorized { access_token } => {
                let status = self.finalize_github_login_blocking(&access_token)?;
                Ok(CloudSyncDevicePollView {
                    status: CloudSyncDevicePollStatus::Authorized,
                    interval_secs: None,
                    error: None,
                    sync_status: Some(status),
                })
            }
        }
    }

    /// Sign in directly using a GitHub Personal Access Token (`gist` scope).
    pub fn cloud_sync_login_with_token(
        &self,
        token: &str,
    ) -> Result<CloudSyncStatusView, ClientCoreError> {
        self.finalize_github_login_blocking(token)
    }

    fn finalize_github_login_blocking(
        &self,
        token: &str,
    ) -> Result<CloudSyncStatusView, ClientCoreError> {
        let client = self.cloud_client();
        let existing_cfg = load_sync_config_from_store(&self.store);
        let (user, remote_gist) = self.runtime.block_on(async {
            let user = client.verify_token(token).await?;
            let gist = client
                .fetch_vault_gist(token, existing_cfg.gist_id.as_deref())
                .await?;
            Ok::<_, CloudSyncError>((user, gist))
        })?;

        let mut cfg = existing_cfg;
        let account_changed = cfg.github_login.as_deref() != Some(user.login.as_str());
        if account_changed {
            cfg.clear_derived_key();
            cfg.gist_id = None;
            cfg.gist_html_url = None;
            cfg.last_synced_at_ms = None;
            cfg.last_vault_revision = 0;
        }
        cfg.github_access_token = Some(token.trim().to_string());
        cfg.github_login = Some(user.login);
        cfg.github_name = user.name;
        cfg.github_avatar_url = user.avatar_url;
        cfg.last_error = None;
        if let Some(gist) = remote_gist {
            // If cached key doesn't match remote envelope salt, require passphrase entry
            if let Some(cached) = cfg.derived_key()
                && !cached.matches_envelope(&gist.envelope)
            {
                cfg.clear_derived_key();
            }
            cfg.gist_id = Some(gist.gist_id);
            cfg.gist_html_url = Some(gist.html_url);
            cfg.remote_vault_exists = true;
        } else {
            cfg.remote_vault_exists = false;
        }
        save_sync_config_to_store(&self.store, &cfg)?;
        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(cfg.to_status_view(false))
    }

    /// Unlock (or initialize) the E2EE Cloud Vault with the user's Sync Passphrase and perform an immediate sync.
    pub fn cloud_sync_unlock_with_passphrase(
        self: &Arc<Self>,
        passphrase: &str,
    ) -> Result<CloudSyncResultView, ClientCoreError> {
        let core = Arc::clone(self);
        let pass = passphrase.to_string();
        self.runtime
            .block_on(core.cloud_sync_unlock_async(&pass))
            .map_err(Into::into)
    }

    /// Run a full bidirectional pull -> LWW merge -> push Cloud Sync cycle now.
    pub fn cloud_sync_now(self: &Arc<Self>) -> Result<CloudSyncResultView, ClientCoreError> {
        let core = Arc::clone(self);
        self.runtime
            .block_on(core.cloud_sync_now_async())
            .map_err(Into::into)
    }

    /// Trigger a non-blocking background Cloud Sync (used after local mutations or foreground resume).
    pub fn schedule_cloud_auto_sync(self: &Arc<Self>, throttled: bool) {
        let cfg = load_sync_config_from_store(&self.store);
        if !cfg.auto_sync_enabled || !cfg.is_signed_in() || cfg.derived_key().is_none() {
            return;
        }
        let now = unix_ms_now();
        if throttled {
            let last = self.cloud_sync_last_auto_ms.load(Ordering::SeqCst);
            if now.saturating_sub(last) < 30_000 {
                return;
            }
        }
        self.cloud_sync_last_auto_ms.store(now, Ordering::SeqCst);
        let core = Arc::clone(self);
        self.runtime.spawn(async move {
            if !throttled {
                tokio::time::sleep(Duration::from_millis(1200)).await;
            }
            if let Err(err) = core.cloud_sync_now_async().await {
                tracing::debug!(error = %err, "Background cloud auto-sync skipped/failed");
            }
        });
    }

    /// Sign out of Cloud Sync on this device (leaves remote GitHub Gist untouched).
    pub fn cloud_sync_logout(
        &self,
        clear_local_hosts: bool,
    ) -> Result<CloudSyncStatusView, ClientCoreError> {
        let prev = load_sync_config_from_store(&self.store);
        let cleared = PersistedCloudSyncConfig {
            github_client_id: prev.github_client_id,
            auto_sync_enabled: prev.auto_sync_enabled,
            ..PersistedCloudSyncConfig::default()
        };
        save_sync_config_to_store(&self.store, &cleared)?;

        if clear_local_hosts {
            let host_ids: Vec<String> = self
                .hosts
                .read()
                .map(|g| g.keys().cloned().collect())
                .unwrap_or_default();
            for hid in host_ids {
                let _ = self.store.delete_client_host(&hid);
            }
            if let Ok(mut guard) = self.hosts.write() {
                for (_, mut h) in guard.drain() {
                    fail_pending_host_requests(&mut h, "Signed out of Cloud Sync");
                }
            }
            if let Ok(mut projs) = self.projects.write() {
                projs.clear();
            }
        }

        self.emit(|revision| CoreNotification::FleetChanged { revision });
        Ok(cleared.to_status_view(false))
    }

    /// Return the current merged cross-device user preferences (`key -> JSON value`).
    pub fn cloud_sync_get_preferences(&self) -> HashMap<String, serde_json::Value> {
        load_sync_prefs_from_store(&self.store)
            .into_iter()
            .map(|(k, entry)| (k, entry.value))
            .collect()
    }

    /// Update one or more cross-device user preferences locally and schedule an auto-sync push.
    pub fn cloud_sync_update_preferences(
        self: &Arc<Self>,
        patch: HashMap<String, serde_json::Value>,
    ) -> Result<(), ClientCoreError> {
        if patch.is_empty() {
            return Ok(());
        }
        let now_ms = now_unix_ms();
        let mut prefs = load_sync_prefs_from_store(&self.store);
        let mut changed = false;
        for (k, v) in patch {
            let unchanged = prefs.get(&k).is_some_and(|existing| existing.value == v);
            if !unchanged {
                prefs.insert(
                    k,
                    SyncedPreferenceEntry {
                        value: v,
                        updated_at_ms: now_ms,
                    },
                );
                changed = true;
            }
        }
        if changed {
            save_sync_prefs_to_store(&self.store, &prefs)?;
            self.schedule_cloud_auto_sync(false);
        }
        Ok(())
    }

    async fn cloud_sync_unlock_async(
        self: &Arc<Self>,
        passphrase: &str,
    ) -> Result<CloudSyncResultView, CloudSyncError> {
        let _guard = self.cloud_sync_mutex.lock().await;
        self.cloud_sync_in_flight.store(true, Ordering::SeqCst);
        let res = self.cloud_sync_unlock_inner(passphrase).await;
        self.cloud_sync_in_flight.store(false, Ordering::SeqCst);
        if let Err(ref err) = res {
            let mut cfg = load_sync_config_from_store(&self.store);
            cfg.last_error = Some(err.to_string());
            let _ = save_sync_config_to_store(&self.store, &cfg);
        }
        res
    }

    async fn cloud_sync_unlock_inner(
        self: &Arc<Self>,
        passphrase: &str,
    ) -> Result<CloudSyncResultView, CloudSyncError> {
        let mut cfg = load_sync_config_from_store(&self.store);
        let token = cfg
            .github_access_token
            .clone()
            .filter(|s| !s.trim().is_empty())
            .ok_or(CloudSyncError::NotSignedIn)?;

        let client = self.cloud_client();
        let (key, remote_gist, remote_payload) = unlock_remote_vault_with_passphrase(
            &client,
            &token,
            cfg.gist_id.as_deref(),
            passphrase,
        )
        .await?;

        cfg.set_derived_key(&key);
        if let Some(ref gist) = remote_gist {
            cfg.gist_id = Some(gist.gist_id.clone());
            cfg.gist_html_url = Some(gist.html_url.clone());
            cfg.remote_vault_exists = true;
        }
        cfg.last_error = None;
        save_sync_config_to_store(&self.store, &cfg)?;

        self.merge_apply_and_push_locked(&client, &token, &key, &mut cfg, remote_payload.as_ref())
            .await
    }

    async fn cloud_sync_now_async(self: &Arc<Self>) -> Result<CloudSyncResultView, CloudSyncError> {
        let _guard = self.cloud_sync_mutex.lock().await;
        self.cloud_sync_in_flight.store(true, Ordering::SeqCst);
        let res = self.cloud_sync_now_inner().await;
        self.cloud_sync_in_flight.store(false, Ordering::SeqCst);
        if let Err(ref err) = res {
            let mut cfg = load_sync_config_from_store(&self.store);
            if matches!(err, CloudSyncError::PassphraseRotationRequired) {
                cfg.clear_derived_key();
            }
            cfg.last_error = Some(err.to_string());
            let _ = save_sync_config_to_store(&self.store, &cfg);
            self.emit(|revision| CoreNotification::FleetChanged { revision });
        }
        res
    }

    async fn cloud_sync_now_inner(self: &Arc<Self>) -> Result<CloudSyncResultView, CloudSyncError> {
        let mut cfg = load_sync_config_from_store(&self.store);
        let token = cfg
            .github_access_token
            .clone()
            .filter(|s| !s.trim().is_empty())
            .ok_or(CloudSyncError::NotSignedIn)?;
        let key = cfg.derived_key().ok_or(CloudSyncError::VaultLocked)?;

        let client = self.cloud_client();
        let remote_gist = client
            .fetch_vault_gist(&token, cfg.gist_id.as_deref())
            .await?;

        let remote_payload = if let Some(ref gist) = remote_gist {
            cfg.gist_id = Some(gist.gist_id.clone());
            cfg.gist_html_url = Some(gist.html_url.clone());
            cfg.remote_vault_exists = true;
            Some(decrypt_remote_with_cached_key(&key, gist)?)
        } else {
            cfg.remote_vault_exists = false;
            None
        };

        self.merge_apply_and_push_locked(&client, &token, &key, &mut cfg, remote_payload.as_ref())
            .await
    }

    async fn merge_apply_and_push_locked(
        self: &Arc<Self>,
        client: &GitHubCloudClient,
        token: &str,
        key: &orbit_crypto::DerivedVaultKey,
        cfg: &mut PersistedCloudSyncConfig,
        remote_payload: Option<&crate::cloud_sync::CloudVaultPayload>,
    ) -> Result<CloudSyncResultView, CloudSyncError> {
        let now_ms = now_unix_ms();
        let mut local_meta = load_sync_meta_from_store(&self.store);
        let mut local_prefs = load_sync_prefs_from_store(&self.store);

        // Authoritatively prefer this machine's persisted `$ORBIT_HOME/pairing-secret` over any
        // cached or remote synced value before merging so rotated secrets never roll back.
        // Resolve identity and pairing-secret from the EXACT same Orbit home directory.
        let (local_daemon_id, local_daemon_secret) = read_local_host_identity_and_secret();
        let mut refreshed_local_stored: Option<StoredHostProfile> = None;
        if let (Some((local_hid, local_pub)), Some(local_secret)) =
            (local_daemon_id.as_ref(), local_daemon_secret.as_ref())
            && let Ok(mut guard) = self.hosts.write()
            && let Some(h) = guard.get_mut(local_hid)
            && h.profile.daemon_pubkey_b64 == *local_pub
        {
            let remote_secret_ts = remote_payload
                .and_then(|r| r.hosts.get(local_hid))
                .map(|r| r.pairing_secret_updated_at_ms)
                .unwrap_or(0);
            let remote_secret_differs = remote_payload
                .and_then(|r| r.hosts.get(local_hid))
                .is_some_and(|r| r.pairing_secret != *local_secret);
            if h.profile.pairing_secret != *local_secret
                || (remote_secret_differs
                    && h.profile.pairing_secret_updated_at_ms <= remote_secret_ts)
            {
                let fresh_ts = now_ms
                    .max(h.profile.pairing_secret_updated_at_ms.saturating_add(1))
                    .max(remote_secret_ts.saturating_add(1));
                h.profile.pairing_secret = local_secret.clone();
                h.profile.pairing_secret_updated_at_ms = fresh_ts;
                h.is_local = true;
                refreshed_local_stored = Some(h.to_stored_host());
            }
        }
        if let Some(ref stored) = refreshed_local_stored {
            let _ = self.store.upsert_client_host(stored);
        }

        // Snapshot local hosts, projects, and project order without holding locks across `.await`
        let fallback_host_ts = if remote_payload.is_none() { now_ms } else { 1 };
        let local_hosts: Vec<LocalHostSyncRecord> = self
            .hosts
            .read()
            .map(|guard| {
                guard
                    .values()
                    .map(|h| {
                        let ts = local_meta
                            .host_updated_at_ms
                            .get(&h.profile.host_id)
                            .copied()
                            .unwrap_or(fallback_host_ts);
                        LocalHostSyncRecord {
                            profile: h.to_stored_host(),
                            os_arch: h.os_arch.clone(),
                            updated_at_ms: ts,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let local_projects: Vec<StoredClientProject> = self
            .projects
            .read()
            .map(|guard| {
                guard
                    .values()
                    .map(|p| StoredClientProject {
                        project_id: p.project_id.clone(),
                        host_id: p.host_id.clone(),
                        name: p.name.clone(),
                        directories: p.directories.clone(),
                        updated_at_ms: p.updated_at_ms,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let local_project_order = self.project_order();

        let mut outcome = merge_cloud_vault(
            remote_payload,
            &local_hosts,
            &local_projects,
            &local_project_order,
            &local_prefs,
            &local_meta,
            now_ms,
        );

        // Apply remote host changes locally
        let mut hosts_to_connect: Vec<String> = Vec::new();
        for synced in &outcome.hosts_to_upsert {
            let mut stored = stored_host_from_synced(synced);
            let is_this_machine = local_daemon_id.as_ref().is_some_and(|(lid, lpub)| {
                *lid == stored.host_id && *lpub == stored.daemon_pubkey_b64
            });
            let remote_port = stored
                .meta
                .remote_daemon_port
                .or_else(|| extract_ws_port(&stored.direct_endpoints))
                .unwrap_or(6868);
            if is_this_machine {
                let loopback = format!("ws://127.0.0.1:{remote_port}/ws");
                if !stored.direct_endpoints.contains(&loopback) {
                    stored.direct_endpoints.insert(0, loopback);
                }
                if let Some(ref local_secret) = local_daemon_secret
                    && stored.pairing_secret != *local_secret
                {
                    let fresh_ts =
                        now_ms.max(stored.meta.pairing_secret_updated_at_ms.saturating_add(1));
                    stored.pairing_secret = local_secret.clone();
                    stored.meta.pairing_secret_updated_at_ms = fresh_ts;
                    if let Some(merged_entry) = outcome.merged.hosts.get_mut(&stored.host_id) {
                        merged_entry.pairing_secret = local_secret.clone();
                        merged_entry.pairing_secret_updated_at_ms = fresh_ts;
                        outcome.needs_push = true;
                    }
                }
            }
            let _ = self.store.upsert_client_host(&stored);
            local_meta.record_host_mutated(&stored.host_id, synced.updated_at_ms);

            if let Ok(mut guard) = self.hosts.write() {
                let profile = SavedHostProfile {
                    host_id: stored.host_id.clone(),
                    hostname: stored.display_name.clone(),
                    custom_name: stored.meta.custom_name.clone(),
                    ssh_alias: stored.meta.ssh_alias.clone(),
                    route_mode: HostRouteMode::from_opt_str(stored.meta.route_mode.as_deref()),
                    remote_daemon_port: remote_port,
                    custom_direct_endpoint: stored.meta.custom_direct_endpoint.clone(),
                    daemon_pubkey_b64: stored.daemon_pubkey_b64.clone(),
                    direct_endpoints: stored.direct_endpoints.clone(),
                    relay_url: stored.relay_url.clone(),
                    pairing_secret: stored.pairing_secret.clone(),
                    pairing_secret_updated_at_ms: stored.meta.pairing_secret_updated_at_ms,
                    state: HostConnectionState::Disconnected,
                };
                if let Some(existing) = guard.get_mut(&stored.host_id) {
                    let prev_state = existing.profile.state;
                    existing.profile = profile;
                    existing.profile.state = prev_state;
                    if is_this_machine {
                        existing.is_local = true;
                    }
                    if existing.session.is_none() {
                        hosts_to_connect.push(stored.host_id.clone());
                    }
                } else {
                    let mut mem = HostMemoryState::new(profile);
                    mem.is_local = is_this_machine;
                    if let Some(ref oa) = synced.os_arch {
                        mem.os_arch = Some(oa.clone());
                    }
                    guard.insert(stored.host_id.clone(), mem);
                    hosts_to_connect.push(stored.host_id.clone());
                }
            }
        }

        for removed_hid in &outcome.hosts_to_remove {
            let _ = self.store.delete_client_host(removed_hid);
            let del_ts = outcome
                .merged
                .hosts
                .get(removed_hid)
                .and_then(|h| h.deleted_at_ms)
                .unwrap_or(now_ms);
            local_meta.record_host_deleted(removed_hid, del_ts);
            if let Ok(mut guard) = self.hosts.write()
                && let Some(mut removed) = guard.remove(removed_hid)
            {
                fail_pending_host_requests(&mut removed, "Host removed via Cloud Sync");
            }
            if let Ok(mut projs) = self.projects.write() {
                projs.retain(|_, p| p.host_id != *removed_hid);
            }
        }

        // Apply remote project changes locally
        for proj in &outcome.projects_to_upsert {
            let stored = StoredClientProject {
                project_id: proj.project_id.clone(),
                host_id: proj.host_id.clone(),
                name: proj.name.clone(),
                directories: proj.directories.clone(),
                updated_at_ms: proj.updated_at_ms,
            };
            let _ = self.store.upsert_client_project(&stored);
            local_meta.record_project_mutated(&proj.project_id);
            if let Ok(mut guard) = self.projects.write() {
                guard.insert(
                    proj.project_id.clone(),
                    ClientProjectView {
                        project_id: proj.project_id.clone(),
                        host_id: proj.host_id.clone(),
                        name: proj.name.clone(),
                        directories: proj.directories.clone(),
                        updated_at_ms: proj.updated_at_ms,
                    },
                );
            }
        }
        for removed_pid in &outcome.projects_to_remove {
            let _ = self.store.delete_client_project(removed_pid);
            let del_ts = outcome
                .merged
                .projects
                .get(removed_pid)
                .and_then(|p| p.deleted_at_ms)
                .unwrap_or(now_ms);
            local_meta.record_project_deleted(removed_pid, del_ts);
            if let Ok(mut guard) = self.projects.write() {
                guard.remove(removed_pid);
            }
        }

        // Apply merged project order if remote won
        if let Some((ref new_order, order_ts)) = outcome.project_order_changed {
            if let Ok(json) = serde_json::to_string(new_order) {
                let _ = self.store.set_client_kv("project_order", &json, order_ts);
            }
            if let Ok(mut guard) = self.project_order.write() {
                *guard = new_order.clone();
            }
            local_meta.record_project_order_mutated(order_ts);
        }

        // Apply merged preferences if remote won
        if !outcome.preferences_to_apply.is_empty() {
            for (k, entry) in &outcome.preferences_to_apply {
                local_prefs.insert(k.clone(), entry.clone());
            }
            let _ = save_sync_prefs_to_store(&self.store, &local_prefs);
        }

        // Record authoritative merged host timestamps so local-only pre-sync hosts stay monotonic
        for (hid, entry) in &outcome.merged.hosts {
            if entry.deleted_at_ms.is_none() {
                local_meta
                    .host_updated_at_ms
                    .entry(hid.clone())
                    .or_insert(entry.updated_at_ms);
            }
        }
        let _ = save_sync_meta_to_store(&self.store, &local_meta);

        // Push encrypted vault to GitHub Secret Gist if local state contributed new changes
        if outcome.needs_push {
            let (gid, html_url) =
                push_vault_payload(client, token, cfg.gist_id.as_deref(), key, &outcome.merged)
                    .await?;
            cfg.gist_id = Some(gid);
            cfg.gist_html_url = Some(html_url);
            cfg.remote_vault_exists = true;
        }

        let active_hosts = outcome
            .merged
            .hosts
            .values()
            .filter(|h| h.deleted_at_ms.is_none())
            .count() as u32;
        let active_projects = outcome
            .merged
            .projects
            .values()
            .filter(|p| p.deleted_at_ms.is_none())
            .count() as u32;

        cfg.last_synced_at_ms = Some(now_ms);
        cfg.last_vault_revision = outcome.merged.revision;
        cfg.synced_host_count = active_hosts;
        cfg.synced_project_count = active_projects;
        cfg.last_error = None;
        save_sync_config_to_store(&self.store, cfg)?;

        for hid in hosts_to_connect {
            self.connect_host_async(hid);
        }
        self.emit(|revision| CoreNotification::FleetChanged { revision });

        Ok(CloudSyncResultView {
            hosts_added: outcome.hosts_added_count,
            hosts_updated: outcome.hosts_updated_count,
            hosts_removed: outcome.hosts_removed_count,
            projects_added: outcome.projects_added_count,
            projects_updated: outcome.projects_updated_count,
            projects_removed: outcome.projects_removed_count,
            preferences_updated: outcome.preferences_updated_count,
            pushed_remote: outcome.needs_push,
            vault_revision: outcome.merged.revision,
            synced_at_ms: now_ms,
            gist_html_url: cfg.gist_html_url.clone(),
        })
    }
}

fn read_local_host_identity_pair_in(orbit_dir: &Path) -> Option<(String, String)> {
    let raw = std::fs::read_to_string(orbit_dir.join("host-keypair.json")).ok()?;
    let kp: orbit_crypto::HostIdentityKeypair = serde_json::from_str(&raw).ok()?;
    let host_id = kp.host_id().ok()?;
    Some((host_id, kp.public_key_b64))
}

fn read_local_host_identity_and_secret() -> (Option<(String, String)>, Option<String>) {
    let Some(orbit_dir) = crate::desktop_ops::resolve_orbit_home_dir() else {
        return (None, None);
    };
    let identity = read_local_host_identity_pair_in(&orbit_dir);
    let secret = crate::desktop_ops::read_local_daemon_pairing_secret_in(&orbit_dir);
    (identity, secret)
}

fn build_workspace_file_view(
    host_id: &str,
    cwd: &str,
    path: &str,
    raw: RawFileReadResponse,
) -> WorkspaceFileView {
    let language = detect_language_for_path(path).to_string();
    let (highlighted_lines, markdown_blocks) = if raw.content_kind == "text" {
        if language == "markdown" {
            // Markdown opens in Preview mode by default; skip expensive upfront 2,500-line
            // `syntect` source highlighting so large Markdown artifacts open instantaneously.
            // Source view falls back cleanly to raw content and lazily highlights on demand.
            let md = parse_markdown_blocks(&raw.content, false);
            (Vec::new(), md)
        } else {
            // Highlight up to 600 lines eagerly for sub-15ms first paint; FileViewerTab virtualizes
            // all lines and lazily highlights deeper ranges via `highlight_code_snippet` when scrolled.
            let lines = highlight_code_lines_capped(&language, &raw.content, 600);
            (lines, Vec::new())
        }
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
        etag: raw.etag,
        mtime_unix_ms: raw.mtime_unix_ms,
        writable: raw.writable,
        line_ending: raw.line_ending,
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

fn reconcile_pending_queue_ops(state: &mut HostMemoryState, agent: &mut AgentSummary) {
    let ag_id = agent.agent_id.as_str();
    if let Some(adds) = state.pending_queue_adds.get_mut(ag_id) {
        adds.retain(|(_, qv)| {
            !agent
                .queued_prompts
                .iter()
                .any(|q| q.queue_id == qv.queue_id)
        });
        for (_, qv) in adds.iter() {
            agent.queued_prompts.push(qv.clone());
        }
        if adds.is_empty() {
            state.pending_queue_adds.remove(ag_id);
        }
    }
    if let Some(removals) = state.pending_queue_removals.get_mut(ag_id) {
        removals.retain(|(_, qid)| agent.queued_prompts.iter().any(|q| q.queue_id == *qid));
        for (_, qid) in removals.iter() {
            agent.queued_prompts.retain(|q| q.queue_id != *qid);
        }
        if removals.is_empty() {
            state.pending_queue_removals.remove(ag_id);
        }
    }
    if let Some((_, desired_ids)) = state.pending_queue_reorders.get(ag_id) {
        let matches_desired = {
            let mut desired_iter = desired_ids
                .iter()
                .filter(|id| agent.queued_prompts.iter().any(|q| q.queue_id == **id));
            agent
                .queued_prompts
                .iter()
                .all(|q| desired_iter.next().is_none_or(|want| q.queue_id == *want))
        };
        if matches_desired {
            state.pending_queue_reorders.remove(ag_id);
        } else if agent.queued_prompts.len() >= 2 {
            let desired = desired_ids.clone();
            let mut by_id: HashMap<String, QueuedPromptView> = agent
                .queued_prompts
                .drain(..)
                .map(|q| (q.queue_id.clone(), q))
                .collect();
            let mut reordered = Vec::with_capacity(by_id.len());
            for qid in &desired {
                if let Some(item) = by_id.remove(qid) {
                    reordered.push(item);
                }
            }
            let mut remaining: Vec<_> = by_id.into_values().collect();
            remaining.sort_by_key(|q| q.created_at);
            reordered.extend(remaining);
            agent.queued_prompts = reordered;
        }
    }
}

fn clear_pending_queue_op_by_req(
    state: &mut HostMemoryState,
    req_id: Uuid,
    rollback_failed_add: bool,
) -> bool {
    let mut changed = false;
    let mut empty_add_keys = Vec::new();
    for (ag_id, list) in state.pending_queue_adds.iter_mut() {
        let mut rolled_back_qids = Vec::new();
        list.retain(|(rid, qv)| {
            if *rid == req_id {
                if rollback_failed_add {
                    rolled_back_qids.push(qv.queue_id.clone());
                }
                false
            } else {
                true
            }
        });
        if !rolled_back_qids.is_empty()
            && let Some(ag) = state.agents.get_mut(ag_id)
        {
            ag.queued_prompts
                .retain(|q| !rolled_back_qids.iter().any(|id| id == &q.queue_id));
            changed = true;
        }
        if list.is_empty() {
            empty_add_keys.push(ag_id.clone());
        }
    }
    for k in empty_add_keys {
        state.pending_queue_adds.remove(&k);
    }

    let mut empty_rem_keys = Vec::new();
    for (ag_id, list) in state.pending_queue_removals.iter_mut() {
        list.retain(|(rid, _)| *rid != req_id);
        if list.is_empty() {
            empty_rem_keys.push(ag_id.clone());
        }
    }
    for k in empty_rem_keys {
        state.pending_queue_removals.remove(&k);
    }
    state
        .pending_queue_reorders
        .retain(|_, (rid, _)| *rid != req_id);
    changed
}

fn upsert_cached_artifact(list: &mut Vec<ArtifactSummary>, artifact: ArtifactSummary) {
    if let Some(slot) = list
        .iter_mut()
        .find(|existing| existing.artifact_id == artifact.artifact_id)
    {
        *slot = artifact;
    } else {
        list.insert(0, artifact);
    }
    list.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.artifact_id.cmp(&b.artifact_id))
    });
}

fn upsert_cached_goal(list: &mut Vec<GoalRunRecord>, goal: GoalRunRecord) {
    if let Some(slot) = list
        .iter_mut()
        .find(|existing| existing.goal_id == goal.goal_id)
    {
        *slot = goal;
    } else {
        list.insert(0, goal);
    }
    list.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| b.goal_id.cmp(&a.goal_id))
    });
}

fn upsert_cached_monitor(list: &mut Vec<MonitorWatchRecord>, monitor: MonitorWatchRecord) {
    if let Some(slot) = list
        .iter_mut()
        .find(|existing| existing.monitor_id == monitor.monitor_id)
    {
        *slot = monitor;
    } else {
        list.insert(0, monitor);
    }
    list.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| b.monitor_id.cmp(&a.monitor_id))
    });
}

fn upsert_cached_workflow_run(list: &mut Vec<WorkflowRunRecord>, run: WorkflowRunRecord) {
    if let Some(slot) = list
        .iter_mut()
        .find(|existing| existing.run_id == run.run_id)
    {
        *slot = run;
    } else {
        list.insert(0, run);
    }
    list.sort_by(|a, b| {
        b.updated_at_ms
            .cmp(&a.updated_at_ms)
            .then_with(|| b.run_id.cmp(&a.run_id))
    });
    list.truncate(100);
}

fn provider_subagent_logical_key(payload: &TimelinePayload) -> Option<(&'static str, String)> {
    match payload {
        TimelinePayload::AssistantThinking { item_id, .. } if !item_id.is_empty() => {
            Some(("thinking", item_id.clone()))
        }
        TimelinePayload::AssistantMessage { item_id, .. } if !item_id.is_empty() => {
            Some(("msg", item_id.clone()))
        }
        TimelinePayload::ToolCall { call_id, .. } if !call_id.is_empty() => {
            Some(("tool", call_id.clone()))
        }
        TimelinePayload::UserPrompt { message_id, .. } if !message_id.is_empty() => {
            Some(("user", message_id.clone()))
        }
        _ => None,
    }
}

fn collect_provider_subagent_resubscriptions(h: &HostMemoryState) -> Vec<(String, String, u64)> {
    let mut keys: Vec<(String, String)> = h.subscribed_provider_subagents.iter().cloned().collect();
    if let Some(ref active) = h.active_provider_subagent
        && !keys.contains(active)
    {
        keys.push(active.clone());
    }
    keys.sort();
    keys.into_iter()
        .map(|(parent_id, sub_id)| {
            let since_seq = h
                .provider_subagent_timelines
                .get(&(parent_id.clone(), sub_id.clone()))
                .and_then(|list| list.iter().map(|e| e.seq).max())
                .unwrap_or(0);
            (parent_id, sub_id, since_seq)
        })
        .collect()
}

fn fail_pending_host_requests(h: &mut HostMemoryState, reason: &str) {
    for (_, tx) in h.pending_attachment_acks.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_file_reads.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_symbol_searches.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_file_writes.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_file_mutations.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_instruction_lists.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_instruction_reads.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_instruction_saves.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_rewinds.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_forks.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_handoff_previews.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_handoff_executes.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_git_repo_inspections.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_acp_probes.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_acp_snapshots.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_mcp_snapshots.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_mcp_probes.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, (_, tx)) in h.pending_artifact_lists.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    h.unsolicited_unfiltered_artifact_reqs.clear();
    for (_, tx) in h.pending_artifact_reads.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_artifact_upserts.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_artifact_deletes.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, (_, tx)) in h.pending_managed_skills_lists.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    h.background_managed_skills_reqs.clear();
    for (_, tx) in h.pending_managed_skill_reads.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_path_rules.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_path_rule_matches.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    h.path_rules_by_cwd.clear();
    for (_, tx) in h.pending_lsp_servers.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_lsp_symbols.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_lsp_diagnostics.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_lsp_locations.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_usage_insights.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_context_occupancy.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_lifecycle_hooks.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_lifecycle_hook_tests.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_timer_creates.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_timer_lists.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_timer_updates.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    h.background_timer_list_reqs.clear();
    for (_, tx) in h.pending_guardian_configs.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_goal_updates.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_goal_lists.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_monitor_updates.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_event_triggers.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_event_trigger_tests.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_monitor_lists.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_browser_snapshots.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_browser_sessions.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_browser_actions.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_workflow_updates.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_workflow_lists.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_im_channels.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_im_channel_tests.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_push_snapshots.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (_, tx) in h.pending_push_tests.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
    for (req_id, tx) in h.pending_side_questions.drain() {
        let _ = tx.send(SideQuestionStreamEvent::Done {
            request_id: req_id.to_string(),
            full_text: String::new(),
            error: Some(reason.to_string()),
        });
    }
    // Dropping pending daemon-upgrade ack senders lets `upgrade_host_daemon_over_wire` observe
    // `RecvError` immediately when the remote daemon disconnects to hot-restart.
    h.pending_daemon_upgrades.clear();
    for (_, tx) in h.pending_upgrade_terminals.drain() {
        let _ = tx.send(Err(reason.to_string()));
    }
}

fn upsert_conditional_timer_record(
    timers: &mut Vec<ConditionalTimerRecord>,
    timer: ConditionalTimerRecord,
) {
    if let Some(slot) = timers.iter_mut().find(|t| t.timer_id == timer.timer_id) {
        *slot = timer;
    } else {
        timers.push(timer);
    }
    timers.sort_by(|a, b| match (a.state.is_armed(), b.state.is_armed()) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (true, true) => a
            .fire_at_ms
            .cmp(&b.fire_at_ms)
            .then_with(|| a.created_at_ms.cmp(&b.created_at_ms)),
        (false, false) => b
            .resolved_at_ms
            .unwrap_or(b.created_at_ms)
            .cmp(&a.resolved_at_ms.unwrap_or(a.created_at_ms)),
    });
    // Keep all armed timers plus at most 100 recent resolved timers in memory.
    let mut resolved_seen = 0usize;
    timers.retain(|t| {
        if t.state == ConditionalTimerState::Armed {
            true
        } else {
            resolved_seen += 1;
            resolved_seen <= 100
        }
    });
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

pub(crate) fn cwd_paths_equivalent(stored_cwd: &str, query_cwd: &str) -> bool {
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
    fleet: CoalesceSlot<CoreNotification>,
    hosts: HashMap<String, CoalesceSlot<CoreNotification>>,
    git: HashMap<(String, String), CoalesceSlot<CoreNotification>>,
    timelines: HashMap<(String, String), CoalesceSlot<CoreNotification>>,
    terminals: HashMap<(String, u8), CoalesceSlot<CoreNotification>>,
}

struct StripTerminalBytesSubscriber {
    downstream: Arc<dyn CoreEventSubscriber>,
}

impl CoreEventSubscriber for StripTerminalBytesSubscriber {
    fn on_notification(&self, mut event: CoreNotification) {
        if let CoreNotification::TerminalChanged {
            ref mut raw_bytes, ..
        } = event
        {
            raw_bytes.clear();
        }
        self.downstream.on_notification(event);
    }
}

struct CoalescingSubscriber {
    downstream: Arc<dyn CoreEventSubscriber>,
    runtime_handle: tokio::runtime::Handle,
    interval: Duration,
    include_terminal_raw_bytes: bool,
    state: Arc<Mutex<CoalescingState>>,
}

impl CoalescingSubscriber {
    fn new(
        downstream: Arc<dyn CoreEventSubscriber>,
        runtime_handle: tokio::runtime::Handle,
        interval: Duration,
        include_terminal_raw_bytes: bool,
    ) -> Self {
        Self {
            downstream,
            runtime_handle,
            interval,
            include_terminal_raw_bytes,
            state: Arc::new(Mutex::new(CoalescingState {
                fleet: CoalesceSlot::default(),
                hosts: HashMap::new(),
                git: HashMap::new(),
                timelines: HashMap::new(),
                terminals: HashMap::new(),
            })),
        }
    }

    fn meta_interval(&self) -> Duration {
        self.interval.saturating_mul(2)
    }
}

impl CoreEventSubscriber for CoalescingSubscriber {
    fn on_notification(&self, event: CoreNotification) {
        match event {
            CoreNotification::FleetChanged { .. } => {
                let meta_interval = self.meta_interval();
                let now = tokio::time::Instant::now();
                let mut emit_now = false;
                let mut schedule_after: Option<Duration> = None;

                if let Ok(mut guard) = self.state.lock() {
                    let slot = &mut guard.fleet;
                    let elapsed = slot.last_emit.map(|t| now.saturating_duration_since(t));
                    if !slot.timer_scheduled && elapsed.is_none_or(|d| d >= meta_interval) {
                        slot.last_emit = Some(now);
                        emit_now = true;
                    } else {
                        slot.pending = Some(event.clone());
                        if !slot.timer_scheduled {
                            slot.timer_scheduled = true;
                            let wait = elapsed
                                .map(|d| meta_interval.saturating_sub(d))
                                .unwrap_or(meta_interval);
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
                            g.fleet.timer_scheduled = false;
                            let ev = g.fleet.pending.take()?;
                            g.fleet.last_emit = Some(tokio::time::Instant::now());
                            Some(ev)
                        });
                        if let Some(ev) = ready {
                            downstream.on_notification(ev);
                        }
                    });
                }
            }
            CoreNotification::HostChanged { ref host_id, .. } => {
                let key = host_id.clone();
                let meta_interval = self.meta_interval();
                let now = tokio::time::Instant::now();
                let mut emit_now = false;
                let mut schedule_after: Option<Duration> = None;

                if let Ok(mut guard) = self.state.lock() {
                    let slot = guard.hosts.entry(key.clone()).or_default();
                    let elapsed = slot.last_emit.map(|t| now.saturating_duration_since(t));
                    if !slot.timer_scheduled && elapsed.is_none_or(|d| d >= meta_interval) {
                        slot.last_emit = Some(now);
                        emit_now = true;
                    } else {
                        slot.pending = Some(event.clone());
                        if !slot.timer_scheduled {
                            slot.timer_scheduled = true;
                            let wait = elapsed
                                .map(|d| meta_interval.saturating_sub(d))
                                .unwrap_or(meta_interval);
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
                            let slot = g.hosts.get_mut(&key)?;
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
            CoreNotification::GitChanged {
                ref host_id,
                ref cwd,
                ..
            } => {
                let key = (host_id.clone(), cwd.clone());
                let meta_interval = self.meta_interval();
                let now = tokio::time::Instant::now();
                let mut emit_now = false;
                let mut schedule_after: Option<Duration> = None;

                if let Ok(mut guard) = self.state.lock() {
                    let slot = guard.git.entry(key.clone()).or_default();
                    let elapsed = slot.last_emit.map(|t| now.saturating_duration_since(t));
                    if !slot.timer_scheduled && elapsed.is_none_or(|d| d >= meta_interval) {
                        slot.last_emit = Some(now);
                        emit_now = true;
                    } else {
                        slot.pending = Some(event.clone());
                        if !slot.timer_scheduled {
                            slot.timer_scheduled = true;
                            let wait = elapsed
                                .map(|d| meta_interval.saturating_sub(d))
                                .unwrap_or(meta_interval);
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
                            let slot = g.git.get_mut(&key)?;
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
                                if self.include_terminal_raw_bytes {
                                    const MAX_COALESCED_TERMINAL_BYTES: usize = 128 * 1024;
                                    prev_bytes.extend_from_slice(raw_bytes);
                                    if prev_bytes.len() > MAX_COALESCED_TERMINAL_BYTES {
                                        let excess =
                                            prev_bytes.len() - MAX_COALESCED_TERMINAL_BYTES;
                                        prev_bytes.drain(..excess);
                                    }
                                } else {
                                    prev_bytes.clear();
                                }
                            }
                            _ => {
                                slot.pending = Some(CoreNotification::TerminalChanged {
                                    revision,
                                    host_id: host_id.clone(),
                                    slot: term_slot,
                                    raw_bytes: if self.include_terminal_raw_bytes {
                                        raw_bytes.clone()
                                    } else {
                                        Vec::new()
                                    },
                                });
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
                    if self.include_terminal_raw_bytes {
                        self.downstream.on_notification(event);
                    } else {
                        self.downstream
                            .on_notification(CoreNotification::TerminalChanged {
                                revision,
                                host_id: host_id.clone(),
                                slot: term_slot,
                                raw_bytes: Vec::new(),
                            });
                    }
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
    let list = state.timelines.entry(agent_id.to_string()).or_default();
    let idx = list.len();
    list.push(TimelineEvent {
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
    state.mark_stage_dirty(agent_id, idx, false);
    state.mark_stage_meta_dirty(agent_id);
}

fn is_pending_optimistic_prompt(ev: &TimelineEvent) -> bool {
    ev.seq == 0 && matches!(ev.payload, TimelinePayload::UserPrompt { .. })
}

pub(crate) fn coalesce_timeline_slice(events: &[TimelineEvent]) -> Vec<TimelineEvent> {
    let mut coalesced = Vec::with_capacity(events.len());
    for ev in events {
        coalesce_or_push_timeline(&mut coalesced, ev.clone());
    }
    coalesced
}

/// Merge persisted replica events with live in-memory events without dropping unpersisted middle
/// sequence numbers or duplicating optimistic `seq == 0` user prompts already persisted on disk.
pub(crate) fn merge_persisted_and_memory_timeline(
    persisted: Vec<TimelineEvent>,
    mem_events: Vec<TimelineEvent>,
) -> Vec<TimelineEvent> {
    if persisted.is_empty() {
        return mem_events;
    }
    if mem_events.is_empty() {
        return persisted;
    }

    let max_epoch = persisted
        .iter()
        .chain(mem_events.iter())
        .filter(|e| e.seq > 0)
        .map(|e| e.epoch)
        .max()
        .unwrap_or(0);

    let mut positive: Vec<TimelineEvent> = Vec::with_capacity(persisted.len() + mem_events.len());
    let mut persisted_seqs: HashSet<u64> = HashSet::with_capacity(persisted.len());
    let mut persisted_prompt_ids: HashSet<String> = HashSet::new();
    let mut persisted_prompt_texts: HashSet<String> = HashSet::new();

    for ev in persisted {
        if ev.seq > 0 {
            if max_epoch > 0 && ev.epoch < max_epoch {
                continue;
            }
            persisted_seqs.insert(ev.seq);
            if let TimelinePayload::UserPrompt {
                message_id, text, ..
            } = &ev.payload
            {
                if !message_id.is_empty() {
                    persisted_prompt_ids.insert(message_id.clone());
                }
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    persisted_prompt_texts.insert(trimmed.to_string());
                }
            }
            positive.push(ev);
        }
    }

    let mut optimistic: Vec<TimelineEvent> = Vec::new();
    for ev in mem_events {
        if ev.seq > 0 {
            if max_epoch > 0 && ev.epoch < max_epoch {
                continue;
            }
            if persisted_seqs.insert(ev.seq) {
                if let TimelinePayload::UserPrompt {
                    message_id, text, ..
                } = &ev.payload
                {
                    if !message_id.is_empty() {
                        persisted_prompt_ids.insert(message_id.clone());
                    }
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        persisted_prompt_texts.insert(trimmed.to_string());
                    }
                }
                positive.push(ev);
            }
        } else {
            let is_dup_prompt = matches!(
                &ev.payload,
                TimelinePayload::UserPrompt { message_id, text, .. }
                    if (!message_id.is_empty() && persisted_prompt_ids.contains(message_id))
                        || (!text.trim().is_empty() && persisted_prompt_texts.contains(text.trim()))
            );
            if !is_dup_prompt {
                optimistic.push(ev);
            }
        }
    }

    positive.sort_by_key(|e| e.seq);
    positive.extend(optimistic);
    positive
}

fn coalesce_or_push_timeline(list: &mut Vec<TimelineEvent>, mut incoming: TimelineEvent) -> usize {
    let active_end = list
        .iter()
        .position(is_pending_optimistic_prompt)
        .unwrap_or(list.len());
    let precedes_incoming =
        |ev: &TimelineEvent| -> bool { ev.seq > 0 && (incoming.seq == 0 || ev.seq < incoming.seq) };

    let turn_start_idx = list[..active_end]
        .iter()
        .rposition(|ev| {
            precedes_incoming(ev)
                && matches!(
                    ev.payload,
                    TimelinePayload::UserPrompt { .. } | TimelinePayload::AgentMessage { .. }
                )
        })
        .map(|i| i + 1)
        .unwrap_or(0);

    let seg_start_idx = list[..active_end]
        .iter()
        .rposition(|ev| {
            precedes_incoming(ev)
                && matches!(
                    ev.payload,
                    TimelinePayload::UserPrompt { .. }
                        | TimelinePayload::AgentMessage { .. }
                        | TimelinePayload::ToolCall { .. }
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
                // Replace optimistic `seq == 0` UserPrompt with matching message_id first, then text
                if !inc_mid.is_empty()
                    && let Some((idx, opt_slot)) = list.iter_mut().enumerate().find(|(_, ev)| {
                        ev.seq == 0
                            && matches!(
                                &ev.payload,
                                TimelinePayload::UserPrompt { message_id, .. } if message_id == inc_mid
                            )
                    })
                {
                    *opt_slot = incoming;
                    return idx;
                }
                if let Some((idx, opt_slot)) = list.iter_mut().enumerate().find(|(_, ev)| {
                    ev.seq == 0
                        && matches!(
                            &ev.payload,
                            TimelinePayload::UserPrompt { text, .. } if text.trim() == inc_text.trim()
                        )
                }) {
                    *opt_slot = incoming;
                    return idx;
                }
            }
            if !inc_mid.is_empty()
                && let Some((idx, dup_slot)) = list.iter_mut().enumerate().find(|(_, ev)| {
                    matches!(
                        &ev.payload,
                        TimelinePayload::UserPrompt { message_id, .. } if message_id == inc_mid
                    )
                })
            {
                *dup_slot = incoming;
                return idx;
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

            if let Some((rel_idx, prev)) = list[seg_start_idx..active_end]
                .iter_mut()
                .enumerate()
                .rev()
                .find(|(_, prev_ev)| {
                    matches!(
                        &prev_ev.payload,
                        TimelinePayload::AssistantMessage { item_id: existing_id, .. }
                            if existing_id == &effective_id
                    )
                })
            {
                *prev = incoming;
                return seg_start_idx + rel_idx;
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

            if let Some((rel_idx, prev)) = list[seg_start_idx..active_end]
                .iter_mut()
                .enumerate()
                .rev()
                .find(|(_, prev_ev)| {
                    matches!(
                        &prev_ev.payload,
                        TimelinePayload::AssistantThinking { item_id: existing_id, .. }
                            if existing_id == &effective_id
                    )
                })
            {
                *prev = incoming;
                return seg_start_idx + rel_idx;
            }
        }
        TimelinePayload::ToolCall { call_id, .. } if !call_id.is_empty() => {
            let target_call_id = call_id.clone();
            if let Some((rel_idx, prev)) = list[turn_start_idx..active_end]
                .iter_mut()
                .enumerate()
                .rev()
                .find(|(_, prev_ev)| {
                    matches!(
                        &prev_ev.payload,
                        TimelinePayload::ToolCall { call_id: existing_id, .. }
                            if existing_id == &target_call_id
                    )
                })
            {
                *prev = incoming;
                return turn_start_idx + rel_idx;
            }
        }
        _ => {}
    }

    if incoming.seq > 0 {
        if let Some((idx, same_seq)) = list
            .iter_mut()
            .enumerate()
            .find(|(_, e)| e.seq == incoming.seq)
        {
            *same_seq = incoming;
            return idx;
        }
        if let Some(first_opt_idx) = list.iter().position(is_pending_optimistic_prompt) {
            list.insert(first_opt_idx, incoming);
            return first_opt_idx;
        }
    }
    list.push(incoming);
    list.len() - 1
}

fn contains_ascii_case_insensitive_str(haystack: &str, needle: &str) -> bool {
    let nb = needle.as_bytes();
    if nb.is_empty() {
        return true;
    }
    haystack
        .as_bytes()
        .windows(nb.len())
        .any(|w| w.eq_ignore_ascii_case(nb))
}

fn payload_affects_auxiliary_state(payload: &TimelinePayload) -> bool {
    match payload {
        TimelinePayload::AssistantThinking { .. } => false,
        TimelinePayload::AssistantMessage { text, .. } => {
            (contains_ascii_case_insensitive_str(text, "http://")
                || contains_ascii_case_insensitive_str(text, "https://"))
                && (contains_ascii_case_insensitive_str(text, "localhost")
                    || contains_ascii_case_insensitive_str(text, "127.0.0.1")
                    || contains_ascii_case_insensitive_str(text, "0.0.0.0")
                    || contains_ascii_case_insensitive_str(text, "[::1]"))
        }
        _ => true,
    }
}

fn dirty_index_for_event(list: &[TimelineEvent], idx: usize, payload: &TimelinePayload) -> usize {
    match payload {
        TimelinePayload::PermissionResolved { request_id, .. } => list
            .iter()
            .position(|e| {
                matches!(
                    &e.payload,
                    TimelinePayload::PermissionRequest { request_id: rid, .. }
                        if rid == request_id
                )
            })
            .map_or(idx, |found| found.min(idx)),
        TimelinePayload::QuestionResolved { request_id, .. } => list
            .iter()
            .position(|e| {
                matches!(
                    &e.payload,
                    TimelinePayload::QuestionRequest { request_id: rid, .. }
                        if rid == request_id
                )
            })
            .map_or(idx, |found| found.min(idx)),
        _ => idx,
    }
}

type ResolvedGatesMap = (HashMap<String, String>, HashMap<String, Vec<Vec<String>>>);

fn collect_resolved_gates(events: &[TimelineEvent]) -> ResolvedGatesMap {
    let mut all_resolved_perms: HashMap<String, String> = HashMap::new();
    let mut all_resolved_questions: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    for ev in events {
        match &ev.payload {
            TimelinePayload::PermissionResolved {
                request_id,
                decision,
            } => {
                all_resolved_perms.insert(request_id.clone(), decision.clone());
            }
            TimelinePayload::QuestionResolved {
                request_id,
                answers,
            } => {
                all_resolved_questions.insert(request_id.clone(), answers.clone());
            }
            _ => {}
        }
    }
    (all_resolved_perms, all_resolved_questions)
}

fn render_timeline_event_view(
    agent_id: &str,
    agent_cwd: Option<&str>,
    ev: &TimelineEvent,
    all_resolved_perms: &HashMap<String, String>,
    all_resolved_questions: &HashMap<String, Vec<Vec<String>>>,
    markdown_cache: &mut HashMap<(String, u64, usize), Vec<RichBlockView>>,
    streaming_markdown_cache: &mut HashMap<(String, String), (usize, Vec<RichBlockView>)>,
) -> Option<TimelineItemView> {
    let ts_ms = Some(ev.timestamp.timestamp_millis());
    match &ev.payload {
        TimelinePayload::UserPrompt {
            message_id,
            text,
            badges,
        } => {
            let key = (agent_id.to_string(), ev.seq, text.len());
            if markdown_cache.len() > 2048 {
                markdown_cache.clear();
            }
            let blocks = markdown_cache
                .entry(key)
                .or_insert_with(|| parse_markdown_blocks(text, false))
                .clone();
            Some(TimelineItemView::UserPrompt {
                seq: ev.seq,
                message_id: message_id.clone(),
                text: text.clone(),
                badges: badges.clone(),
                blocks,
                timestamp_ms: ts_ms,
            })
        }
        TimelinePayload::AssistantThinking { item_id, text } => {
            Some(TimelineItemView::AssistantThinking {
                seq: ev.seq,
                item_id: item_id.clone(),
                text: text.trim().to_string(),
                timestamp_ms: ts_ms,
            })
        }
        TimelinePayload::AssistantMessage {
            item_id,
            text,
            streaming,
        } => {
            if let Some(warning_msg) = extract_legacy_provider_warning_text(text) {
                Some(TimelineItemView::Notice {
                    seq: ev.seq,
                    level: "warning".into(),
                    message: warning_msg,
                    timestamp_ms: ts_ms,
                })
            } else if *streaming {
                let slot_key = (
                    agent_id.to_string(),
                    if item_id.is_empty() {
                        format!("seq:{}", ev.seq)
                    } else {
                        item_id.clone()
                    },
                );
                let blocks = match streaming_markdown_cache.get(&slot_key) {
                    Some((cached_len, cached_blocks)) if *cached_len == text.len() => {
                        cached_blocks.clone()
                    }
                    _ => {
                        let parsed = parse_markdown_blocks(text, true);
                        if streaming_markdown_cache.len() > 64 {
                            streaming_markdown_cache.clear();
                        }
                        streaming_markdown_cache.insert(slot_key, (text.len(), parsed.clone()));
                        parsed
                    }
                };
                Some(TimelineItemView::AssistantMessage {
                    seq: ev.seq,
                    item_id: item_id.clone(),
                    text: text.clone(),
                    streaming: true,
                    blocks,
                    timestamp_ms: ts_ms,
                })
            } else {
                if !item_id.is_empty() {
                    streaming_markdown_cache.remove(&(agent_id.to_string(), item_id.clone()));
                }
                let key = (agent_id.to_string(), ev.seq, text.len());
                if markdown_cache.len() > 2048 {
                    markdown_cache.clear();
                }
                let blocks = markdown_cache
                    .entry(key)
                    .or_insert_with(|| parse_markdown_blocks(text, false))
                    .clone();
                Some(TimelineItemView::AssistantMessage {
                    seq: ev.seq,
                    item_id: item_id.clone(),
                    text: text.clone(),
                    streaming: false,
                    blocks,
                    timestamp_ms: ts_ms,
                })
            }
        }
        TimelinePayload::ToolCall {
            call_id,
            status,
            detail,
            background_task,
        } => {
            let (summary_title, diff_files) = summarize_tool_detail(detail);
            let step_descriptor = Some(crate::stage::describe_tool_detail(
                *status,
                &summary_title,
                detail,
                &diff_files,
                agent_cwd,
            ));
            Some(TimelineItemView::ToolCall {
                seq: ev.seq,
                call_id: call_id.clone(),
                status: *status,
                summary_title,
                detail: detail.clone(),
                background_task: background_task.clone(),
                step_descriptor,
                diff_files,
                timestamp_ms: ts_ms,
            })
        }
        TimelinePayload::PermissionRequest {
            request_id,
            title,
            detail,
            allowed_decisions,
            guardian_assessment,
        } => Some(TimelineItemView::PermissionRequest {
            seq: ev.seq,
            request_id: request_id.clone(),
            title: title.clone(),
            detail: detail.clone(),
            allowed_decisions: allowed_decisions.clone(),
            resolved_decision: all_resolved_perms.get(request_id).cloned(),
            guardian_assessment: guardian_assessment.clone(),
            timestamp_ms: ts_ms,
        }),
        TimelinePayload::QuestionRequest {
            request_id,
            questions,
        } => Some(TimelineItemView::QuestionRequest {
            seq: ev.seq,
            request_id: request_id.clone(),
            questions: questions.clone(),
            resolved_answers: all_resolved_questions.get(request_id).cloned(),
            timestamp_ms: ts_ms,
        }),
        TimelinePayload::Notice { level, message } => Some(TimelineItemView::Notice {
            seq: ev.seq,
            level: level.clone(),
            message: message.clone(),
            timestamp_ms: ts_ms,
        }),
        TimelinePayload::AgentMessage {
            message_id,
            kind,
            source_agent_id,
            source_host_id,
            source_title,
            event,
            text,
            permission_context,
        } => {
            let key = (agent_id.to_string(), ev.seq, text.len());
            if markdown_cache.len() > 2048 {
                markdown_cache.clear();
            }
            let blocks = markdown_cache
                .entry(key)
                .or_insert_with(|| parse_markdown_blocks(text, false))
                .clone();
            Some(TimelineItemView::AgentMessage {
                seq: ev.seq,
                message_id: message_id.clone(),
                kind: *kind,
                source_agent_id: source_agent_id.clone(),
                source_host_id: source_host_id.clone(),
                source_title: source_title.clone(),
                event: event.clone(),
                text: text.clone(),
                permission_context: permission_context.clone(),
                blocks,
                timestamp_ms: ts_ms,
            })
        }
        TimelinePayload::StatusChanged {
            status: AgentStatus::Error { message, .. },
        } => (!message.trim().is_empty()).then(|| TimelineItemView::Notice {
            seq: ev.seq,
            level: "error".into(),
            message: message.clone(),
            timestamp_ms: ts_ms,
        }),
        TimelinePayload::StatusChanged {
            status: AgentStatus::AutonomousWakeup { source, .. },
        } => {
            // `waiting:*` transitions represent passive background-task wait state, not a new
            // wakeup boundary: the stage's trailing live indicator already renders
            // `Waiting for N background tasks…` directly from `currentAgent.status_label`, so
            // emitting a `WakeupDivider` here would duplicate the exact same text right above it.
            if source.trim().starts_with("waiting:") {
                None
            } else {
                Some(TimelineItemView::Notice {
                    seq: ev.seq,
                    level: "wakeup".into(),
                    message: format_autonomous_wakeup_notice(source),
                    timestamp_ms: ts_ms,
                })
            }
        }
        TimelinePayload::PermissionResolved { .. }
        | TimelinePayload::QuestionResolved { .. }
        | TimelinePayload::StatusChanged { .. } => None,
    }
}

fn extract_legacy_provider_warning_text(text: &str) -> Option<String> {
    if !text.contains('⚠') {
        return None;
    }
    let mut warnings = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let rest = trimmed.strip_prefix('⚠')?.trim();
        let lower = rest.to_ascii_lowercase();
        let is_runtime_warning = lower.starts_with("api error")
            || lower.starts_with("the stream was interrupted")
            || lower.starts_with("rate limit")
            || lower.starts_with("request failed")
            || lower.starts_with("connection ")
            || lower.starts_with("network ")
            || lower.starts_with("retrying")
            || lower.starts_with("error:");
        if !is_runtime_warning {
            return None;
        }
        warnings.push(rest);
    }
    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join("\n"))
    }
}

fn format_autonomous_wakeup_notice(source: &str) -> String {
    let trimmed = source.trim();
    if let Some(rest) = trimmed.strip_prefix("waiting:") {
        let desc = rest.trim();
        return if desc.is_empty() {
            "Waiting for background task(s)".to_string()
        } else {
            format!("Waiting for {desc}")
        };
    }
    let cleaned = trimmed.replace('_', " ");
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("background task") {
        "Resumed from background task".to_string()
    } else {
        format!("Resumed from {cleaned}")
    }
}

fn agent_status_is_foreground_running(status: &AgentStatus) -> bool {
    match status {
        AgentStatus::Initializing | AgentStatus::Running { .. } => true,
        AgentStatus::AutonomousWakeup { source, .. } => !source.trim().starts_with("waiting:"),
        AgentStatus::Idle
        | AgentStatus::AwaitingPermission { .. }
        | AgentStatus::Error { .. }
        | AgentStatus::Closed => false,
    }
}

/// Returns true if all stage/card-relevant fields of `AgentSummary` are unchanged
/// (ignoring `read_seq`, `updated_at`, and `last_activity_at`).
fn agent_stage_meta_unchanged(prev: &AgentSummary, next: &AgentSummary) -> bool {
    prev.workspace_id == next.workspace_id
        && prev.cwd == next.cwd
        && prev.title == next.title
        && prev.provider == next.provider
        && prev.model == next.model
        && prev.effort == next.effort
        && prev.fast_mode == next.fast_mode
        && prev.mode == next.mode
        && prev.status == next.status
        && prev.parent_agent_id == next.parent_agent_id
        && prev.parent_host_id == next.parent_host_id
        && prev.latest_seq == next.latest_seq
        && prev.timeline_epoch == next.timeline_epoch
        && prev.archived_at == next.archived_at
        && prev.background == next.background
        && prev.auto_archive == next.auto_archive
        && prev.profile_id == next.profile_id
        && prev.labels == next.labels
        && prev.attention == next.attention
        && prev.is_pinned == next.is_pinned
        && prev.pinned_order == next.pinned_order
        && prev.handle == next.handle
        && prev.task_summary == next.task_summary
        && prev.forked_from_agent_id == next.forked_from_agent_id
        && prev.forked_at_seq == next.forked_at_seq
        && prev.queued_prompts.len() == next.queued_prompts.len()
        && prev
            .queued_prompts
            .iter()
            .zip(next.queued_prompts.iter())
            .all(|(a, b)| a.queue_id == b.queue_id && a.display_text == b.display_text)
        && serde_json::to_string(&prev.cluster_suggestion).ok()
            == serde_json::to_string(&next.cluster_suggestion).ok()
}

fn agent_to_card_view(a: &AgentSummary, pref: Option<&ClientThreadPref>) -> AgentCardView {
    let (status_label, is_running, has_background_tasks, is_awaiting_permission) = match &a.status {
        AgentStatus::Initializing => ("Starting".to_string(), true, false, false),
        AgentStatus::Idle => ("Idle".to_string(), false, false, false),
        AgentStatus::Running { .. } => ("Working".to_string(), true, false, false),
        AgentStatus::AutonomousWakeup { source, .. } => {
            if let Some(rest) = source.trim().strip_prefix("waiting:") {
                let desc = rest.trim();
                let label = if desc.is_empty() {
                    "Waiting for background tasks".to_string()
                } else if desc.to_ascii_lowercase().starts_with("waiting") {
                    desc.to_string()
                } else {
                    format!("Waiting for {desc}")
                };
                (label, false, true, false)
            } else {
                (format!("Waking ({source})"), true, false, false)
            }
        }
        AgentStatus::AwaitingPermission { .. } => {
            ("Needs approval".to_string(), false, false, true)
        }
        AgentStatus::Error { .. } => ("Error".to_string(), false, false, false),
        AgentStatus::Closed => ("Closed".to_string(), false, false, false),
    };
    let local_seen_seq = pref.map(|p| p.seen_seq).unwrap_or(0);
    let effective_seen_seq = a.read_seq.max(local_seen_seq);
    let is_pinned = a.is_pinned || pref.is_some_and(|p| p.is_pinned);
    let is_unread = !a.agent_id.starts_with("opt_")
        && !is_running
        && (a.attention != AttentionState::None
            || (a.latest_seq > 0 && a.latest_seq > effective_seen_seq));
    let worktree_branch = a.labels.get("orbit.worktree_branch").cloned();
    let provider_label = if a.provider == ProviderKind::Acp {
        a.acp_agent_id
            .clone()
            .unwrap_or_else(|| a.provider.display_name().to_string())
    } else {
        a.provider.display_name().to_string()
    };
    AgentCardView {
        agent_id: a.agent_id.clone(),
        workspace_id: a.workspace_id.clone(),
        cwd: a.cwd.clone(),
        title: a.title.clone(),
        provider: a.provider,
        provider_label,
        model: a.model.clone(),
        effort: a.effort,
        fast_mode: a.fast_mode,
        mode: a.mode.clone(),
        status: a.status.clone(),
        status_label,
        is_running,
        has_background_tasks,
        is_awaiting_permission,
        parent_agent_id: a.parent_agent_id.clone(),
        parent_host_id: a.parent_host_id.clone(),
        profile_id: a.profile_id.clone(),
        background: a.background,
        auto_archive: a.auto_archive,
        attention: a.attention,
        archived_at_rfc3339: a.archived_at.map(|ts| ts.to_rfc3339()),
        labels: a.labels.clone(),
        latest_seq: a.latest_seq,
        updated_at_rfc3339: a.updated_at.to_rfc3339(),
        is_pinned,
        seen_seq: effective_seen_seq,
        read_seq: a.read_seq,
        is_unread,
        queued_prompts: a.queued_prompts.clone(),
        handle: a.handle.clone(),
        cluster_suggestion: a.cluster_suggestion.clone(),
        worktree_branch,
        task_summary: a.task_summary.clone(),
        forked_from_agent_id: a.forked_from_agent_id.clone(),
        forked_at_seq: a.forked_at_seq,
        acp_agent_id: a.acp_agent_id.clone(),
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
    for &kind in ProviderKind::all() {
        m.entry(kind).or_insert_with(|| {
            crate::catalog::fallback_provider_catalog(kind)
                .models
                .into_iter()
                .map(|spec| ModelOption {
                    id: spec.id,
                    label: spec.label,
                    description: (!spec.description.is_empty()).then_some(spec.description),
                    is_default: spec.is_default,
                })
                .collect()
        });
    }
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
                        background_task: None,
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
                        background_task: None,
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
                    background_task: None,
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
        assert_eq!(detail.provider_catalogs.len(), ProviderKind::all().len());

        let opt_id = core.create_agent_with_config(CreateAgentRequest {
            host_id: "host_mobile",
            cwd: "/Users/jess/Code/orbit",
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: Some("claude-opus-5-5".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: None,
            mode: SessionMode::AcceptEdits,
            initial_prompt: "Run clippy",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: None,
            auto_cluster: None,
        });

        let stage = core.agent_stage("host_mobile", &opt_id);
        assert_eq!(stage.turns.len(), 1);
        assert!(stage.turns[0].user_prompt.is_some());

        core.on_app_background();
        core.on_app_foreground();
        core.on_network_changed();
    }

    #[test]
    fn subscribe_coalesced_throttles_timeline_and_fleet_bursts_with_leading_edge() {
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

        // FleetChanged emits leading-edge immediately and coalesces rapid bursts into a trailing flush
        core.emit(|revision| CoreNotification::FleetChanged { revision });
        core.emit(|revision| CoreNotification::FleetChanged { revision });
        assert_eq!(counter.fleet_count.load(Ordering::SeqCst), 1);

        // Emit 10 rapid TimelineChanged events for the same agent -> 1 immediate + 1 trailing flush
        for _ in 0..10 {
            core.emit(|revision| CoreNotification::TimelineChanged {
                revision,
                host_id: "h1".into(),
                agent_id: "a1".into(),
            });
        }
        assert_eq!(counter.timeline_count.load(Ordering::SeqCst), 1);
        std::thread::sleep(Duration::from_millis(85));
        assert_eq!(counter.fleet_count.load(Ordering::SeqCst), 2);
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
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 5,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: Some(now),
                    queued_prompts: Vec::new(),
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

    #[test]
    fn projects_autonomous_wakeup_and_legacy_provider_warnings_as_notices() {
        let core = OrbitAppCore::open_in_memory().expect("in-memory core");
        let now = Utc::now();
        {
            let mut guard = core.hosts.write().expect("write");
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "host_warn".into(),
                hostname: "local".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 6868,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.agents.insert(
                "ag_w".into(),
                AgentSummary {
                    agent_id: "ag_w".into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/tmp".into(),
                    title: "Test".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 3,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: Some(now),
                    queued_prompts: Vec::new(),
                },
            );
            guard.insert("host_warn".into(), mem);
        }

        core.handle_inbound_wire(
            "host_warn",
            WireMessage::TimelineCatchup {
                request_id: Uuid::new_v4(),
                agent_id: "ag_w".into(),
                has_more: false,
                events: vec![
                    TimelineEvent {
                        seq: 1,
                        epoch: 1,
                        agent_id: "ag_w".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "m1".into(),
                            text: "\n⚠ API error (attempt 1): dial tcp i/o timeout\n".into(),
                            streaming: false,
                        },
                    },
                    TimelineEvent {
                        seq: 2,
                        epoch: 1,
                        agent_id: "ag_w".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::StatusChanged {
                            status: AgentStatus::AutonomousWakeup {
                                turn_id: "wk1".into(),
                                source: "background_task".into(),
                            },
                        },
                    },
                    TimelineEvent {
                        seq: 3,
                        epoch: 1,
                        agent_id: "ag_w".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "m2".into(),
                            text: "Normal assistant reply with ⚠ symbol inline.".into(),
                            streaming: false,
                        },
                    },
                    TimelineEvent {
                        seq: 4,
                        epoch: 1,
                        agent_id: "ag_w".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::StatusChanged {
                            status: AgentStatus::AutonomousWakeup {
                                turn_id: "bgwait_1".into(),
                                source: "waiting:3 background tasks".into(),
                            },
                        },
                    },
                ],
            },
        );

        let full = core.agent_timeline("host_warn", "ag_w");
        assert!(full.items.iter().any(|it| matches!(
            it,
            TimelineItemView::Notice { seq: 1, level, message, .. }
                if level == "warning" && message.contains("i/o timeout")
        )));
        assert!(full.items.iter().any(|it| matches!(
            it,
            TimelineItemView::Notice { seq: 2, level, message, .. }
                if level == "wakeup" && message.contains("background task")
        )));
        assert!(full.items.iter().any(|it| matches!(
            it,
            TimelineItemView::AssistantMessage { seq: 3, text, .. }
                if text.contains("Normal assistant reply")
        )));
        // Passive `waiting:*` transitions must NOT emit a duplicate `WakeupDivider` notice row!
        assert!(
            !full
                .items
                .iter()
                .any(|it| matches!(it, TimelineItemView::Notice { seq: 4, .. }))
        );
    }

    #[test]
    fn incremental_agent_stage_delta_slices_tail_resolves_gates_and_resets_on_rewind() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let now = Utc::now();
        {
            let mut guard = core.hosts.write().expect("lock");
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h1".into(),
                hostname: "h1".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 7474,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.agents.insert(
                "ag1".into(),
                AgentSummary {
                    agent_id: "ag1".into(),
                    workspace_id: "ws1".into(),
                    cwd: "/repo".into(),
                    title: "Stage Delta".into(),
                    provider: ProviderKind::Codex,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Running {
                        turn_id: "t2".into(),
                    },
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 5,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    timeline_epoch: 1,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: None,
                    queued_prompts: Vec::new(),
                },
            );
            guard.insert("h1".into(), mem);
        }

        core.handle_inbound_wire(
            "h1",
            WireMessage::TimelineCatchup {
                request_id: Uuid::new_v4(),
                agent_id: "ag1".into(),
                has_more: false,
                events: vec![
                    TimelineEvent {
                        seq: 1,
                        epoch: 1,
                        agent_id: "ag1".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::UserPrompt {
                            message_id: "u1".into(),
                            text: "turn 1".into(),
                            badges: vec![],
                        },
                    },
                    TimelineEvent {
                        seq: 2,
                        epoch: 1,
                        agent_id: "ag1".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::PermissionRequest {
                            request_id: "perm1".into(),
                            title: "Run test".into(),
                            detail: ToolCallDetail::Shell {
                                command: "cargo test".into(),
                                cwd: Some("/repo".into()),
                                exit_code: None,
                                output_preview: None,
                            },
                            allowed_decisions: vec!["allow_once".into()],
                            guardian_assessment: None,
                        },
                    },
                    TimelineEvent {
                        seq: 3,
                        epoch: 1,
                        agent_id: "ag1".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "m1".into(),
                            text: "done 1".into(),
                            streaming: false,
                        },
                    },
                    TimelineEvent {
                        seq: 4,
                        epoch: 1,
                        agent_id: "ag1".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::UserPrompt {
                            message_id: "u2".into(),
                            text: "turn 2".into(),
                            badges: vec![],
                        },
                    },
                    TimelineEvent {
                        seq: 5,
                        epoch: 1,
                        agent_id: "ag1".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "m2".into(),
                            text: "half".into(),
                            streaming: true,
                        },
                    },
                ],
            },
        );

        let d0 = core.agent_stage_delta("h1", "ag1", 0);
        assert!(d0.reset);
        assert_eq!(d0.total_turn_count, 2);
        assert_eq!(d0.pending_permissions.len(), 1);
        let rev1 = d0.revision;

        // No-op pull at same revision returns empty delta
        let d_same = core.agent_stage_delta("h1", "ag1", rev1);
        assert!(!d_same.reset);
        assert!(d_same.changed_turns.is_empty());

        // Tail streaming update only replaces turn index 1 (turn 2)
        core.handle_inbound_wire(
            "h1",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 6,
                    epoch: 1,
                    agent_id: "ag1".into(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::AssistantMessage {
                        item_id: "m2".into(),
                        text: "half + complete".into(),
                        streaming: false,
                    },
                },
            },
        );
        let d1 = core.agent_stage_delta("h1", "ag1", rev1);
        assert!(!d1.reset);
        assert_eq!(d1.replace_from_turn_index, 1);
        assert_eq!(d1.changed_turns.len(), 1);
        let rev2 = d1.revision;

        // PermissionResolved invalidates original turn 0 where gate lived
        core.handle_inbound_wire(
            "h1",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 7,
                    epoch: 1,
                    agent_id: "ag1".into(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::PermissionResolved {
                        request_id: "perm1".into(),
                        decision: "allow_once".into(),
                    },
                },
            },
        );
        let d2 = core.agent_stage_delta("h1", "ag1", rev2);
        assert!(!d2.reset);
        assert_eq!(d2.replace_from_turn_index, 0);
        assert!(d2.pending_permissions.is_empty());
        let rev3 = d2.revision;

        // Rewind resets stage revision history and forces full reset
        core.handle_inbound_wire(
            "h1",
            WireMessage::AgentRewound {
                request_id: Some(Uuid::new_v4()),
                agent: AgentSummary {
                    agent_id: "ag1".into(),
                    workspace_id: "ws1".into(),
                    cwd: "/repo".into(),
                    title: "Stage Delta".into(),
                    provider: ProviderKind::Codex,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 3,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    timeline_epoch: 2,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: None,
                    queued_prompts: Vec::new(),
                },
                to_before_seq: 4,
                events: vec![TimelineEvent {
                    seq: 1,
                    epoch: 2,
                    agent_id: "ag1".into(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::UserPrompt {
                        message_id: "u1".into(),
                        text: "turn 1".into(),
                        badges: vec![],
                    },
                }],
                restored_files: 0,
                restored_prompt: None,
                notice: None,
            },
        );
        let d_rewind = core.agent_stage_delta("h1", "ag1", rev3);
        assert!(d_rewind.reset);
        assert_eq!(d_rewind.replace_from_turn_index, 0);
        assert_eq!(d_rewind.total_turn_count, 1);
    }

    #[test]
    fn fleet_sidebar_thread_prefs_opt_migration_and_multislot_terminal() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let now = Utc::now();
        {
            let mut guard = core.hosts.write().expect("lock");
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h1".into(),
                hostname: "dev".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 7474,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.workspaces.push(WorkspaceSummary {
                workspace_id: "ws1".into(),
                cwd: "/work/orbit".into(),
                project_root: None,
                title: "orbit".into(),
                mission_goal: None,
                is_worktree: false,
                git_branch: Some("main".into()),
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            });
            guard.insert("h1".into(), mem);
        }

        // First AgentsSnapshot hydrates existing agent without unread badge
        core.handle_inbound_wire(
            "h1",
            WireMessage::AgentsSnapshot {
                request_id: Uuid::new_v4(),
                agents: vec![AgentSummary {
                    agent_id: "ag_old".into(),
                    workspace_id: "ws1".into(),
                    cwd: "/work/orbit".into(),
                    title: "Existing".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 10,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    timeline_epoch: 1,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: None,
                    queued_prompts: Vec::new(),
                }],
            },
        );
        let detail = core.host_detail("h1").expect("detail");
        assert!(!detail.agents[0].is_unread);

        // Pin optimistic session and verify migration on AgentCreated
        let req_id = Uuid::new_v4();
        let opt_id = format!("opt_{req_id}");
        {
            let mut guard = core.hosts.write().expect("lock");
            let h = guard.get_mut("h1").expect("h1");
            h.optimistic_by_req_id.insert(req_id, opt_id.clone());
            h.agents.insert(
                opt_id.clone(),
                AgentSummary {
                    agent_id: opt_id.clone(),
                    workspace_id: "ws1".into(),
                    cwd: "/work/orbit".into(),
                    title: "Draft".into(),
                    provider: ProviderKind::Codex,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Initializing,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: None,
                    queued_prompts: Vec::new(),
                },
            );
        }
        assert!(core.toggle_pin_agent("h1", &opt_id));
        core.handle_inbound_wire(
            "h1",
            WireMessage::AgentCreated {
                request_id: req_id,
                agent: AgentSummary {
                    agent_id: "ag_real".into(),
                    workspace_id: "ws1".into(),
                    cwd: "/work/orbit".into(),
                    title: "Draft".into(),
                    provider: ProviderKind::Codex,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 3,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    timeline_epoch: 1,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: None,
                    queued_prompts: Vec::new(),
                },
            },
        );

        let sidebar = core.fleet_sidebar(
            crate::sidebar::SidebarProjectSort::Recent,
            crate::sidebar::SidebarThreadFilter::All,
            "",
            Some("h1"),
            Some("ag_old"),
            Some("/work/orbit"),
        );
        assert_eq!(sidebar.pinned_conversations.len(), 1);
        assert_eq!(sidebar.pinned_conversations[0].agent.agent_id, "ag_real");
        assert!(sidebar.pinned_conversations[0].is_unread);
        core.mark_agent_seen("h1", "ag_real", 3);
        let sidebar_after_seen = core.fleet_sidebar(
            crate::sidebar::SidebarProjectSort::Recent,
            crate::sidebar::SidebarThreadFilter::All,
            "",
            Some("h1"),
            Some("ag_old"),
            Some("/work/orbit"),
        );
        assert!(!sidebar_after_seen.pinned_conversations[0].is_unread);

        // Multi-slot terminal isolation & StripTerminalBytesSubscriber
        let received = Arc::new(Mutex::new(Vec::new()));
        struct CapturingSub(Arc<Mutex<Vec<CoreNotification>>>);
        impl CoreEventSubscriber for CapturingSub {
            fn on_notification(&self, event: CoreNotification) {
                if let Ok(mut g) = self.0.lock() {
                    g.push(event);
                }
            }
        }
        core.subscribe_with_options(
            SubscribeOptions {
                min_interval_ms: 0,
                include_terminal_raw_bytes: false,
            },
            Arc::new(CapturingSub(Arc::clone(&received))),
        );

        core.handle_inbound_terminal(
            "h1",
            TerminalBinaryFrame::Output {
                slot: 1,
                data: b"slot-one".to_vec(),
            },
        );
        core.handle_inbound_terminal(
            "h1",
            TerminalBinaryFrame::Output {
                slot: 2,
                data: b"slot-two".to_vec(),
            },
        );
        let s1 = core.terminal_screen_slot("h1", 1).expect("slot 1");
        let s2 = core.terminal_screen_slot("h1", 2).expect("slot 2");
        let row_contains = |screen: &TerminalScreenView, needle: &str| {
            screen.rows_data.iter().any(|r| {
                r.spans
                    .iter()
                    .map(|sp| sp.text.as_str())
                    .collect::<String>()
                    .contains(needle)
            })
        };
        assert!(row_contains(&s1, "slot-one"));
        assert!(!row_contains(&s1, "slot-two"));
        assert!(row_contains(&s2, "slot-two"));

        let notes = received.lock().expect("notes");
        assert!(notes.iter().any(|n| matches!(
            n,
            CoreNotification::TerminalChanged { slot: 2, raw_bytes, .. } if raw_bytes.is_empty()
        )));
    }

    #[test]
    fn streaming_markdown_updates_do_not_evict_settled_cache_and_preserve_aux_revision() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let now = Utc::now();
        let mut mem = HostMemoryState::new(SavedHostProfile {
            host_id: "h_stream".into(),
            hostname: "stream-box".into(),
            custom_name: None,
            ssh_alias: None,
            route_mode: HostRouteMode::Auto,
            remote_daemon_port: 7474,
            custom_direct_endpoint: None,
            daemon_pubkey_b64: "pk".into(),
            direct_endpoints: vec![],
            relay_url: None,
            pairing_secret: String::new(),
            pairing_secret_updated_at_ms: 0,
            state: HostConnectionState::OnlineDirect,
        });

        let settled_ev = TimelineEvent {
            seq: 1,
            epoch: 1,
            agent_id: "ag_s".into(),
            turn_id: None,
            timestamp: now,
            payload: TimelinePayload::AssistantMessage {
                item_id: "settled_1".into(),
                text: "Settled paragraph".into(),
                streaming: false,
            },
        };
        let _ = render_timeline_event_view(
            "ag_s",
            None,
            &settled_ev,
            &HashMap::new(),
            &HashMap::new(),
            &mut mem.markdown_cache,
            &mut mem.streaming_markdown_cache,
        );
        assert_eq!(mem.markdown_cache.len(), 1);

        // Simulate 2500 streaming increments on one item: settled markdown_cache stays untouched.
        for step in 1..=2500usize {
            let stream_ev = TimelineEvent {
                seq: 10 + step as u64,
                epoch: 1,
                agent_id: "ag_s".into(),
                turn_id: None,
                timestamp: now,
                payload: TimelinePayload::AssistantMessage {
                    item_id: "live_1".into(),
                    text: "a".repeat(step),
                    streaming: true,
                },
            };
            let _ = render_timeline_event_view(
                "ag_s",
                None,
                &stream_ev,
                &HashMap::new(),
                &HashMap::new(),
                &mut mem.markdown_cache,
                &mut mem.streaming_markdown_cache,
            );
        }
        assert_eq!(mem.markdown_cache.len(), 1);
        assert_eq!(mem.streaming_markdown_cache.len(), 1);

        // Verify agent_stage_delta_with_aux skips unchanged aux state on pure text streaming
        mem.agents.insert(
            "ag_s".into(),
            AgentSummary {
                agent_id: "ag_s".into(),
                workspace_id: "ws1".into(),
                cwd: "/repo".into(),
                title: "Stream".into(),
                provider: ProviderKind::Codex,
                model: None,
                effort: None,
                fast_mode: false,
                mode: SessionMode::Default,
                status: AgentStatus::Running {
                    turn_id: "t1".into(),
                },
                parent_agent_id: None,
                parent_host_id: None,
                latest_seq: 2,
                read_seq: 0,
                is_pinned: false,
                pinned_order: 0,
                handle: None,
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: None,
                forked_at_seq: None,
                acp_agent_id: None,
                timeline_epoch: 1,
                created_at: now,
                updated_at: now,
                archived_at: None,
                background: false,
                auto_archive: false,
                profile_id: None,
                labels: HashMap::new(),
                attention: AttentionState::None,
                last_activity_at: None,
                queued_prompts: Vec::new(),
            },
        );
        core.hosts
            .write()
            .expect("lock")
            .insert("h_stream".into(), mem);

        core.handle_inbound_wire(
            "h_stream",
            WireMessage::TimelineCatchup {
                request_id: Uuid::new_v4(),
                agent_id: "ag_s".into(),
                has_more: false,
                events: vec![
                    TimelineEvent {
                        seq: 1,
                        epoch: 1,
                        agent_id: "ag_s".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::ToolCall {
                            call_id: "tc_edit".into(),
                            status: ToolCallStatus::Completed,
                            detail: ToolCallDetail::FileEdit {
                                path: "/repo/a.rs".into(),
                                unified_diff: Some(
                                    "--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-a\n+b".into(),
                                ),
                            },
                            background_task: None,
                        },
                    },
                    TimelineEvent {
                        seq: 2,
                        epoch: 1,
                        agent_id: "ag_s".into(),
                        turn_id: None,
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "msg_live".into(),
                            text: "| col1 | col2 |\n| --- | --- |\n| a | b |".into(),
                            streaming: true,
                        },
                    },
                ],
            },
        );

        let d0 = core.agent_stage_delta_with_aux("h_stream", "ag_s", 0, None);
        assert!(!d0.aux_unchanged);
        assert_eq!(d0.session_turn_diffs.len(), 1);
        let aux_rev = d0.aux_revision;

        // Streaming chunk without URLs or tool changes keeps aux_revision unchanged
        core.handle_inbound_wire(
            "h_stream",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 3,
                    epoch: 1,
                    agent_id: "ag_s".into(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::AssistantMessage {
                        item_id: "msg_live".into(),
                        text: "| col1 | col2 |\n| --- | --- |\n| a | b |\n\nTrailing prose".into(),
                        streaming: true,
                    },
                },
            },
        );
        let d1 = core.agent_stage_delta_with_aux("h_stream", "ag_s", d0.revision, Some(aux_rev));
        assert!(d1.aux_unchanged);
        assert_eq!(d1.aux_revision, aux_rev);
        assert!(d1.session_turn_diffs.is_empty());

        // Emitting a localhost URL bumps aux_revision and ships insights again
        core.handle_inbound_wire(
            "h_stream",
            WireMessage::TimelineLive {
                event: TimelineEvent {
                    seq: 4,
                    epoch: 1,
                    agent_id: "ag_s".into(),
                    turn_id: None,
                    timestamp: now,
                    payload: TimelinePayload::AssistantMessage {
                        item_id: "msg_live".into(),
                        text: "| col1 | col2 |\n| --- | --- |\n| a | b |\n\nPreview http://localhost:5173".into(),
                        streaming: false,
                    },
                },
            },
        );
        let d2 = core.agent_stage_delta_with_aux("h_stream", "ag_s", d1.revision, Some(aux_rev));
        assert!(!d2.aux_unchanged);
        assert!(d2.aux_revision > aux_rev);
        assert_eq!(d2.session_turn_diffs.len(), 1);
        assert_eq!(
            d2.insights.detected_local_urls,
            vec!["http://localhost:5173".to_string()]
        );
    }

    #[test]
    fn subagent_and_provider_subagent_updates_emit_parent_timeline_changed() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let now = Utc::now();

        let received = Arc::new(Mutex::new(Vec::new()));
        struct CapturingSub(Arc<Mutex<Vec<CoreNotification>>>);
        impl CoreEventSubscriber for CapturingSub {
            fn on_notification(&self, event: CoreNotification) {
                if let Ok(mut g) = self.0.lock() {
                    g.push(event);
                }
            }
        }
        core.subscribe_with_options(
            SubscribeOptions {
                min_interval_ms: 0,
                include_terminal_raw_bytes: false,
            },
            Arc::new(CapturingSub(Arc::clone(&received))),
        );

        {
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h_sub".into(),
                hostname: "sub-box".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 7474,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.agents.insert(
                "ag_parent".into(),
                AgentSummary {
                    agent_id: "ag_parent".into(),
                    workspace_id: "ws1".into(),
                    title: "Parent".into(),
                    cwd: "/repo".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    created_at: now,
                    updated_at: now,
                    latest_seq: 1,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    timeline_epoch: 1,
                    archived_at: None,
                    parent_agent_id: None,
                    parent_host_id: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: None,
                    queued_prompts: Vec::new(),
                },
            );
            core.hosts
                .write()
                .expect("hosts write lock")
                .insert("h_sub".into(), mem);
        }

        // Prime stage cache for parent
        let initial = core.agent_stage_delta_with_aux("h_sub", "ag_parent", 0, None);
        assert!(initial.insights.subagents.is_empty());

        // Clear any initial notifications
        received.lock().expect("lock").clear();

        // 1. Child Orbit agent update must notify parent's timeline_changed
        core.handle_inbound_wire(
            "h_sub",
            WireMessage::AgentUpdated {
                agent: AgentSummary {
                    agent_id: "ag_child".into(),
                    workspace_id: "ws1".into(),
                    title: "Child Worker".into(),
                    cwd: "/repo".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
                    created_at: now,
                    updated_at: now,
                    latest_seq: 1,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    timeline_epoch: 1,
                    archived_at: None,
                    parent_agent_id: Some("ag_parent".into()),
                    parent_host_id: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: None,
                    queued_prompts: Vec::new(),
                },
            },
        );
        {
            let mut notes = received.lock().expect("lock");
            assert!(notes.iter().any(|ev| matches!(
                ev,
                CoreNotification::TimelineChanged { agent_id, .. } if agent_id == "ag_parent"
            )));
            assert!(notes.iter().any(|ev| matches!(
                ev,
                CoreNotification::TimelineChanged { agent_id, .. } if agent_id == "ag_child"
            )));
            notes.clear();
        }

        // 2. ProviderSubagentUpdated must also notify parent's timeline_changed & update insights
        core.handle_inbound_wire(
            "h_sub",
            WireMessage::ProviderSubagentUpdated {
                subagent: orbit_protocol::ProviderSubagentDescriptor {
                    subagent_id: "conv_sub_1".into(),
                    parent_agent_id: "ag_parent".into(),
                    parent_subagent_id: None,
                    title: "Researcher".into(),
                    subtitle: Some("Investigating".into()),
                    status: orbit_protocol::ProviderSubagentStatus::Running,
                    created_at: now,
                    updated_at: now,
                },
            },
        );
        {
            let notes = received.lock().expect("lock");
            assert!(notes.iter().any(|ev| matches!(
                ev,
                CoreNotification::TimelineChanged { agent_id, .. } if agent_id == "ag_parent"
            )));
        }

        let delta = core.agent_stage_delta_with_aux(
            "h_sub",
            "ag_parent",
            initial.revision,
            Some(initial.aux_revision),
        );
        assert!(!delta.aux_unchanged);
        assert_eq!(delta.insights.subagents.len(), 2);
    }

    #[test]
    fn agent_to_card_view_splits_foreground_running_from_background_waiting() {
        let now = Utc::now();
        let make_summary = |status: AgentStatus| AgentSummary {
            agent_id: "ag_bg_split".into(),
            workspace_id: "ws_1".into(),
            cwd: "/repo".into(),
            title: "Preview".into(),
            provider: ProviderKind::Antigravity,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
            status,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 10,
            timeline_epoch: 1,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: AttentionState::Finished,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };

        let pref = ClientThreadPref {
            host_id: "h1".into(),
            agent_id: "ag_bg_split".into(),
            seen_seq: 5,
            is_pinned: false,
            pinned_order: 0,
            updated_at_ms: 0,
        };

        // 1. Waiting on background task: foreground is NOT busy, background flag IS true, unread works
        let waiting = agent_to_card_view(
            &make_summary(AgentStatus::AutonomousWakeup {
                turn_id: "bgwait_1".into(),
                source: "waiting:1 background task".into(),
            }),
            Some(&pref),
        );
        assert!(!waiting.is_running);
        assert!(waiting.has_background_tasks);
        assert!(!waiting.is_awaiting_permission);
        assert_eq!(waiting.status_label, "Waiting for 1 background task");
        assert!(waiting.is_unread);

        // 2. Active post-turn wakeup: foreground IS busy
        let waking = agent_to_card_view(
            &make_summary(AgentStatus::AutonomousWakeup {
                turn_id: "wakeup_1".into(),
                source: "background_task".into(),
            }),
            Some(&pref),
        );
        assert!(waking.is_running);
        assert!(!waking.has_background_tasks);
        assert!(!waking.is_unread);
    }

    #[test]
    fn provider_subagent_turns_projects_native_events_into_stage_turns() {
        let now = Utc::now();
        let core = OrbitAppCore::open_in_memory().expect("open core");
        {
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h_sub_turns".into(),
                hostname: "sub-box".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 7474,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.provider_subagent_timelines.insert(
                ("ag_parent".into(), "ps_reviewer".into()),
                vec![
                    ProviderSubagentEvent {
                        seq: 1,
                        parent_agent_id: "ag_parent".into(),
                        subagent_id: "ps_reviewer".into(),
                        timestamp: now,
                        payload: TimelinePayload::AssistantThinking {
                            item_id: "th_1".into(),
                            text: "Checking diff for privacy issues.".into(),
                        },
                    },
                    ProviderSubagentEvent {
                        seq: 2,
                        parent_agent_id: "ag_parent".into(),
                        subagent_id: "ps_reviewer".into(),
                        timestamp: now,
                        payload: TimelinePayload::ToolCall {
                            call_id: "tc_1".into(),
                            status: ToolCallStatus::Completed,
                            detail: ToolCallDetail::FileRead {
                                path: "/repo/src/main.rs".into(),
                                start_line: Some(1),
                                end_line: Some(40),
                            },
                            background_task: None,
                        },
                    },
                    ProviderSubagentEvent {
                        seq: 3,
                        parent_agent_id: "ag_parent".into(),
                        subagent_id: "ps_reviewer".into(),
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "msg_1".into(),
                            text: "## Review Summary\n\nNo privacy leaks found.".into(),
                            streaming: false,
                        },
                    },
                ],
            );
            core.hosts
                .write()
                .expect("hosts write lock")
                .insert("h_sub_turns".into(), mem);
        }

        let turns = core.provider_subagent_turns(
            "h_sub_turns",
            "ag_parent",
            "ps_reviewer",
            Some("Audit the recent diff for privacy issues."),
        );
        assert_eq!(turns.len(), 1);
        assert!(turns[0].user_prompt.is_some());
        assert_eq!(turns[0].work_counts.thought_count, 1);
        assert_eq!(turns[0].work_counts.read_file_count, 1);
        assert!(turns[0].terminal_reply.is_some());
        assert!(!turns[0].live_segments.is_empty());
    }

    #[test]
    fn provider_subagent_turns_settles_active_items_when_subagent_completed() {
        let now = Utc::now();
        let core = OrbitAppCore::open_in_memory().expect("open core");
        {
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h_sub_settled".into(),
                hostname: "sub-box".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 7474,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.provider_subagents.insert(
                "ag_parent".into(),
                vec![orbit_protocol::ProviderSubagentDescriptor {
                    subagent_id: "ps_done".into(),
                    parent_agent_id: "ag_parent".into(),
                    parent_subagent_id: None,
                    title: "Finished Worker".into(),
                    subtitle: None,
                    status: orbit_protocol::ProviderSubagentStatus::Completed,
                    created_at: now,
                    updated_at: now,
                }],
            );
            mem.provider_subagent_timelines.insert(
                ("ag_parent".into(), "ps_done".into()),
                vec![
                    ProviderSubagentEvent {
                        seq: 1,
                        parent_agent_id: "ag_parent".into(),
                        subagent_id: "ps_done".into(),
                        timestamp: now,
                        payload: TimelinePayload::ToolCall {
                            call_id: "tc_open".into(),
                            status: ToolCallStatus::Running,
                            detail: ToolCallDetail::FileRead {
                                path: "/repo/src/lib.rs".into(),
                                start_line: None,
                                end_line: None,
                            },
                            background_task: None,
                        },
                    },
                    ProviderSubagentEvent {
                        seq: 2,
                        parent_agent_id: "ag_parent".into(),
                        subagent_id: "ps_done".into(),
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "msg_streaming".into(),
                            text: "Final summary text.".into(),
                            streaming: true,
                        },
                    },
                ],
            );
            core.hosts
                .write()
                .expect("hosts write lock")
                .insert("h_sub_settled".into(), mem);
        }

        let turns = core.provider_subagent_turns(
            "h_sub_settled",
            "ag_parent",
            "ps_done",
            Some("Check lib.rs"),
        );
        assert_eq!(turns.len(), 1);
        assert!(
            !turns[0].has_active_item,
            "completed provider subagent must settle leftover active items"
        );
        assert!(turns[0].terminal_reply.is_some());
        for seg in &turns[0].live_segments {
            if let stage::TurnLiveSegmentView::StepGroup {
                has_running_step, ..
            } = seg
            {
                assert!(!has_running_step);
            }
        }
    }

    #[test]
    fn provider_subagent_timelines_evicts_oldest_finished_while_pinning_running() {
        let now = Utc::now();
        let mut mem = HostMemoryState::new(SavedHostProfile {
            host_id: "h_sub_lru".into(),
            hostname: "lru-box".into(),
            custom_name: None,
            ssh_alias: None,
            route_mode: HostRouteMode::Auto,
            remote_daemon_port: 7474,
            custom_direct_endpoint: None,
            daemon_pubkey_b64: "pk".into(),
            direct_endpoints: vec![],
            relay_url: None,
            pairing_secret: String::new(),
            pairing_secret_updated_at_ms: 0,
            state: HostConnectionState::OnlineDirect,
        });

        let mut descriptors = Vec::new();
        for idx in 0..12 {
            let sub_id = format!("ps_{idx}");
            let status = if idx == 0 || idx == 1 {
                orbit_protocol::ProviderSubagentStatus::Running
            } else {
                orbit_protocol::ProviderSubagentStatus::Completed
            };
            descriptors.push(orbit_protocol::ProviderSubagentDescriptor {
                subagent_id: sub_id.clone(),
                parent_agent_id: "ag_parent".into(),
                parent_subagent_id: None,
                title: format!("Worker {idx}"),
                subtitle: None,
                status,
                created_at: now,
                updated_at: now,
            });
            let key = ("ag_parent".to_string(), sub_id.clone());
            mem.provider_subagent_timelines.insert(
                key.clone(),
                vec![ProviderSubagentEvent {
                    seq: 1,
                    parent_agent_id: "ag_parent".into(),
                    subagent_id: sub_id,
                    timestamp: now,
                    payload: TimelinePayload::AssistantMessage {
                        item_id: format!("msg_{idx}"),
                        text: format!("Output {idx}"),
                        streaming: false,
                    },
                }],
            );
            mem.touch_provider_subagent_lru(&key);
        }
        mem.provider_subagents
            .insert("ag_parent".into(), descriptors);

        // Touch finished `ps_2` so it becomes most recently accessed and must survive eviction.
        let touched = ("ag_parent".to_string(), "ps_2".to_string());
        mem.touch_provider_subagent_lru(&touched);
        mem.enforce_provider_subagent_timeline_bounds(Some(&touched));

        assert_eq!(mem.provider_subagent_timelines.len(), 8);
        // Running subagents `ps_0` and `ps_1` stay pinned despite being oldest.
        assert!(
            mem.provider_subagent_timelines
                .contains_key(&("ag_parent".into(), "ps_0".into()))
        );
        assert!(
            mem.provider_subagent_timelines
                .contains_key(&("ag_parent".into(), "ps_1".into()))
        );
        // Recently touched finished `ps_2` stays cached.
        assert!(mem.provider_subagent_timelines.contains_key(&touched));
        // Oldest untouched finished entries (`ps_3`..=`ps_6`) were evicted.
        for evicted_idx in 3..=6 {
            assert!(
                !mem.provider_subagent_timelines
                    .contains_key(&("ag_parent".into(), format!("ps_{evicted_idx}"))),
                "expected ps_{evicted_idx} to be evicted"
            );
        }
    }

    #[test]
    fn provider_subagent_active_open_tab_is_pinned_against_lru_eviction() {
        let now = Utc::now();
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let open_key = ("ag_parent".to_string(), "ps_open".to_string());
        {
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h_sub_pin".into(),
                hostname: "pin-box".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 7474,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.provider_subagents.insert(
                "ag_parent".into(),
                vec![orbit_protocol::ProviderSubagentDescriptor {
                    subagent_id: "ps_open".into(),
                    parent_agent_id: "ag_parent".into(),
                    parent_subagent_id: None,
                    title: "Inspected Finished Subagent".into(),
                    subtitle: None,
                    status: orbit_protocol::ProviderSubagentStatus::Completed,
                    created_at: now,
                    updated_at: now,
                }],
            );
            mem.provider_subagent_timelines.insert(
                open_key.clone(),
                vec![ProviderSubagentEvent {
                    seq: 1,
                    parent_agent_id: "ag_parent".into(),
                    subagent_id: "ps_open".into(),
                    timestamp: now,
                    payload: TimelinePayload::AssistantMessage {
                        item_id: "msg_open".into(),
                        text: "Pinned transcript".into(),
                        streaming: false,
                    },
                }],
            );
            mem.touch_provider_subagent_lru(&open_key);
            core.hosts
                .write()
                .expect("hosts write lock")
                .insert("h_sub_pin".into(), mem);
        }

        // Mark `ps_open` as the actively open subagent tab via `open_provider_subagent_timeline`.
        core.open_provider_subagent_timeline("h_sub_pin", "ag_parent", "ps_open");

        // Stream 10 other finished subagents while `ps_open` stays open in the side panel.
        {
            let mut hosts = core.hosts.write().expect("hosts write lock");
            let mem = hosts.get_mut("h_sub_pin").expect("host exists");
            for idx in 0..10 {
                let sub_id = format!("ps_other_{idx}");
                mem.provider_subagents
                    .entry("ag_parent".into())
                    .or_default()
                    .push(orbit_protocol::ProviderSubagentDescriptor {
                        subagent_id: sub_id.clone(),
                        parent_agent_id: "ag_parent".into(),
                        parent_subagent_id: None,
                        title: format!("Other {idx}"),
                        subtitle: None,
                        status: orbit_protocol::ProviderSubagentStatus::Completed,
                        created_at: now,
                        updated_at: now,
                    });
                let key = ("ag_parent".to_string(), sub_id.clone());
                mem.provider_subagent_timelines.insert(
                    key.clone(),
                    vec![ProviderSubagentEvent {
                        seq: 1,
                        parent_agent_id: "ag_parent".into(),
                        subagent_id: sub_id,
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: format!("msg_other_{idx}"),
                            text: format!("Other output {idx}"),
                            streaming: false,
                        },
                    }],
                );
                mem.touch_provider_subagent_lru(&key);
                mem.enforce_provider_subagent_timeline_bounds(Some(&key));
            }

            assert_eq!(mem.provider_subagent_timelines.len(), 8);
            assert!(
                mem.provider_subagent_timelines.contains_key(&open_key),
                "actively open finished subagent must remain pinned against LRU eviction"
            );
        }
    }

    #[test]
    fn provider_subagent_turns_settles_failed_subagent_running_tool_as_failed() {
        let now = Utc::now();
        let core = OrbitAppCore::open_in_memory().expect("open core");
        {
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h_sub_failed".into(),
                hostname: "fail-box".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 7474,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.provider_subagents.insert(
                "ag_parent".into(),
                vec![orbit_protocol::ProviderSubagentDescriptor {
                    subagent_id: "ps_fail".into(),
                    parent_agent_id: "ag_parent".into(),
                    parent_subagent_id: None,
                    title: "Failing Worker".into(),
                    subtitle: None,
                    status: orbit_protocol::ProviderSubagentStatus::Failed,
                    created_at: now,
                    updated_at: now,
                }],
            );
            mem.provider_subagent_timelines.insert(
                ("ag_parent".into(), "ps_fail".into()),
                vec![ProviderSubagentEvent {
                    seq: 1,
                    parent_agent_id: "ag_parent".into(),
                    subagent_id: "ps_fail".into(),
                    timestamp: now,
                    payload: TimelinePayload::ToolCall {
                        call_id: "tc_stuck".into(),
                        status: ToolCallStatus::Running,
                        detail: ToolCallDetail::Shell {
                            command: "cargo test".into(),
                            cwd: None,
                            exit_code: None,
                            output_preview: None,
                        },
                        background_task: None,
                    },
                }],
            );
            core.hosts
                .write()
                .expect("hosts write lock")
                .insert("h_sub_failed".into(), mem);
        }

        let turns =
            core.provider_subagent_turns("h_sub_failed", "ag_parent", "ps_fail", Some("Run tests"));
        assert_eq!(turns.len(), 1);
        assert!(!turns[0].has_active_item);
        assert!(turns[0].has_error);
        assert_eq!(turns[0].work_counts.failed_count, 1);
    }

    #[test]
    fn coalescing_subscriber_collapses_fleet_and_host_bursts() {
        let core = OrbitAppCore::open_in_memory().expect("open");
        let received: Arc<Mutex<Vec<CoreNotification>>> = Arc::new(Mutex::new(Vec::new()));
        struct VecSub(Arc<Mutex<Vec<CoreNotification>>>);
        impl CoreEventSubscriber for VecSub {
            fn on_notification(&self, event: CoreNotification) {
                self.0.lock().expect("lock").push(event);
            }
        }
        let sub = CoalescingSubscriber::new(
            Arc::new(VecSub(Arc::clone(&received))),
            core.runtime.handle().clone(),
            Duration::from_millis(20),
            false,
        );

        for rev in 1..=20 {
            sub.on_notification(CoreNotification::FleetChanged { revision: rev });
            sub.on_notification(CoreNotification::HostChanged {
                revision: rev,
                host_id: "h1".into(),
            });
        }

        // Leading edge fires immediately once per channel
        {
            let got = received.lock().expect("lock");
            assert_eq!(got.len(), 2);
        }

        // Wait for trailing flush (2 * 20ms = 40ms)
        std::thread::sleep(Duration::from_millis(80));
        let got = received.lock().expect("lock");
        let fleet_revs: Vec<u64> = got
            .iter()
            .filter_map(|e| match e {
                CoreNotification::FleetChanged { revision } => Some(*revision),
                _ => None,
            })
            .collect();
        let host_revs: Vec<u64> = got
            .iter()
            .filter_map(|e| match e {
                CoreNotification::HostChanged { revision, host_id } if host_id == "h1" => {
                    Some(*revision)
                }
                _ => None,
            })
            .collect();
        assert_eq!(fleet_revs, vec![1, 20]);
        assert_eq!(host_revs, vec![1, 20]);
    }

    #[test]
    fn mark_agent_seen_quiet_cursor_advance_does_not_spam_fleet_events() {
        let core = OrbitAppCore::open_in_memory().expect("open");
        let now = Utc::now();
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h_seen".into(),
                hostname: "local".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 6868,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: String::new(),
                direct_endpoints: Vec::new(),
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.agents.insert(
                "ag_run".into(),
                AgentSummary {
                    agent_id: "ag_run".into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/repo".into(),
                    title: "Streaming".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::FullAccess,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 10,
                    timeline_epoch: 1,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: Some(now),
                    queued_prompts: Vec::new(),
                    read_seq: 9,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
            );
            hosts.insert("h_seen".into(), mem);
        }

        let received: Arc<Mutex<Vec<CoreNotification>>> = Arc::new(Mutex::new(Vec::new()));
        struct VecSub(Arc<Mutex<Vec<CoreNotification>>>);
        impl CoreEventSubscriber for VecSub {
            fn on_notification(&self, event: CoreNotification) {
                self.0.lock().expect("lock").push(event);
            }
        }
        core.subscribe(Arc::new(VecSub(Arc::clone(&received))));

        // Advancing seen cursor while actively running and AttentionState::None must NOT emit FleetChanged/HostChanged
        core.mark_agent_seen("h_seen", "ag_run", 10);
        assert!(received.lock().expect("lock").is_empty());

        // Transition to Finished (unread badge) and mark seen -> MUST emit FleetChanged + HostChanged
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let ag = hosts
                .get_mut("h_seen")
                .expect("h")
                .agents
                .get_mut("ag_run")
                .expect("ag");
            ag.status = AgentStatus::Idle;
            ag.attention = AttentionState::Finished;
            ag.latest_seq = 11;
        }
        core.mark_agent_seen("h_seen", "ag_run", 11);
        let notes = received.lock().expect("lock");
        assert!(
            notes
                .iter()
                .any(|n| matches!(n, CoreNotification::FleetChanged { .. }))
        );
        assert!(
            notes
                .iter()
                .any(|n| matches!(n, CoreNotification::HostChanged { .. }))
        );
    }

    #[test]
    fn agent_updated_read_seq_echo_does_not_bump_stage_aux_revision() {
        let core = OrbitAppCore::open_in_memory().expect("open");
        let now = Utc::now();
        let make_ag = |read_seq: u64| AgentSummary {
            agent_id: "ag_echo".into(),
            workspace_id: "ws_1".into(),
            cwd: "/repo".into(),
            title: "Echo".into(),
            provider: ProviderKind::Antigravity,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
            status: AgentStatus::Running {
                turn_id: "t1".into(),
            },
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 10,
            timeline_epoch: 1,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h_echo".into(),
                hostname: "local".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 6868,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: String::new(),
                direct_endpoints: Vec::new(),
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.agents.insert("ag_echo".into(), make_ag(5));
            hosts.insert("h_echo".into(), mem);
        }

        let initial = core.agent_stage_delta_with_aux("h_echo", "ag_echo", 0, None);
        let received: Arc<Mutex<Vec<CoreNotification>>> = Arc::new(Mutex::new(Vec::new()));
        struct VecSub(Arc<Mutex<Vec<CoreNotification>>>);
        impl CoreEventSubscriber for VecSub {
            fn on_notification(&self, event: CoreNotification) {
                self.0.lock().expect("lock").push(event);
            }
        }
        core.subscribe(Arc::new(VecSub(Arc::clone(&received))));

        // Echoed AgentUpdated where only read_seq advanced during active streaming
        core.handle_inbound_wire("h_echo", WireMessage::AgentUpdated { agent: make_ag(10) });
        assert!(
            received.lock().expect("lock").is_empty(),
            "read_seq-only echo during active running must not spam notifications"
        );
        let after = core.agent_stage_delta_with_aux(
            "h_echo",
            "ag_echo",
            initial.revision,
            Some(initial.aux_revision),
        );
        assert_eq!(after.revision, initial.revision);
        assert_eq!(after.aux_revision, initial.aux_revision);
        assert!(after.aux_unchanged);
    }

    #[test]
    fn stage_cache_lru_evicts_idle_agents_beyond_cap_while_keeping_running() {
        let core = OrbitAppCore::open_in_memory().expect("open");
        let now = Utc::now();
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: "h_lru".into(),
                hostname: "local".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 6868,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: String::new(),
                direct_endpoints: Vec::new(),
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            for idx in 0..12 {
                let id = format!("ag_{idx}");
                mem.agents.insert(
                    id.clone(),
                    AgentSummary {
                        agent_id: id,
                        workspace_id: "ws_1".into(),
                        cwd: "/repo".into(),
                        title: format!("Agent {idx}"),
                        provider: ProviderKind::Antigravity,
                        model: None,
                        effort: None,
                        fast_mode: false,
                        mode: SessionMode::FullAccess,
                        status: if idx == 0 {
                            AgentStatus::Running {
                                turn_id: "t0".into(),
                            }
                        } else {
                            AgentStatus::Idle
                        },
                        parent_agent_id: None,
                        parent_host_id: None,
                        latest_seq: 1,
                        timeline_epoch: 1,
                        created_at: now,
                        updated_at: now,
                        archived_at: None,
                        background: false,
                        auto_archive: false,
                        profile_id: None,
                        labels: HashMap::new(),
                        attention: AttentionState::None,
                        last_activity_at: Some(now),
                        queued_prompts: Vec::new(),
                        read_seq: 1,
                        is_pinned: false,
                        pinned_order: 0,
                        handle: None,
                        cluster_suggestion: None,
                        task_summary: None,
                        forked_from_agent_id: None,
                        forked_at_seq: None,
                        acp_agent_id: None,
                    },
                );
            }
            hosts.insert("h_lru".into(), mem);
        }

        for idx in 0..12 {
            let _ = core.agent_stage("h_lru", &format!("ag_{idx}"));
        }

        let hosts = core.hosts.read().expect("hosts");
        let mem = hosts.get("h_lru").expect("h_lru");
        assert!(
            mem.stage_cache.len() <= 8,
            "stage cache should stay bounded (got {})",
            mem.stage_cache.len()
        );
        assert!(
            mem.stage_cache.contains_key("ag_0"),
            "active running agent must never be evicted"
        );
        assert!(
            mem.stage_cache.contains_key("ag_11"),
            "most recently accessed agent must remain cached"
        );
    }

    #[test]
    fn e2e_github_device_flow_and_multi_device_encrypted_sync() {
        use std::sync::atomic::AtomicUsize;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        #[derive(Default)]
        struct MockGitHubState {
            poll_count: AtomicUsize,
            gist_content: Mutex<Option<String>>,
        }

        let state = Arc::new(MockGitHubState::default());
        let rt = tokio::runtime::Runtime::new().expect("rt");
        let base_url = rt.block_on({
            let state = Arc::clone(&state);
            async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind");
                let addr = listener.local_addr().expect("addr");
                tokio::spawn(async move {
                    loop {
                        let Ok((mut sock, _)) = listener.accept().await else {
                            break;
                        };
                        let st = Arc::clone(&state);
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 65_536];
                            let n = sock.read(&mut buf).await.unwrap_or(0);
                            if n == 0 {
                                return;
                            }
                            let req = String::from_utf8_lossy(&buf[..n]).to_string();
                            let first_line = req.lines().next().unwrap_or("");
                            let parts: Vec<&str> = first_line.split_whitespace().collect();
                            let method = parts.first().copied().unwrap_or("");
                            let path = parts.get(1).copied().unwrap_or("");
                            let body_str = req.split("\r\n\r\n").nth(1).unwrap_or("");

                            let (status, headers, resp_body) = match (method, path) {
                                ("POST", "/login/device/code") => (
                                    "200 OK",
                                    "",
                                    serde_json::json!({
                                        "device_code": "dev_code_123",
                                        "user_code": "ORBT-9942",
                                        "verification_uri": "https://github.com/login/device",
                                        "expires_in": 900,
                                        "interval": 5
                                    })
                                    .to_string(),
                                ),
                                ("POST", "/login/oauth/access_token") => {
                                    let count = st.poll_count.fetch_add(1, Ordering::SeqCst);
                                    if count == 0 {
                                        (
                                            "200 OK",
                                            "",
                                            serde_json::json!({"error": "authorization_pending"})
                                                .to_string(),
                                        )
                                    } else {
                                        (
                                            "200 OK",
                                            "",
                                            serde_json::json!({
                                                "access_token": "gho_test_token_xyz",
                                                "token_type": "bearer",
                                                "scope": "gist"
                                            })
                                            .to_string(),
                                        )
                                    }
                                }
                                ("GET", "/user") => (
                                    "200 OK",
                                    "X-OAuth-Scopes: gist\r\n",
                                    serde_json::json!({
                                        "login": "octocat",
                                        "name": "The Octocat",
                                        "avatar_url": "https://avatars.githubusercontent.com/u/583231"
                                    })
                                    .to_string(),
                                ),
                                ("GET", p) if p.starts_with("/gists?") => {
                                    let existing = st.gist_content.lock().expect("lock").clone();
                                    if existing.is_some() {
                                        (
                                            "200 OK",
                                            "",
                                            serde_json::json!([{
                                                "id": "gist_orbit_001",
                                                "html_url": "https://gist.github.com/octocat/gist_orbit_001",
                                                "files": {
                                                    crate::cloud_sync::CLOUD_VAULT_GIST_FILENAME: {}
                                                }
                                            }])
                                            .to_string(),
                                        )
                                    } else {
                                        ("200 OK", "", "[]".to_string())
                                    }
                                }
                                ("GET", "/gists/gist_orbit_001") => {
                                    let existing = st.gist_content.lock().expect("lock").clone();
                                    if let Some(content) = existing {
                                        (
                                            "200 OK",
                                            "",
                                            serde_json::json!({
                                                "id": "gist_orbit_001",
                                                "html_url": "https://gist.github.com/octocat/gist_orbit_001",
                                                "files": {
                                                    crate::cloud_sync::CLOUD_VAULT_GIST_FILENAME: {
                                                        "content": content,
                                                        "truncated": false
                                                    }
                                                }
                                            })
                                            .to_string(),
                                        )
                                    } else {
                                        (
                                            "404 Not Found",
                                            "",
                                            serde_json::json!({"message": "Not Found"}).to_string(),
                                        )
                                    }
                                }
                                ("POST", "/gists") | ("PATCH", "/gists/gist_orbit_001") => {
                                    let parsed: serde_json::Value =
                                        serde_json::from_str(body_str).unwrap_or_default();
                                    let content = parsed["files"]
                                        [crate::cloud_sync::CLOUD_VAULT_GIST_FILENAME]["content"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                    *st.gist_content.lock().expect("lock") = Some(content);
                                    (
                                        "200 OK",
                                        "",
                                        serde_json::json!({
                                            "id": "gist_orbit_001",
                                            "html_url": "https://gist.github.com/octocat/gist_orbit_001"
                                        })
                                        .to_string(),
                                    )
                                }
                                _ => (
                                    "404 Not Found",
                                    "",
                                    serde_json::json!({"message": "unhandled"}).to_string(),
                                ),
                            };

                            let raw = format!(
                                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{resp_body}",
                                resp_body.len()
                            );
                            let _ = sock.write_all(raw.as_bytes()).await;
                        });
                    }
                });
                format!("http://{addr}")
            }
        });

        // --- Device A: Sign in via Device Flow, pair 2 hosts, pin project, set prefs, sync ---
        let device_a = OrbitAppCore::open_in_memory().expect("device a");
        device_a.set_cloud_sync_github_bases_for_test(&base_url, &base_url);

        let started = device_a
            .cloud_sync_start_github_device_flow(Some("Ov23liTestClient".into()))
            .expect("start device flow");
        assert_eq!(started.user_code, "ORBT-9942");

        let poll1 = device_a
            .cloud_sync_poll_github_device_flow(&started.device_code, None)
            .expect("poll 1");
        assert_eq!(poll1.status, CloudSyncDevicePollStatus::Pending);

        let poll2 = device_a
            .cloud_sync_poll_github_device_flow(&started.device_code, None)
            .expect("poll 2");
        assert_eq!(poll2.status, CloudSyncDevicePollStatus::Authorized);
        let st_a = poll2.sync_status.expect("sync status");
        assert_eq!(st_a.account_login.as_deref(), Some("octocat"));
        assert_eq!(st_a.state, crate::cloud_sync::CloudSyncState::Locked);
        assert!(!st_a.remote_vault_exists);

        let kp1 = orbit_crypto::HostIdentityKeypair::generate().expect("kp1");
        let kp2 = orbit_crypto::HostIdentityKeypair::generate().expect("kp2");
        let h1_id = kp1.host_id().expect("h1");
        let h2_id = kp2.host_id().expect("h2");
        let offer1 = PairingOffer {
            host_id: h1_id.clone(),
            hostname: "secret-studio-mac".into(),
            daemon_pubkey_b64: kp1.public_key_b64,
            direct_endpoints: vec![
                "ws://127.0.0.1:6868/ws".into(),
                "ws://192.168.1.88:6868/ws".into(),
            ],
            relay_url: Some("wss://relay.private.local:7878".into()),
            pairing_secret: "sec1".into(),
        };
        let offer2 = PairingOffer {
            host_id: h2_id.clone(),
            hostname: "gpu-cluster-box".into(),
            daemon_pubkey_b64: kp2.public_key_b64,
            direct_endpoints: vec!["ws://10.0.0.5:6868/ws".into()],
            relay_url: None,
            pairing_secret: "sec2".into(),
        };
        device_a
            .pair_host(&offer1.to_qr_uri().expect("uri1"))
            .expect("pair h1");
        device_a
            .pair_host(&offer2.to_qr_uri().expect("uri2"))
            .expect("pair h2");
        let proj = device_a
            .upsert_project(None, &h1_id, "UltraSecretApp", vec!["/code/app".into()])
            .expect("proj");
        device_a
            .reorder_projects(vec![proj.project_id.clone()])
            .expect("order");
        device_a
            .cloud_sync_update_preferences(HashMap::from([
                ("theme".to_string(), serde_json::json!("dark")),
                ("enterToSend".to_string(), serde_json::json!(false)),
            ]))
            .expect("prefs");

        let unlock_a = device_a
            .cloud_sync_unlock_with_passphrase("super-strong-sync-passphrase")
            .expect("unlock a");
        assert!(unlock_a.pushed_remote);
        assert_eq!(unlock_a.vault_revision, 1);

        // Verify zero-knowledge property: raw Gist on GitHub must NOT contain plaintext hostnames/projects!
        let raw_gist = state
            .gist_content
            .lock()
            .expect("lock")
            .clone()
            .expect("gist written");
        assert!(raw_gist.contains("orbit-cloud-vault/v1"));
        assert!(!raw_gist.contains("secret-studio-mac"));
        assert!(!raw_gist.contains("UltraSecretApp"));
        assert!(!raw_gist.contains("relay.private.local"));

        // --- Device B: Brand-new empty device signs in, fails wrong passphrase, unlocks with right one ---
        let device_b = OrbitAppCore::open_in_memory().expect("device b");
        device_b.set_cloud_sync_github_bases_for_test(&base_url, &base_url);

        let st_b = device_b
            .cloud_sync_login_with_token("gho_test_token_xyz")
            .expect("token login");
        assert_eq!(st_b.state, crate::cloud_sync::CloudSyncState::Locked);
        assert!(st_b.remote_vault_exists);

        // Wrong passphrase must be rejected cleanly
        assert!(
            device_b
                .cloud_sync_unlock_with_passphrase("wrong-passphrase")
                .is_err()
        );

        // Correct passphrase unlocks and syncs everything immediately
        let unlock_b = device_b
            .cloud_sync_unlock_with_passphrase("super-strong-sync-passphrase")
            .expect("unlock b");
        assert_eq!(unlock_b.hosts_added, 2);
        assert_eq!(unlock_b.projects_added, 1);
        assert_eq!(unlock_b.preferences_updated, 2);

        let fleet_b = device_b.fleet_overview();
        assert_eq!(fleet_b.hosts.len(), 2);
        let synced_h1 = fleet_b
            .hosts
            .iter()
            .find(|h| h.host_id == h1_id)
            .expect("h1 on b");
        // Loopback 127.0.0.1 must have been stripped before cloud upload!
        assert_eq!(
            synced_h1.direct_endpoints,
            vec!["ws://192.168.1.88:6868/ws".to_string()]
        );
        assert_eq!(
            synced_h1.relay_url.as_deref(),
            Some("wss://relay.private.local:7878")
        );
        assert_eq!(fleet_b.projects.len(), 1);
        assert_eq!(fleet_b.projects[0].name, "UltraSecretApp");
        assert_eq!(fleet_b.project_order, vec![proj.project_id]);
        let prefs_b = device_b.cloud_sync_get_preferences();
        assert_eq!(prefs_b.get("theme"), Some(&serde_json::json!("dark")));
        assert_eq!(prefs_b.get("enterToSend"), Some(&serde_json::json!(false)));

        // --- Device B deletes h2, syncs -> Device A pulls and removes h2 via tombstone ---
        std::thread::sleep(Duration::from_millis(5));
        device_b.remove_host(&h2_id).expect("remove h2 on b");
        let sync_b = device_b.cloud_sync_now().expect("sync b push delete");
        assert!(sync_b.pushed_remote);

        let sync_a = device_a.cloud_sync_now().expect("sync a pull delete");
        assert_eq!(sync_a.hosts_removed, 1);
        assert_eq!(device_a.fleet_overview().hosts.len(), 1);
        assert_eq!(device_a.fleet_overview().hosts[0].host_id, h1_id);
    }

    #[test]
    fn live_event_before_catchup_never_jumps_contiguous_cursor_over_missing_user_prompt() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let now = Utc::now();
        let mut mem = HostMemoryState::new(SavedHostProfile {
            host_id: "h_gap".into(),
            hostname: "gap-host".into(),
            custom_name: None,
            ssh_alias: None,
            route_mode: HostRouteMode::Auto,
            remote_daemon_port: 6868,
            custom_direct_endpoint: None,
            daemon_pubkey_b64: "pk".into(),
            direct_endpoints: vec![],
            relay_url: None,
            pairing_secret: String::new(),
            pairing_secret_updated_at_ms: 0,
            state: HostConnectionState::OnlineDirect,
        });
        mem.agents.insert(
            "ag_gap".into(),
            AgentSummary {
                agent_id: "ag_gap".into(),
                workspace_id: "ws_1".into(),
                cwd: "/repo".into(),
                title: "Gap Test".into(),
                provider: ProviderKind::Antigravity,
                model: None,
                effort: None,
                fast_mode: false,
                mode: SessionMode::Default,
                status: AgentStatus::Running {
                    turn_id: "t2".into(),
                },
                parent_agent_id: None,
                parent_host_id: None,
                latest_seq: 12,
                timeline_epoch: 1,
                created_at: now,
                updated_at: now,
                archived_at: None,
                background: false,
                auto_archive: false,
                profile_id: None,
                labels: HashMap::new(),
                attention: AttentionState::None,
                last_activity_at: Some(now),
                read_seq: 10,
                is_pinned: false,
                pinned_order: 0,
                handle: None,
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: None,
                forked_at_seq: None,
                acp_agent_id: None,
                queued_prompts: Vec::new(),
            },
        );
        core.hosts
            .write()
            .expect("lock")
            .insert("h_gap".into(), mem);

        // Prior turn already persisted up to seq 10 before client restart
        let prev_reply = TimelineEvent {
            seq: 10,
            epoch: 1,
            agent_id: "ag_gap".into(),
            turn_id: Some("t1".into()),
            timestamp: now,
            payload: TimelinePayload::AssistantMessage {
                item_id: "msg_prev".into(),
                text: "Choose option A or B?".into(),
                streaming: false,
            },
        };
        core.store
            .upsert_replica_timeline_event(&prev_reply)
            .expect("persist prev");
        core.store
            .set_replica_contiguous_seq("ag_gap", 10)
            .expect("set cursor 10");

        // After restart (ag_gap NOT in caught_up_agents), TimelineLive seq 12 arrives before catchup
        let live_tool = TimelineEvent {
            seq: 12,
            epoch: 1,
            agent_id: "ag_gap".into(),
            turn_id: Some("t2".into()),
            timestamp: now,
            payload: TimelinePayload::ToolCall {
                call_id: "tc_1".into(),
                status: ToolCallStatus::Completed,
                detail: ToolCallDetail::FileRead {
                    path: "/repo/a.rs".into(),
                    start_line: None,
                    end_line: None,
                },
                background_task: None,
            },
        };
        core.store
            .upsert_replica_timeline_event(&live_tool)
            .expect("persist live 12");
        core.handle_inbound_wire(
            "h_gap",
            WireMessage::TimelineLive {
                event: live_tool.clone(),
            },
        );

        // Contiguous cursor MUST remain at 10 (not jump to 12) so seq 11 UserPrompt is not skipped
        assert_eq!(
            core.store
                .get_replica_contiguous_seq("ag_gap")
                .expect("cursor"),
            10
        );
        assert_eq!(core.safe_catchup_since_seq("ag_gap", 12), 10);

        // Now TimelineCatchup arrives with the missing UserPrompt (seq 11) + ToolCall (seq 12)
        let missing_prompt = TimelineEvent {
            seq: 11,
            epoch: 1,
            agent_id: "ag_gap".into(),
            turn_id: Some("t2".into()),
            timestamp: now,
            payload: TimelinePayload::UserPrompt {
                message_id: "u_opt_a".into(),
                text: "方案A".into(),
                badges: Vec::new(),
            },
        };
        let catchup_batch = vec![missing_prompt, live_tool];
        core.store
            .upsert_replica_timeline_events_batch(&catchup_batch)
            .expect("persist catchup");
        core.handle_inbound_wire(
            "h_gap",
            WireMessage::TimelineCatchup {
                request_id: Uuid::new_v4(),
                agent_id: "ag_gap".into(),
                events: catchup_batch,
                has_more: false,
            },
        );

        assert_eq!(
            core.store
                .get_replica_contiguous_seq("ag_gap")
                .expect("cursor 12"),
            12
        );
        let stage = core.agent_stage("h_gap", "ag_gap");
        assert!(stage.turns.iter().any(|t| matches!(
            &t.user_prompt,
            Some(TimelineItemView::UserPrompt { text, .. }) if text == "方案A"
        )));

        // Once caught up, a persisted live event (seq 13) advances contiguous_seq
        let live_reply = TimelineEvent {
            seq: 13,
            epoch: 1,
            agent_id: "ag_gap".into(),
            turn_id: Some("t2".into()),
            timestamp: now,
            payload: TimelinePayload::AssistantMessage {
                item_id: "msg_reply".into(),
                text: "Done!".into(),
                streaming: false,
            },
        };
        core.store
            .upsert_replica_timeline_event(&live_reply)
            .expect("persist live 13");
        core.handle_inbound_wire("h_gap", WireMessage::TimelineLive { event: live_reply });
        assert_eq!(
            core.store
                .get_replica_contiguous_seq("ag_gap")
                .expect("cursor 13"),
            13
        );

        // If a live event fails to persist to SQLite (not present in client_replica_events),
        // contiguous_seq resets to 0 instead of silently sealing a gap.
        let unpersisted_live = TimelineEvent {
            seq: 14,
            epoch: 1,
            agent_id: "ag_gap".into(),
            turn_id: Some("t2".into()),
            timestamp: now,
            payload: TimelinePayload::StatusChanged {
                status: AgentStatus::Idle,
            },
        };
        core.handle_inbound_wire(
            "h_gap",
            WireMessage::TimelineLive {
                event: unpersisted_live,
            },
        );
        assert_eq!(
            core.store
                .get_replica_contiguous_seq("ag_gap")
                .expect("cursor reset"),
            0
        );
    }

    #[test]
    fn search_messages_cross_host_enrichment_cjk_punctuation_and_cleanup() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let now = Utc::now();

        // Set up two hosts across a multi-host fleet
        let mut mem_a = HostMemoryState::new(SavedHostProfile {
            host_id: "host_devbox".into(),
            hostname: "devbox.internal".into(),
            custom_name: Some("Devbox".into()),
            ssh_alias: None,
            route_mode: HostRouteMode::Auto,
            remote_daemon_port: 6868,
            custom_direct_endpoint: None,
            daemon_pubkey_b64: "pk_a".into(),
            direct_endpoints: vec![],
            relay_url: None,
            pairing_secret: String::new(),
            pairing_secret_updated_at_ms: 0,
            state: HostConnectionState::OnlineDirect,
        });
        mem_a.initial_agents_hydrated = true;
        mem_a.agents.insert(
            "ag_db".into(),
            AgentSummary {
                agent_id: "ag_db".into(),
                workspace_id: "ws_orbit".into(),
                cwd: "/Users/dev/code/orbit".into(),
                title: "SQLite FTS5 Search".into(),
                provider: ProviderKind::Antigravity,
                model: None,
                effort: None,
                fast_mode: false,
                mode: SessionMode::Default,
                status: AgentStatus::Idle,
                parent_agent_id: None,
                parent_host_id: None,
                latest_seq: 2,
                timeline_epoch: 0,
                created_at: now,
                updated_at: now,
                archived_at: None,
                background: false,
                auto_archive: false,
                profile_id: None,
                labels: HashMap::new(),
                attention: AttentionState::None,
                last_activity_at: Some(now),
                read_seq: 2,
                is_pinned: false,
                pinned_order: 0,
                handle: None,
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: None,
                forked_at_seq: None,
                acp_agent_id: None,
                queued_prompts: Vec::new(),
            },
        );
        let mut mem_b = HostMemoryState::new(SavedHostProfile {
            host_id: "host_mac".into(),
            hostname: "mac-mini".into(),
            custom_name: None,
            ssh_alias: None,
            route_mode: HostRouteMode::Auto,
            remote_daemon_port: 6868,
            custom_direct_endpoint: None,
            daemon_pubkey_b64: "pk_b".into(),
            direct_endpoints: vec![],
            relay_url: None,
            pairing_secret: String::new(),
            pairing_secret_updated_at_ms: 0,
            state: HostConnectionState::OnlineDirect,
        });
        mem_b.initial_agents_hydrated = true;
        mem_b.agents.insert(
            "ag_web".into(),
            AgentSummary {
                agent_id: "ag_web".into(),
                workspace_id: "ws_web".into(),
                cwd: "/srv/projects/orbit-web".into(),
                title: "Cmd+K UI".into(),
                provider: ProviderKind::ClaudeCode,
                model: None,
                effort: None,
                fast_mode: false,
                mode: SessionMode::Default,
                status: AgentStatus::Idle,
                parent_agent_id: None,
                parent_host_id: None,
                latest_seq: 1,
                timeline_epoch: 0,
                created_at: now,
                updated_at: now,
                archived_at: None,
                background: false,
                auto_archive: false,
                profile_id: None,
                labels: HashMap::new(),
                attention: AttentionState::None,
                last_activity_at: Some(now),
                read_seq: 1,
                is_pinned: false,
                pinned_order: 0,
                handle: None,
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: None,
                forked_at_seq: None,
                acp_agent_id: None,
                queued_prompts: Vec::new(),
            },
        );

        {
            let mut hosts = core.hosts.write().expect("hosts write");
            hosts.insert("host_devbox".into(), mem_a);
            hosts.insert("host_mac".into(), mem_b);
        }
        core.upsert_project(
            Some("proj_orbit"),
            "host_devbox",
            "Orbit Core",
            vec!["/Users/dev/code/orbit".into()],
        )
        .expect("upsert project");

        // Index replica messages across both hosts
        core.store
            .upsert_replica_timeline_events_batch_for_host(
                "host_devbox",
                &[
                    TimelineEvent {
                        seq: 1,
                        epoch: 0,
                        agent_id: "ag_db".into(),
                        turn_id: Some("t1".into()),
                        timestamp: now,
                        payload: TimelinePayload::UserPrompt {
                            message_id: "m1".into(),
                            text: "请实现全文检索以及 foo-bar 路由解析与 a/b 测试".into(),
                            badges: Vec::new(),
                        },
                    },
                    TimelineEvent {
                        seq: 2,
                        epoch: 0,
                        agent_id: "ag_db".into(),
                        turn_id: Some("t1".into()),
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "a1".into(),
                            text: "Added nebula_fts_engine with BM25 scoring and \"unclosed quote safety.".into(),
                            streaming: false,
                        },
                    },
                ],
            )
            .expect("seed host_devbox");

        core.store
            .upsert_replica_timeline_event_for_host(
                "host_mac",
                &TimelineEvent {
                    seq: 1,
                    epoch: 0,
                    agent_id: "ag_web".into(),
                    turn_id: Some("t1".into()),
                    timestamp: now,
                    payload: TimelinePayload::AssistantMessage {
                        item_id: "a2".into(),
                        text: "Wired nebula_fts_engine into CommandPalette with 80ms debounce."
                            .into(),
                        streaming: false,
                    },
                },
            )
            .expect("seed host_mac");

        // Cross-host query returns enriched hits from both hosts
        let hits = core.search_messages("nebula_fts_engine", 10);
        assert_eq!(hits.len(), 2);
        let devbox_hit = hits
            .iter()
            .find(|h| h.host_id == "host_devbox")
            .expect("devbox hit");
        assert_eq!(devbox_hit.host_name, "Devbox");
        assert_eq!(devbox_hit.agent_id, "ag_db");
        assert_eq!(devbox_hit.title, "SQLite FTS5 Search");
        assert_eq!(devbox_hit.project_name, "Orbit Core");
        assert_eq!(devbox_hit.role, "assistant");
        assert!(
            devbox_hit
                .snippet
                .contains("<mark>nebula_fts_engine</mark>")
        );
        assert!(!devbox_hit.updated_at_rfc3339.is_empty());

        let mac_hit = hits
            .iter()
            .find(|h| h.host_id == "host_mac")
            .expect("mac hit");
        assert_eq!(mac_hit.host_name, "mac-mini");
        assert_eq!(mac_hit.project_name, "orbit-web");

        // CJK + punctuation queries through OrbitAppCore
        let cjk_hits = core.search_messages("全文检索", 10);
        assert_eq!(cjk_hits.len(), 1);
        assert_eq!(cjk_hits[0].agent_id, "ag_db");
        assert!(cjk_hits[0].snippet.contains("<mark>全文检索</mark>"));

        assert_eq!(core.search_messages("foo-bar", 10).len(), 1);
        assert_eq!(core.search_messages("a/b", 10).len(), 1);
        assert_eq!(core.search_messages("\"unclosed", 10).len(), 1);
        assert!(core.search_messages("   ", 10).is_empty());

        // Rewinding replica timeline removes rewound events from search results
        core.store
            .replace_replica_timeline_for_host("host_devbox", "ag_db", &[])
            .expect("rewind empty");
        assert!(core.search_messages("全文检索", 10).is_empty());
        assert_eq!(core.search_messages("nebula_fts_engine", 10).len(), 1);

        // Deleting agent removes remaining hit
        core.delete_agent("host_mac", "ag_web");
        assert!(core.search_messages("nebula_fts_engine", 10).is_empty());

        // Orphaned / unknown-host FTS entries never resurface with empty host_id or placeholder title
        core.store
            .upsert_replica_timeline_event_for_host(
                "host_removed",
                &TimelineEvent {
                    seq: 1,
                    epoch: 0,
                    agent_id: "ag_ghost".into(),
                    turn_id: Some("t1".into()),
                    timestamp: now,
                    payload: TimelinePayload::UserPrompt {
                        message_id: "g1".into(),
                        text: "ghost_orphan_sentinel_token".into(),
                        badges: Vec::new(),
                    },
                },
            )
            .expect("seed ghost host");
        core.store
            .upsert_replica_timeline_event_for_host(
                "host_devbox",
                &TimelineEvent {
                    seq: 1,
                    epoch: 0,
                    agent_id: "ag_missing_from_mem".into(),
                    turn_id: Some("t1".into()),
                    timestamp: now,
                    payload: TimelinePayload::UserPrompt {
                        message_id: "g2".into(),
                        text: "ghost_orphan_sentinel_token".into(),
                        badges: Vec::new(),
                    },
                },
            )
            .expect("seed missing agent");
        assert!(
            core.search_messages("ghost_orphan_sentinel_token", 10)
                .is_empty(),
            "orphaned host/agent FTS rows must never surface in search_messages"
        );
    }

    #[test]
    fn agent_forked_updates_host_state_and_projects_provenance_card() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let now = Utc::now();
        let host_id = "host_fork";
        {
            let mut hosts = core.hosts.write().expect("hosts");
            hosts.insert(
                host_id.into(),
                HostMemoryState::new(SavedHostProfile {
                    host_id: host_id.into(),
                    hostname: "devbox".into(),
                    custom_name: None,
                    ssh_alias: None,
                    route_mode: HostRouteMode::Auto,
                    remote_daemon_port: 7474,
                    custom_direct_endpoint: None,
                    daemon_pubkey_b64: String::new(),
                    direct_endpoints: vec![],
                    relay_url: None,
                    pairing_secret: String::new(),
                    pairing_secret_updated_at_ms: 0,
                    state: HostConnectionState::OnlineDirect,
                }),
            );
        }

        let req_id = Uuid::new_v4();
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut hosts = core.hosts.write().expect("hosts");
            hosts
                .get_mut(host_id)
                .expect("host")
                .pending_forks
                .insert(req_id, tx);
        }

        let forked_ws = WorkspaceSummary {
            workspace_id: "ws_wt_fork".into(),
            title: "Auth (fork)".into(),
            cwd: "/tmp/wt/auth-fork".into(),
            git_branch: Some("orbit/fork-auth".into()),
            is_worktree: true,
            updated_at: now,
            source_cwd: Some("/repo/auth".into()),
            base_ref: Some("main".into()),
            parent_workspace_id: Some("ws_main".into()),
            archived_at: None,
            background: false,
            setup_status: orbit_protocol::WorkspaceSetupStatus::Idle,
            mission_goal: None,
            project_root: Some("/repo/auth".into()),
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: false,
        };
        let forked_agent = AgentSummary {
            agent_id: "ag_fork_child".into(),
            workspace_id: "ws_wt_fork".into(),
            cwd: "/tmp/wt/auth-fork".into(),
            title: "Auth (fork)".into(),
            provider: ProviderKind::ClaudeCode,
            model: Some("sonnet".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 12,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::from([("orbit.worktree_branch".into(), "orbit/fork-auth".into())]),
            attention: AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 12,
            is_pinned: false,
            pinned_order: 0,
            handle: Some("fork".into()),
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: Some("ag_parent_origin".into()),
            forked_at_seq: Some(8),
            acp_agent_id: None,
        };

        core.handle_inbound_wire(
            host_id,
            WireMessage::AgentForked {
                request_id: req_id,
                agent: forked_agent,
                workspace: Some(forked_ws),
            },
        );

        let res = rx
            .blocking_recv()
            .expect("oneshot")
            .expect("fork succeeded");
        assert_eq!(res.agent.agent_id, "ag_fork_child");
        assert_eq!(
            res.agent.forked_from_agent_id.as_deref(),
            Some("ag_parent_origin")
        );
        assert_eq!(res.agent.forked_at_seq, Some(8));
        assert_eq!(
            res.workspace.as_ref().map(|w| w.workspace_id.as_str()),
            Some("ws_wt_fork")
        );

        let detail = core.host_detail(host_id).expect("detail");
        assert_eq!(
            detail.last_created_agent_id.as_deref(),
            Some("ag_fork_child")
        );
        let card = detail
            .agents
            .iter()
            .find(|a| a.agent_id == "ag_fork_child")
            .expect("projected card");
        assert_eq!(
            card.forked_from_agent_id.as_deref(),
            Some("ag_parent_origin")
        );
        assert_eq!(card.forked_at_seq, Some(8));
        assert!(
            detail
                .workspaces
                .iter()
                .any(|w| w.workspace_id == "ws_wt_fork")
        );
    }

    #[test]
    fn update_agent_full_guards_optimistic_id_and_uses_narrow_invalidation() {
        use std::sync::atomic::AtomicU64;

        struct EventCounter {
            fleet: AtomicU64,
            host: AtomicU64,
            timeline: AtomicU64,
        }
        impl CoreEventSubscriber for EventCounter {
            fn on_notification(&self, notification: CoreNotification) {
                match notification {
                    CoreNotification::FleetChanged { .. } => {
                        self.fleet.fetch_add(1, Ordering::Relaxed);
                    }
                    CoreNotification::HostChanged { .. } => {
                        self.host.fetch_add(1, Ordering::Relaxed);
                    }
                    CoreNotification::TimelineChanged { .. } => {
                        self.timeline.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
        }

        let core = OrbitAppCore::open_in_memory().expect("core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("kp");
        let offer = PairingOffer {
            host_id: "host_opt".into(),
            hostname: "devbox".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "sec".into(),
        };
        core.pair_host(&offer.to_qr_uri().expect("uri"))
            .expect("pair");

        let opt_id = core.create_agent_with_config(CreateAgentRequest {
            host_id: "host_opt",
            cwd: "/repo",
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: Some("claude-opus-4-6".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: Some(false),
            mode: SessionMode::AcceptEdits,
            initial_prompt: "hello",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: None,
            auto_cluster: None,
        });
        assert!(opt_id.starts_with("opt_"));

        // Clear any pending queued wire messages from `create_agent`
        {
            let mut hosts = core.hosts.write().expect("hosts");
            if let Some(h) = hosts.get_mut("host_opt") {
                h.pending_outbox.clear();
            }
        }

        let counter = Arc::new(EventCounter {
            fleet: AtomicU64::new(0),
            host: AtomicU64::new(0),
            timeline: AtomicU64::new(0),
        });
        core.subscribe(counter.clone());

        // Toggle fast mode on an optimistic `opt_*` agent:
        // 1. Updates local optimistic agent state
        // 2. Does NOT dispatch a wire UpdateAgent for `opt_*`
        // 3. Emits FleetChanged + HostChanged WITHOUT forcing TimelineChanged
        core.set_agent_fast_mode("host_opt", &opt_id, true);

        {
            let hosts = core.hosts.read().expect("hosts");
            let h = hosts.get("host_opt").expect("host");
            assert!(h.agents.get(&opt_id).expect("opt agent").fast_mode);
            assert!(
                h.pending_outbox.is_empty(),
                "update_agent_full must not dispatch wire messages for opt_* agents"
            );
        }
        assert!(counter.fleet.load(Ordering::Relaxed) >= 1);
        assert!(counter.host.load(Ordering::Relaxed) >= 1);
        assert_eq!(
            counter.timeline.load(Ordering::Relaxed),
            0,
            "metadata-only update_agent_full must not force TimelineChanged"
        );
    }

    #[test]
    fn historical_artifact_read_never_downgrades_cache_and_unknown_snapshot_merges() {
        use orbit_protocol::{ArtifactDetail, ArtifactKind, ArtifactRevisionMeta, ArtifactSummary};

        let core = OrbitAppCore::open_in_memory().expect("open");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = orbit_crypto::PairingOffer {
            host_id: "host_art".into(),
            hostname: "art-box".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "secret".into(),
        };
        core.pair_host(&offer.to_qr_uri().expect("uri"))
            .expect("pair");

        let now = Utc::now();
        let art_a_v5 = ArtifactSummary {
            artifact_id: "art_a".into(),
            workspace_id: "ws_1".into(),
            agent_id: Some("ag_1".into()),
            title: "Dashboard".into(),
            kind: ArtifactKind::Html,
            language: Some("html".into()),
            mime_type: Some("text/html".into()),
            resource_uri: None,
            version: 5,
            byte_len: 100,
            created_at: now,
            updated_at: now,
        };
        let art_b_v1 = ArtifactSummary {
            artifact_id: "art_b".into(),
            workspace_id: "ws_2".into(),
            agent_id: Some("ag_2".into()),
            title: "Spec".into(),
            kind: ArtifactKind::Markdown,
            language: None,
            mime_type: Some("text/markdown".into()),
            resource_uri: None,
            version: 1,
            byte_len: 50,
            created_at: now,
            updated_at: now,
        };

        // Seed host cache via registered unsolicited unfiltered ListArtifacts request
        let init_req_id = Uuid::new_v4();
        {
            let mut hosts = core.hosts.write().expect("hosts");
            hosts
                .get_mut("host_art")
                .expect("host")
                .unsolicited_unfiltered_artifact_reqs
                .insert(init_req_id);
        }
        core.handle_inbound_wire(
            "host_art",
            WireMessage::ArtifactsSnapshot {
                request_id: init_req_id,
                artifacts: vec![art_a_v5.clone(), art_b_v1.clone()],
            },
        );
        assert_eq!(core.cached_artifacts("host_art", None, None).len(), 2);

        // 1. Reading historical v1 of art_a (latest is v5) must NOT downgrade cache to v1
        let mut art_a_v1_summary = art_a_v5.clone();
        art_a_v1_summary.version = 1;
        core.handle_inbound_wire(
            "host_art",
            WireMessage::ArtifactContent {
                request_id: Uuid::new_v4(),
                artifact: ArtifactDetail {
                    summary: art_a_v1_summary,
                    content: "<div>old v1</div>".into(),
                    revisions: vec![
                        ArtifactRevisionMeta {
                            version: 5,
                            created_at: now,
                            summary_note: None,
                            byte_len: 100,
                        },
                        ArtifactRevisionMeta {
                            version: 1,
                            created_at: now,
                            summary_note: None,
                            byte_len: 17,
                        },
                    ],
                },
            },
        );
        let after_hist = core.cached_artifacts("host_art", Some("ws_1"), None);
        assert_eq!(after_hist.len(), 1);
        assert_eq!(after_hist[0].version, 5);

        // 2. Late/unknown timed-out filtered snapshot for ws_1 must merge, NOT wipe ws_2!
        core.handle_inbound_wire(
            "host_art",
            WireMessage::ArtifactsSnapshot {
                request_id: Uuid::new_v4(),
                artifacts: vec![art_a_v5],
            },
        );
        let after_late = core.cached_artifacts("host_art", None, None);
        assert_eq!(
            after_late.len(),
            2,
            "unknown/late filtered snapshot must not wipe other workspaces' cached artifacts"
        );
    }

    #[test]
    fn is_registered_local_cwd_enforces_local_registered_roots() {
        let core = OrbitAppCore::open_in_memory().expect("core");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("kp");
        let offer = PairingOffer {
            host_id: "host_local_cwd_test".into(),
            hostname: "macbook".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "sec".into(),
        };
        core.pair_host(&offer.to_qr_uri().expect("uri"))
            .expect("pair");

        let registered_dir =
            std::env::temp_dir().join(format!("orbit_reg_cwd_{}", uuid::Uuid::new_v4()));
        let nested_dir = registered_dir.join("packages").join("web");
        let unregistered_dir =
            std::env::temp_dir().join(format!("orbit_unreg_cwd_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&nested_dir).expect("mkdir nested");
        std::fs::create_dir_all(&unregistered_dir).expect("mkdir unreg");

        let reg_str = registered_dir.to_string_lossy().to_string();
        let nested_str = nested_dir.to_string_lossy().to_string();
        let unreg_str = unregistered_dir.to_string_lossy().to_string();

        // Remote host (`is_local == false`) must always be rejected even when registered.
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let h = hosts.get_mut("host_local_cwd_test").expect("host");
            h.is_local = false;
            h.discovered_projects.push(DiscoveredProject {
                path: reg_str.clone(),
                name: "reg".into(),
                git_branch: None,
                is_git_repo: true,
                last_modified_unix: 0,
                nested_git_repos: Vec::new(),
            });
        }
        assert!(!core.is_registered_local_cwd("host_local_cwd_test", &reg_str));

        // Flip to local host: registered root + nested subdir pass; `/`, `~`, `$HOME`, and unregistered fail.
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let h = hosts.get_mut("host_local_cwd_test").expect("host");
            h.is_local = true;
        }
        assert!(core.is_registered_local_cwd("host_local_cwd_test", &reg_str));
        assert!(core.is_registered_local_cwd("host_local_cwd_test", &nested_str));
        assert!(!core.is_registered_local_cwd("host_local_cwd_test", "/"));
        assert!(!core.is_registered_local_cwd("host_local_cwd_test", "~"));
        assert!(!core.is_registered_local_cwd("host_local_cwd_test", ""));
        assert!(!core.is_registered_local_cwd("host_local_cwd_test", &unreg_str));
        if let Some(home) = crate::desktop_ops::resolve_user_home() {
            assert!(!core.is_registered_local_cwd("host_local_cwd_test", &home.to_string_lossy()));
        }

        let _ = std::fs::remove_dir_all(&registered_dir);
        let _ = std::fs::remove_dir_all(&unregistered_dir);
    }

    #[test]
    fn provider_subagent_catchup_paginates_resubscribes_and_cleans_on_agent_deleted() {
        let core = OrbitAppCore::open_in_memory().expect("core");
        let now = Utc::now();
        let host_id = "h_sub_page";
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let mut mem = HostMemoryState::new(SavedHostProfile {
                host_id: host_id.into(),
                hostname: "local".into(),
                custom_name: None,
                ssh_alias: None,
                route_mode: HostRouteMode::Auto,
                remote_daemon_port: 6868,
                custom_direct_endpoint: None,
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                state: HostConnectionState::OnlineDirect,
            });
            mem.agents.insert(
                "ag_p1".into(),
                AgentSummary {
                    agent_id: "ag_p1".into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/tmp".into(),
                    title: "Parent".into(),
                    provider: ProviderKind::Antigravity,
                    acp_agent_id: None,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    latest_seq: 1,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    parent_host_id: None,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: None,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    queued_prompts: Vec::new(),
                },
            );
            mem.caught_up_agents.insert("ag_p1".into());
            mem.rewound_agents.insert("ag_p1".into());
            hosts.insert(host_id.into(), mem);
        }

        // 1. Open provider subagent timeline -> tracks subscription and queues SubscribeProviderSubagent
        core.open_provider_subagent_timeline(host_id, "ag_p1", "sub_a");
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let h = hosts.get_mut(host_id).expect("host");
            assert!(
                h.subscribed_provider_subagents
                    .contains(&("ag_p1".to_string(), "sub_a".to_string()))
            );
            // Clear initial outbox so we can observe follow-up pagination messages cleanly
            h.pending_outbox.clear();
        }

        // 2. Catchup page 1 with `has_more = true` must enqueue follow-up `since_seq = 2`
        core.handle_inbound_wire(
            host_id,
            WireMessage::ProviderSubagentTimelineCatchup {
                request_id: Uuid::new_v4(),
                parent_agent_id: "ag_p1".into(),
                subagent_id: "sub_a".into(),
                events: vec![
                    ProviderSubagentEvent {
                        seq: 1,
                        parent_agent_id: "ag_p1".into(),
                        subagent_id: "sub_a".into(),
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "m1".into(),
                            text: "page1-a".into(),
                            streaming: false,
                        },
                    },
                    ProviderSubagentEvent {
                        seq: 2,
                        parent_agent_id: "ag_p1".into(),
                        subagent_id: "sub_a".into(),
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "m2".into(),
                            text: "page1-b".into(),
                            streaming: false,
                        },
                    },
                ],
                has_more: true,
            },
        );
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let h = hosts.get_mut(host_id).expect("host");
            assert!(
                h.pending_outbox.iter().any(|m| matches!(
                    m,
                    WireMessage::SubscribeProviderSubagent {
                        parent_agent_id,
                        subagent_id,
                        since_seq: 2,
                        ..
                    } if parent_agent_id == "ag_p1" && subagent_id == "sub_a"
                )),
                "has_more=true must trigger follow-up SubscribeProviderSubagent with since_seq=2"
            );
            h.pending_outbox.clear();

            // 3. Reconnect helper must resubscribe active provider subagent at latest seq = 2
            let resubs = collect_provider_subagent_resubscriptions(h);
            assert_eq!(resubs, vec![("ag_p1".into(), "sub_a".into(), 2)]);
        }

        // 4. Empty catchup with `has_more = true` must NOT infinite-loop
        core.handle_inbound_wire(
            host_id,
            WireMessage::ProviderSubagentTimelineCatchup {
                request_id: Uuid::new_v4(),
                parent_agent_id: "ag_p1".into(),
                subagent_id: "sub_a".into(),
                events: Vec::new(),
                has_more: true,
            },
        );
        {
            let hosts = core.hosts.read().expect("hosts");
            let h = hosts.get(host_id).expect("host");
            assert!(
                h.pending_outbox.is_empty(),
                "empty catchup must not loop even if has_more=true"
            );
        }

        // 5. Logical item replacement on live/catchup (mutated seq replaces older superseded seq)
        //    and page-boundary live event ahead of catchup still triggers follow-up pagination!
        core.handle_inbound_wire(
            host_id,
            WireMessage::ProviderSubagentTimelineLive {
                event: ProviderSubagentEvent {
                    seq: 10,
                    parent_agent_id: "ag_p1".into(),
                    subagent_id: "sub_a".into(),
                    timestamp: now,
                    payload: TimelinePayload::AssistantThinking {
                        item_id: "wake_th_1".into(),
                        text: "v1".into(),
                    },
                },
            },
        );
        core.handle_inbound_wire(
            host_id,
            WireMessage::ProviderSubagentTimelineLive {
                event: ProviderSubagentEvent {
                    seq: 12,
                    parent_agent_id: "ag_p1".into(),
                    subagent_id: "sub_a".into(),
                    timestamp: now,
                    payload: TimelinePayload::AssistantThinking {
                        item_id: "wake_th_1".into(),
                        text: "v2-expanded".into(),
                    },
                },
            },
        );
        {
            let timeline = core.provider_subagent_timeline(host_id, "ag_p1", "sub_a");
            let th_events: Vec<_> = timeline
                .iter()
                .filter(|e| matches!(&e.payload, TimelinePayload::AssistantThinking { item_id, .. } if item_id == "wake_th_1"))
                .collect();
            assert_eq!(
                th_events.len(),
                1,
                "superseded seq 10 must be replaced in memory by seq 12"
            );
            assert_eq!(th_events[0].seq, 12);
        }

        // Now catchup delivers older page [seq 3, seq 4] with has_more=true while live seq 12 is already cached!
        {
            let mut hosts = core.hosts.write().expect("hosts");
            hosts.get_mut(host_id).expect("host").pending_outbox.clear();
        }
        core.handle_inbound_wire(
            host_id,
            WireMessage::ProviderSubagentTimelineCatchup {
                request_id: Uuid::new_v4(),
                parent_agent_id: "ag_p1".into(),
                subagent_id: "sub_a".into(),
                events: vec![
                    ProviderSubagentEvent {
                        seq: 3,
                        parent_agent_id: "ag_p1".into(),
                        subagent_id: "sub_a".into(),
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "m3".into(),
                            text: "page2-a".into(),
                            streaming: false,
                        },
                    },
                    ProviderSubagentEvent {
                        seq: 4,
                        parent_agent_id: "ag_p1".into(),
                        subagent_id: "sub_a".into(),
                        timestamp: now,
                        payload: TimelinePayload::AssistantMessage {
                            item_id: "m4".into(),
                            text: "page2-b".into(),
                            streaming: false,
                        },
                    },
                ],
                has_more: true,
            },
        );
        {
            let hosts = core.hosts.read().expect("hosts");
            let h = hosts.get(host_id).expect("host");
            assert!(
                h.pending_outbox.iter().any(|m| matches!(
                    m,
                    WireMessage::SubscribeProviderSubagent {
                        parent_agent_id,
                        subagent_id,
                        since_seq: 4,
                        ..
                    } if parent_agent_id == "ag_p1" && subagent_id == "sub_a"
                )),
                "catchup inserting new seqs below live seq 12 must still paginate with since_seq=4"
            );
        }

        // 6. Evicting finished timelines removes them from `subscribed_provider_subagents` (prevents reconnect thrash)
        {
            let mut hosts = core.hosts.write().expect("hosts");
            let h = hosts.get_mut(host_id).expect("host");
            let mut descs = Vec::new();
            for i in 0..10 {
                let sid = format!("sub_evict_{i}");
                descs.push(ProviderSubagentDescriptor {
                    subagent_id: sid.clone(),
                    parent_agent_id: "ag_p1".into(),
                    parent_subagent_id: None,
                    title: sid.clone(),
                    subtitle: None,
                    status: orbit_protocol::ProviderSubagentStatus::Completed,
                    created_at: now,
                    updated_at: now,
                });
                let k = ("ag_p1".to_string(), sid);
                h.subscribed_provider_subagents.insert(k.clone());
                h.provider_subagent_timelines.insert(k.clone(), Vec::new());
                h.touch_provider_subagent_lru(&k);
            }
            h.provider_subagents.insert("ag_p1".into(), descs);
            h.enforce_provider_subagent_timeline_bounds(None);
            assert!(
                !h.subscribed_provider_subagents
                    .contains(&("ag_p1".to_string(), "sub_evict_0".to_string())),
                "evicted finished subagent must also be removed from subscribed_provider_subagents"
            );
        }

        // 7. AgentDeleted must prune subagent timelines, active subagent, subscriptions, caches, and sets
        core.handle_inbound_wire(
            host_id,
            WireMessage::AgentDeleted {
                agent_id: "ag_p1".into(),
            },
        );
        {
            let hosts = core.hosts.read().expect("hosts");
            let h = hosts.get(host_id).expect("host");
            assert!(h.provider_subagent_timelines.is_empty());
            assert!(h.provider_subagent_access_order.is_empty());
            assert!(h.active_provider_subagent.is_none());
            assert!(h.subscribed_provider_subagents.is_empty());
            assert!(!h.stage_cache.contains_key("ag_p1"));
            assert!(!h.caught_up_agents.contains("ag_p1"));
            assert!(!h.rewound_agents.contains("ag_p1"));
        }
    }

    #[test]
    fn export_host_pairing_uri_preserves_secret_and_rejects_empty() {
        let core = OrbitAppCore::open_in_memory().expect("core");
        let secret = orbit_crypto::generate_pairing_secret();
        let offer = PairingOffer {
            host_id: "host_export_1".into(),
            hostname: "workstation".into(),
            daemon_pubkey_b64: "pk_export_1".into(),
            direct_endpoints: vec!["ws://192.168.1.99:6868/ws".into()],
            relay_url: Some("wss://relay.example.com:7878".into()),
            pairing_secret: secret.clone(),
        };
        let uri = offer.to_qr_uri().expect("qr");
        core.pair_host(&uri).expect("pair host");

        let exported_uri = core
            .export_host_pairing_uri("host_export_1")
            .expect("export pairing uri");
        let decoded = PairingOffer::from_qr_uri(&exported_uri).expect("decode exported");
        assert_eq!(decoded.host_id, "host_export_1");
        assert_eq!(decoded.pairing_secret, secret);
        assert_eq!(decoded.direct_endpoints, offer.direct_endpoints);
        assert_eq!(decoded.relay_url, offer.relay_url);

        // Clearing the secret must fail closed rather than emitting a dummy secret
        {
            let mut guard = core.hosts.write().expect("hosts");
            guard
                .get_mut("host_export_1")
                .expect("host")
                .profile
                .pairing_secret
                .clear();
        }
        assert!(core.export_host_pairing_uri("host_export_1").is_err());
    }
}
