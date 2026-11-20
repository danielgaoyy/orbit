//! Standardized Client Bridge Protocol v2 DTOs shared across:
//! - Tauri Desktop IPC (`apps/desktop/src-tauri`)
//! - Mobile UniFFI (`crates/orbit-ffi-uniffi`)
//! - HarmonyOS N-API (`crates/orbit-ffi-ohos`)

use orbit_protocol::{PromptBadge, ProviderKind, ReasoningEffort, SessionMode};
use serde::{Deserialize, Serialize};

use crate::HostRouteMode;
pub use crate::app_core::AskSideQuestionRequest;
pub use crate::diagram::{DiagramKind, DiagramTheme, RenderDiagramRequest, RenderDiagramView};
use crate::sidebar::{SidebarProjectSort, SidebarThreadFilter};

pub const CLIENT_BRIDGE_PROTOCOL_VERSION: u32 = 2;

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
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<ReasoningEffort>,
    pub mode: SessionMode,
    pub initial_prompt: String,
    #[serde(default)]
    pub initial_display_prompt: Option<String>,
    #[serde(default)]
    pub initial_badges: Vec<PromptBadge>,
    #[serde(default)]
    pub worktree_branch: Option<String>,
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
pub struct ReadWorkspaceFileRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
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
}
