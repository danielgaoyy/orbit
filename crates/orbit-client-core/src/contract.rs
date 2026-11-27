//! Standardized Client Bridge Protocol v2 DTOs shared across:
//! - Tauri Desktop IPC (`apps/desktop/src-tauri`)
//! - Mobile UniFFI (`crates/orbit-ffi-uniffi`)
//! - HarmonyOS N-API (`crates/orbit-ffi-ohos`)

use std::collections::BTreeMap;

use orbit_protocol::{
    AcpAgentAvailability, ArtifactKind, ForkTarget, InstructionFileEntry, McpTransportKind,
    McpWriteScope, NestedGitRepo, PromptBadge, ProviderKind, ReasoningEffort, SessionMode,
    WorkspaceSummary, WorktreeHandoffDirection, WorktreeHandoffMode,
};
use serde::{Deserialize, Serialize};

use crate::HostRouteMode;
pub use crate::diagram::{DiagramKind, DiagramTheme, RenderDiagramRequest, RenderDiagramView};
pub use crate::export::{
    ExportAgentMeta, ExportFormat, ExportOptions, ExportedTranscript, ExtractedCodeBlock,
};
use crate::sidebar::{SidebarProjectSort, SidebarThreadFilter};
pub use crate::viewmodel::{AgentCardView, AskSideQuestionRequest};

pub const CLIENT_BRIDGE_PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRegistryRefreshView {
    pub agents: Vec<AcpAgentAvailability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeInfoView {
    pub protocol_version: u32,
    pub core_version: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FleetSidebarRequest {
    #[serde(default)]
    pub sort: SidebarProjectSort,
    #[serde(default)]
    pub filter: SidebarThreadFilter,
    #[serde(default, alias = "search_query")]
    pub query: String,
    #[serde(default)]
    pub active_host_id: Option<String>,
    #[serde(default)]
    pub active_agent_id: Option<String>,
    #[serde(default)]
    pub active_cwd: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentStageDeltaRequest {
    pub host_id: String,
    pub agent_id: String,
    #[serde(default)]
    pub since_revision: u64,
    #[serde(default)]
    pub since_aux_revision: Option<u64>,
    #[serde(default)]
    pub turn_limit: Option<u32>,
    #[serde(default)]
    pub since_window_start_turn_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub host_id: String,
    pub cwd: String,
    #[serde(default)]
    pub extra_dirs: Vec<String>,
    pub provider: ProviderKind,
    #[serde(default)]
    pub acp_agent_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub fast_mode: Option<bool>,
    pub mode: SessionMode,
    pub initial_prompt: String,
    #[serde(default)]
    pub initial_display_prompt: Option<String>,
    #[serde(default)]
    pub initial_badges: Vec<PromptBadge>,
    #[serde(default)]
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub auto_cluster: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendPromptRequest {
    pub host_id: String,
    pub agent_id: String,
    pub prompt: String,
    #[serde(default)]
    pub display_prompt: Option<String>,
    #[serde(default)]
    pub badges: Vec<PromptBadge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateHostConfigRequest {
    pub host_id: String,
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(default)]
    pub route_mode: Option<HostRouteMode>,
    #[serde(default)]
    pub ssh_alias: Option<String>,
    #[serde(default)]
    pub remote_daemon_port: Option<u16>,
    #[serde(default)]
    pub custom_direct_endpoint: Option<String>,
    #[serde(default)]
    pub relay_url: Option<String>,
    #[serde(default = "default_reconnect_true")]
    pub reconnect: bool,
}

fn default_reconnect_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRewindAgentRequest {
    pub host_id: String,
    pub agent_id: String,
    pub to_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindPreviewFileView {
    pub path: String,
    pub action: String,
    pub additions: u32,
    pub deletions: u32,
    pub reverse_unified_diff: String,
    pub has_external_conflict: bool,
    /// Pre-parsed and syntax-highlighted reverse diff ready for thin UI `<DiffFilesViewer />`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_file: Option<crate::render::DiffFileView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindPreviewView {
    pub agent_id: String,
    pub to_seq: u64,
    pub checkpoint_kind: orbit_protocol::RewindCheckpointKind,
    pub files: Vec<RewindPreviewFileView>,
    pub external_conflicts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindAgentRequest {
    pub host_id: String,
    pub agent_id: String,
    pub to_before_seq: u64,
    #[serde(default)]
    pub restore_files: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkAgentRequest {
    pub host_id: String,
    pub source_agent_id: String,
    #[serde(default)]
    pub up_to_seq: Option<u64>,
    #[serde(default)]
    pub target: ForkTarget,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub initial_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkAgentResultView {
    pub agent: AgentCardView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadWorkspaceFileRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_etag: Option<String>,
}

/// Authoritative daemon reply for `UpdateWorkspaceBoard` with optimistic CAS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBoardUpdateOutcome {
    pub ok: bool,
    pub conflict: bool,
    pub board: orbit_protocol::WorkspaceBoardSnapshot,
}

fn default_symbol_search_limit() -> u32 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchWorkspaceSymbolsRequest {
    pub host_id: String,
    pub cwd: String,
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_symbol_search_limit")]
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSymbolsView {
    pub host_id: String,
    pub cwd: String,
    pub symbols: Vec<orbit_protocol::WorkspaceSymbolEntry>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteWorkspaceFileRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceFileWriteResultView {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub etag: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime_unix_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<orbit_protocol::WorkspaceFileConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkspaceEntryRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    #[serde(default)]
    pub is_dir: bool,
    #[serde(default)]
    pub initial_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameWorkspaceEntryRequest {
    pub host_id: String,
    pub cwd: String,
    pub old_path: String,
    pub new_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteWorkspaceEntryRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEntryMutationResultView {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_path: Option<String>,
    pub action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSelectionRequest {
    #[serde(default)]
    pub host_id: Option<String>,
    pub provider: ProviderKind,
    #[serde(default)]
    pub desired_model: Option<String>,
    #[serde(default)]
    pub desired_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MentionRankRequest {
    pub host_id: String,
    pub cwd: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub limit: usize,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub active_agent_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitReposInspectionView {
    pub path: String,
    pub is_git_repo: bool,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub nested_git_repos: Vec<NestedGitRepo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSearchHit {
    pub host_id: String,
    #[serde(default)]
    pub host_name: String,
    pub agent_id: String,
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub role: String,
    pub title: String,
    pub project_name: String,
    pub snippet: String,
    pub updated_at_rfc3339: String,
    /// `true` when this match was discovered from a remote daemon's full-history FTS5 index
    /// rather than already being matched in the local client replica.
    #[serde(default)]
    pub is_remote: bool,
    /// `true` when this event `seq` precedes the client's locally loaded timeline window,
    /// signalling that jumping to it should auto-paginate older history.
    #[serde(default)]
    pub is_unloaded: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchMessagesRequest {
    pub query: String,
    #[serde(default)]
    pub host_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchMessagesResponse {
    pub hits: Vec<ConversationSearchHit>,
    #[serde(default)]
    pub remote_hosts_queried: u32,
    #[serde(default)]
    pub remote_hosts_failed: u32,
}

/// Host-scoped agent and request identifiers for a server-verified permission batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestId {
    pub agent_id: String,
    pub permission_request_id: String,
}
impl From<PermissionRequestId> for (String, String) {
    fn from(item: PermissionRequestId) -> Self {
        (item.agent_id, item.permission_request_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnWorkspacePeerRequest {
    pub host_id: String,
    pub workspace_id: String,
    pub cwd: String,
    #[serde(default)]
    pub extra_dirs: Vec<String>,
    pub provider: ProviderKind,
    #[serde(default)]
    pub acp_agent_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub fast_mode: Option<bool>,
    pub mode: SessionMode,
    pub initial_prompt: String,
    #[serde(default)]
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

fn default_include_committed_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewWorktreeHandoffRequest {
    pub host_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub source_agent_id: Option<String>,
    pub direction: WorktreeHandoffDirection,
    #[serde(default)]
    pub target_workspace_id: Option<String>,
    #[serde(default = "default_include_committed_true")]
    pub include_committed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteWorktreeHandoffRequest {
    pub host_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub source_agent_id: Option<String>,
    pub direction: WorktreeHandoffDirection,
    pub mode: WorktreeHandoffMode,
    #[serde(default)]
    pub target_workspace_id: Option<String>,
    #[serde(default = "default_include_committed_true")]
    pub include_committed: bool,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub allow_three_way_conflicts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertArtifactRequest {
    pub host_id: String,
    #[serde(default)]
    pub artifact_id: Option<String>,
    pub workspace_id: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    pub title: String,
    pub kind: ArtifactKind,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub resource_uri: Option<String>,
    #[serde(default)]
    pub mcp_server_name: Option<String>,
    pub content: String,
    #[serde(default)]
    pub summary_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListInstructionFilesRequest {
    pub host_id: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionFilesSnapshotView {
    pub host_id: String,
    pub cwd: String,
    pub files: Vec<InstructionFileEntry>,
    pub total_tokens: u32,
    pub merged_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadInstructionFileRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionFileContentView {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveInstructionFileRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub content: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionFileSavedView {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub etag: String,
    pub size_bytes: u64,
    pub approx_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertExternalMcpServerRequest {
    pub host_id: String,
    pub scope: McpWriteScope,
    #[serde(default)]
    pub cwd: Option<String>,
    pub name: String,
    pub transport: McpTransportKind,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub keep_existing_secrets: bool,
    #[serde(default = "default_include_committed_true")]
    pub enabled: bool,
    /// Create-only intent: refuse to replace an existing server with the same name in the target
    /// scope. Defaults to `false` so older payloads keep the previous upsert semantics.
    #[serde(default)]
    pub create_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRulesSnapshotView {
    pub host_id: String,
    pub cwd: String,
    pub rules: Vec<orbit_protocol::PathScopedRule>,
    pub total_always_tokens: u32,
    pub token_budget_warning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertPathRuleRequest {
    pub host_id: String,
    pub cwd: String,
    pub scope: orbit_protocol::RuleScope,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub trigger_mode: orbit_protocol::RuleTriggerMode,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default = "default_include_committed_true")]
    pub enabled: bool,
    pub body: String,
    #[serde(default)]
    pub expected_etag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletePathRuleRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchPathRulesRequest {
    pub host_id: String,
    pub cwd: String,
    #[serde(default)]
    pub active_paths: Vec<String>,
    #[serde(default)]
    pub manual_rule_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchedPathRulesView {
    pub host_id: String,
    pub cwd: String,
    pub matched: Vec<orbit_protocol::MatchedRuleSummary>,
    pub injected_markdown: String,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleHooksSnapshotView {
    pub hooks: Vec<orbit_protocol::LifecycleHookRule>,
    pub recent_executions: Vec<orbit_protocol::HookExecutionRecord>,
    pub project_hooks_trusted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartGoalRunRequest {
    pub host_id: String,
    pub agent_id: String,
    pub objective: String,
    #[serde(default)]
    pub criteria: Vec<String>,
    #[serde(default)]
    pub verify_command: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub max_duration_secs: Option<u64>,
    #[serde(default)]
    pub replace_existing: bool,
    #[serde(default)]
    pub dispatch_initial_prompt: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlGoalRunRequest {
    pub host_id: String,
    pub goal_id: String,
    pub action: orbit_protocol::GoalControlAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateGoalRunRequest {
    pub host_id: String,
    pub goal_id: String,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub criteria: Option<Vec<String>>,
    #[serde(default)]
    pub verify_command: Option<Option<String>>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub max_duration_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartMonitorWatchRequest {
    pub host_id: String,
    pub agent_id: String,
    pub name: String,
    pub kind: orbit_protocol::MonitorKind,
    #[serde(default)]
    pub expires_in_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopMonitorWatchRequest {
    pub host_id: String,
    pub monitor_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTriggersSnapshotView {
    pub triggers: Vec<orbit_protocol::EventTriggerRecord>,
    pub recent_deliveries: Vec<orbit_protocol::EventTriggerDelivery>,
    pub base_webhook_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertEventTriggerRequest {
    pub host_id: String,
    #[serde(default)]
    pub trigger_id: Option<String>,
    pub name: String,
    pub source: orbit_protocol::TriggerSourceKind,
    #[serde(default)]
    pub event_filters: Vec<String>,
    pub prompt_template: String,
    pub target: orbit_protocol::TriggerDispatchTarget,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub keep_existing_secret: bool,
    #[serde(default)]
    pub github_poll: Option<orbit_protocol::GitHubPollConfig>,
    #[serde(default)]
    pub github_token: Option<String>,
    #[serde(default)]
    pub keep_existing_token: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSessionsSnapshotView {
    pub host_id: String,
    pub sessions: Vec<orbit_protocol::BrowserSessionSummary>,
    pub policy: orbit_protocol::BrowserPolicyConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chrome_binary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBrowserSessionRequest {
    pub host_id: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub initial_url: Option<String>,
    #[serde(default)]
    pub headless: Option<bool>,
    #[serde(default)]
    pub viewport_width: Option<u32>,
    #[serde(default)]
    pub viewport_height: Option<u32>,
    #[serde(default)]
    pub workspace_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteBrowserActionRequest {
    pub host_id: String,
    pub session_id: String,
    pub action: orbit_protocol::BrowserActionRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserActionResponseView {
    pub host_id: String,
    pub session: orbit_protocol::BrowserSessionSummary,
    #[serde(default)]
    pub elements: Vec<orbit_protocol::BrowserElementRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_excerpt: Option<String>,
    #[serde(default)]
    pub captcha_detected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picked_element: Option<orbit_protocol::PickedElementInfo>,
}

/// Active local loopback forwarder pointing to a running workspace service on a remote host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePreviewTunnelView {
    pub host_id: String,
    pub target_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_host: Option<String>,
    pub local_port: u16,
    pub local_url: String,
    pub is_tunneled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartBatchFanoutRequest {
    pub host_id: String,
    pub name: String,
    pub cwd: String,
    pub csv_content: String,
    pub prompt_template: String,
    pub provider: orbit_protocol::ProviderKind,
    #[serde(default)]
    pub model: Option<String>,
    pub mode: orbit_protocol::SessionMode,
    #[serde(default)]
    pub max_concurrency: Option<u32>,
    #[serde(default)]
    pub isolated_worktree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartDagWorkflowRequest {
    pub host_id: String,
    pub name: String,
    pub cwd: String,
    pub steps: Vec<orbit_protocol::DagStepSpec>,
    pub default_provider: orbit_protocol::ProviderKind,
    pub default_mode: orbit_protocol::SessionMode,
    #[serde(default)]
    pub max_concurrency: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartBestOfNRequest {
    pub host_id: String,
    pub name: String,
    pub cwd: String,
    pub prompt: String,
    pub candidates: Vec<orbit_protocol::BestOfNCandidateSpec>,
    pub mode: orbit_protocol::SessionMode,
    #[serde(default)]
    pub verify_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelWorkflowRunRequest {
    pub host_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectBestOfNWinnerRequest {
    pub host_id: String,
    pub run_id: String,
    pub winner_step_id: String,
    #[serde(default)]
    pub apply_to_workspace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditChainExportView {
    pub jsonl: String,
    #[serde(default)]
    pub next_after_seq: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertProjectMemoryRequest {
    pub host_id: String,
    pub cwd: String,
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    pub content: String,
    pub category: orbit_protocol::MemoryCategory,
    #[serde(default)]
    pub status: Option<orbit_protocol::MemoryStatus>,
    #[serde(default)]
    pub file_paths: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}
