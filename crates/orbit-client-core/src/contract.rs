//! Standardized Client Bridge Protocol v2 DTOs shared across:
//! - Tauri Desktop IPC (`apps/desktop/src-tauri`)
//! - Mobile UniFFI (`crates/orbit-ffi-uniffi`)
//! - HarmonyOS N-API (`crates/orbit-ffi-ohos`)

use orbit_protocol::{
    AcpAgentAvailability, ArtifactKind, ForkTarget, NestedGitRepo, PromptBadge, ProviderKind,
    ReasoningEffort, SessionMode, WorkspaceSummary, WorktreeHandoffDirection, WorktreeHandoffMode,
};
use serde::{Deserialize, Serialize};

use crate::HostRouteMode;
pub use crate::app_core::{AgentCardView, AskSideQuestionRequest};
pub use crate::diagram::{DiagramKind, DiagramTheme, RenderDiagramRequest, RenderDiagramView};
pub use crate::export::{
    ExportAgentMeta, ExportFormat, ExportOptions, ExportedTranscript, ExtractedCodeBlock,
};
use crate::sidebar::{SidebarProjectSort, SidebarThreadFilter};

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
    #[serde(default)]
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
pub struct RewindAgentRequest {
    pub host_id: String,
    pub agent_id: String,
    pub to_before_seq: u64,
    #[serde(default)]
    pub restore_files: bool,
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
    pub role: String,
    pub title: String,
    pub project_name: String,
    pub snippet: String,
    pub updated_at_rfc3339: String,
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
    pub content: String,
    #[serde(default)]
    pub summary_note: Option<String>,
}
