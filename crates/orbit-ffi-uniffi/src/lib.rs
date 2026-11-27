//! Strongly-typed UniFFI v0.32 native bridge for:
//! - iOS 16.4+ (`Swift` + `SwiftUI`)
//! - Android `minSdk 29` / `targetSdk 36` (`Kotlin 2` + `Jetpack Compose`)

mod mobile;

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread;

use orbit_client_core::app_core::{
    AgentCardView, AgentTimelineDeltaView, AgentTimelineView, AskSideQuestionRequest,
    ClientProjectView, ClientThreadPref, CoreEventSubscriber, CoreNotification, CreateAgentRequest,
    DirectoryListingView, FleetOverviewView, GitReviewView, HostCardView, HostDetailView,
    OrbitAppCore, PushDevicesSnapshotView, RegisterPushDeviceRequest, RewindAgentResultView,
    SideQuestionStreamEvent, StagedAttachmentView, TimelineItemView, WorkspaceFileView,
};
use orbit_client_core::catalog::{
    ResolvedModelSelectionView, default_effort_description as core_default_effort_description,
    fallback_provider_catalog as core_fallback_provider_catalog,
    format_catalog_model_label as core_format_catalog_model_label,
    format_effort_label as core_format_effort_label,
    resolve_supported_effort_for_model as core_resolve_supported_effort_for_model,
};
use orbit_client_core::cloud_sync::{
    CloudSyncDevicePollStatus, CloudSyncDevicePollView, CloudSyncResultView, CloudSyncState,
    CloudSyncStatusView, GitHubDeviceFlowStartView,
};
use orbit_client_core::composer::{
    ClientPlatform, ComposePromptRequest, ComposedPromptView, ComposerContextInput,
    ComposerPasteInput, ComposerQuoteInput, FastModeSlashAction, MentionCandidateView,
    SlashCommandView, SlashDirective,
};
use orbit_client_core::contract::{
    AgentStageDeltaRequest, BridgeInfoView, CancelWorkflowRunRequest, ControlGoalRunRequest,
    ConversationSearchHit, CreateSessionRequest, CreateWorkspaceEntryRequest,
    DeleteWorkspaceEntryRequest, ExecuteWorktreeHandoffRequest, FleetSidebarRequest,
    ForkAgentRequest, ForkAgentResultView, GitReposInspectionView, MentionRankRequest,
    ModelSelectionRequest, PreviewWorktreeHandoffRequest, ReadWorkspaceFileRequest,
    RenameWorkspaceEntryRequest, RenderDiagramRequest, RenderDiagramView, RewindAgentRequest,
    SendPromptRequest, SpawnWorkspacePeerRequest, StopMonitorWatchRequest, UpdateGoalRunRequest,
    UpdateHostConfigRequest, UpsertArtifactRequest, WorkspaceEntryMutationResultView,
    WorkspaceFileWriteResultView, WriteWorkspaceFileRequest,
};
use orbit_client_core::diagram::{
    DiagramKind, DiagramTheme, detect_diagram_kind as core_detect_diagram_kind,
};
use orbit_client_core::export::{
    ExportFormat, ExportOptions, ExportedTranscript, ExtractedCodeBlock,
};
use orbit_client_core::render::{
    DiffFileView, DiffLineKind, DiffLineView, HighlightedLineView, HighlightedTokenView,
    InlineSpanView, RichBlockView, TableAlignmentView, TableCellView, TableRowView,
    TerminalRowView, TerminalScreenView, TerminalSpanView,
    parse_markdown_blocks as core_parse_markdown_blocks,
};
use orbit_client_core::sidebar::{
    FleetSidebarView, LaunchpadProjectView, SidebarConversationView, SidebarProjectGroupView,
    SidebarProjectSort, SidebarThreadFilter, SidebarWorkspaceGroupView,
};
use orbit_client_core::stage::{
    AgentStageDeltaView, AgentStageView, SessionBackgroundTaskKind, SessionBackgroundTaskStatus,
    SessionBackgroundTaskView, SessionInsightsView, SessionSubagentEntryView,
    SessionTouchedFileAction, SessionTouchedFileView, StageTurnView, ToolStepDescriptorView,
    ToolStepKind, TurnCompactionItemView, TurnFileDiffSummaryView, TurnLiveSegmentView,
    TurnWorkCountsView, WorkSummaryResultView, describe_tool_detail, extract_thinking_headline,
};
use orbit_client_core::{HostConnectionState, HostRouteMode};
use orbit_protocol::{
    AcpAgentAvailability, AcpAgentSource, AcpAgentSpec, AcpProbeResult, AgentMessageDeliveryStatus,
    AgentMessageKind, ArtifactDetail, ArtifactKind, ArtifactRevisionMeta, ArtifactSummary,
    AttentionState, ConditionalTimerRecord, ConditionalTimerState, DirectoryEntry,
    DiscoveredProject, ForkTarget, GoalControlAction, GoalCriterion, GoalRunRecord, GoalStatus,
    HandoffFileChange, MonitorKind, MonitorStatus, MonitorWatchRecord, NestedGitRepo,
    PermissionCheckpointRef, PromptBadge, ProviderKind, ProviderModelCatalog, ProviderModelSpec,
    ProviderQuotaSnapshot, ProviderSubagentDescriptor, ProviderSubagentEvent,
    ProviderSubagentStatus, PushDeviceRecord, PushPlatform, PushPreferences, QuestionItem,
    QuotaWindow, ReasoningEffort, ScheduleKind, ScheduleNewAgentConfig, ScheduleRunRecord,
    ScheduleRunStatus, ScheduleSpec, ScheduleStatus, ScheduleTarget, SessionMode, TerminalSummary,
    ThinkingOptionSpec, TimelinePayload, TimerCondition, ToolCallDetail, ToolCallStatus,
    WorkflowKind, WorkflowRunRecord, WorkflowRunStatus, WorkflowStepRecord, WorkflowStepStatus,
    WorkspaceBoardSnapshot, WorkspaceClusterSuggestion, WorkspaceEventKind, WorkspaceEventRecord,
    WorkspaceFileConflict, WorkspaceIsolation, WorkspaceOrganizeEvent, WorkspaceOrganizePhase,
    WorkspaceSummary, WorktreeHandoffDirection, WorktreeHandoffMode, WorktreeHandoffOutcome,
    WorktreeHandoffPreview,
};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiOrbitError {
    #[error("{detail}")]
    Core { detail: String },
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiConversationSearchHit {
    pub host_id: String,
    pub host_name: String,
    pub agent_id: String,
    pub seq: u64,
    pub role: String,
    pub title: String,
    pub project_name: String,
    pub snippet: String,
    pub updated_at_rfc3339: String,
}

impl From<ConversationSearchHit> for FfiConversationSearchHit {
    fn from(v: ConversationSearchHit) -> Self {
        Self {
            host_id: v.host_id,
            host_name: v.host_name,
            agent_id: v.agent_id,
            seq: v.seq,
            role: v.role,
            title: v.title,
            project_name: v.project_name,
            snippet: v.snippet,
            updated_at_rfc3339: v.updated_at_rfc3339,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiExportFormat {
    Markdown,
    Json,
    Html,
}

impl From<FfiExportFormat> for ExportFormat {
    fn from(v: FfiExportFormat) -> Self {
        match v {
            FfiExportFormat::Markdown => Self::Markdown,
            FfiExportFormat::Json => Self::Json,
            FfiExportFormat::Html => Self::Html,
        }
    }
}

impl From<ExportFormat> for FfiExportFormat {
    fn from(v: ExportFormat) -> Self {
        match v {
            ExportFormat::Markdown => Self::Markdown,
            ExportFormat::Json => Self::Json,
            ExportFormat::Html => Self::Html,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiExportOptions {
    pub format: FfiExportFormat,
    pub include_thinking: bool,
    pub include_tool_calls: bool,
    pub include_timestamps: bool,
}

impl From<FfiExportOptions> for ExportOptions {
    fn from(v: FfiExportOptions) -> Self {
        Self {
            format: v.format.into(),
            include_thinking: v.include_thinking,
            include_tool_calls: v.include_tool_calls,
            include_timestamps: v.include_timestamps,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiExportedTranscript {
    pub suggested_filename: String,
    pub mime_type: String,
    pub content: String,
    pub turn_count: u64,
    pub byte_len: u64,
}

impl From<ExportedTranscript> for FfiExportedTranscript {
    fn from(v: ExportedTranscript) -> Self {
        Self {
            suggested_filename: v.suggested_filename,
            mime_type: v.mime_type,
            content: v.content,
            turn_count: v.turn_count as u64,
            byte_len: v.byte_len as u64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiExtractedCodeBlock {
    pub index: u64,
    pub turn_index: u64,
    pub language: String,
    pub preview: String,
    pub line_count: u64,
    pub code: String,
}

impl From<ExtractedCodeBlock> for FfiExtractedCodeBlock {
    fn from(v: ExtractedCodeBlock) -> Self {
        Self {
            index: v.index as u64,
            turn_index: v.turn_index as u64,
            language: v.language,
            preview: v.preview,
            line_count: v.line_count as u64,
            code: v.code,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiProviderKind {
    Antigravity,
    Codex,
    ClaudeCode,
    OpenCode,
    GeminiCli,
    CursorAgent,
    Copilot,
    Goose,
    Pi,
    Kimi,
    Cline,
    MistralVibe,
    Augment,
    Acp,
}

impl From<ProviderKind> for FfiProviderKind {
    fn from(v: ProviderKind) -> Self {
        match v {
            ProviderKind::Antigravity => Self::Antigravity,
            ProviderKind::Codex => Self::Codex,
            ProviderKind::ClaudeCode => Self::ClaudeCode,
            ProviderKind::OpenCode => Self::OpenCode,
            ProviderKind::GeminiCli => Self::GeminiCli,
            ProviderKind::CursorAgent => Self::CursorAgent,
            ProviderKind::Copilot => Self::Copilot,
            ProviderKind::Goose => Self::Goose,
            ProviderKind::Pi => Self::Pi,
            ProviderKind::Kimi => Self::Kimi,
            ProviderKind::Cline => Self::Cline,
            ProviderKind::MistralVibe => Self::MistralVibe,
            ProviderKind::Augment => Self::Augment,
            ProviderKind::Acp => Self::Acp,
        }
    }
}

impl From<FfiProviderKind> for ProviderKind {
    fn from(v: FfiProviderKind) -> Self {
        match v {
            FfiProviderKind::Antigravity => Self::Antigravity,
            FfiProviderKind::Codex => Self::Codex,
            FfiProviderKind::ClaudeCode => Self::ClaudeCode,
            FfiProviderKind::OpenCode => Self::OpenCode,
            FfiProviderKind::GeminiCli => Self::GeminiCli,
            FfiProviderKind::CursorAgent => Self::CursorAgent,
            FfiProviderKind::Copilot => Self::Copilot,
            FfiProviderKind::Goose => Self::Goose,
            FfiProviderKind::Pi => Self::Pi,
            FfiProviderKind::Kimi => Self::Kimi,
            FfiProviderKind::Cline => Self::Cline,
            FfiProviderKind::MistralVibe => Self::MistralVibe,
            FfiProviderKind::Augment => Self::Augment,
            FfiProviderKind::Acp => Self::Acp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiAcpAgentSource {
    BuiltinCatalog,
    RemoteRegistry,
    Custom,
}

impl From<AcpAgentSource> for FfiAcpAgentSource {
    fn from(v: AcpAgentSource) -> Self {
        match v {
            AcpAgentSource::BuiltinCatalog => Self::BuiltinCatalog,
            AcpAgentSource::RemoteRegistry => Self::RemoteRegistry,
            AcpAgentSource::Custom => Self::Custom,
        }
    }
}

impl From<FfiAcpAgentSource> for AcpAgentSource {
    fn from(v: FfiAcpAgentSource) -> Self {
        match v {
            FfiAcpAgentSource::BuiltinCatalog => Self::BuiltinCatalog,
            FfiAcpAgentSource::RemoteRegistry => Self::RemoteRegistry,
            FfiAcpAgentSource::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAcpAgentSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub install_hint: Option<String>,
    pub homepage: Option<String>,
    pub version: Option<String>,
    pub source: FfiAcpAgentSource,
    pub enabled: bool,
}

impl From<AcpAgentSpec> for FfiAcpAgentSpec {
    fn from(v: AcpAgentSpec) -> Self {
        Self {
            id: v.id,
            name: v.name,
            description: v.description,
            command: v.command,
            args: v.args,
            env: v.env,
            install_hint: v.install_hint,
            homepage: v.homepage,
            version: v.version,
            source: v.source.into(),
            enabled: v.enabled,
        }
    }
}

impl From<FfiAcpAgentSpec> for AcpAgentSpec {
    fn from(v: FfiAcpAgentSpec) -> Self {
        Self {
            id: v.id,
            name: v.name,
            description: v.description,
            command: v.command,
            args: v.args,
            env: v.env,
            install_hint: v.install_hint,
            homepage: v.homepage,
            version: v.version,
            source: v.source.into(),
            enabled: v.enabled,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAcpAgentAvailability {
    pub spec: FfiAcpAgentSpec,
    pub installed: bool,
    pub resolved_path: Option<String>,
}

impl From<AcpAgentAvailability> for FfiAcpAgentAvailability {
    fn from(v: AcpAgentAvailability) -> Self {
        Self {
            spec: v.spec.into(),
            installed: v.installed,
            resolved_path: v.resolved_path,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAcpProbeResult {
    pub agent_id: String,
    pub ok: bool,
    pub protocol_version: Option<String>,
    pub server_name: Option<String>,
    pub server_version: Option<String>,
    pub capabilities_summary: Vec<String>,
    pub auth_methods: Vec<String>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

impl From<AcpProbeResult> for FfiAcpProbeResult {
    fn from(v: AcpProbeResult) -> Self {
        Self {
            agent_id: v.agent_id,
            ok: v.ok,
            protocol_version: v.protocol_version,
            server_name: v.server_name,
            server_version: v.server_version,
            capabilities_summary: v.capabilities_summary,
            auth_methods: v.auth_methods,
            latency_ms: v.latency_ms,
            error: v.error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiReasoningEffort {
    Off,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Ultra,
}

impl From<ReasoningEffort> for FfiReasoningEffort {
    fn from(v: ReasoningEffort) -> Self {
        match v {
            ReasoningEffort::Off => Self::Off,
            ReasoningEffort::Low => Self::Low,
            ReasoningEffort::Medium => Self::Medium,
            ReasoningEffort::High => Self::High,
            ReasoningEffort::Xhigh => Self::Xhigh,
            ReasoningEffort::Max => Self::Max,
            ReasoningEffort::Ultra => Self::Ultra,
        }
    }
}

impl From<FfiReasoningEffort> for ReasoningEffort {
    fn from(v: FfiReasoningEffort) -> Self {
        match v {
            FfiReasoningEffort::Off => Self::Off,
            FfiReasoningEffort::Low => Self::Low,
            FfiReasoningEffort::Medium => Self::Medium,
            FfiReasoningEffort::High => Self::High,
            FfiReasoningEffort::Xhigh => Self::Xhigh,
            FfiReasoningEffort::Max => Self::Max,
            FfiReasoningEffort::Ultra => Self::Ultra,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum FfiSessionMode {
    Default,
    Plan,
    AcceptEdits,
    FullAccess,
    Custom { name: String },
    Auto,
}

impl From<SessionMode> for FfiSessionMode {
    fn from(v: SessionMode) -> Self {
        match v {
            SessionMode::Default => Self::Default,
            SessionMode::Auto => Self::Auto,
            SessionMode::Plan => Self::Plan,
            SessionMode::AcceptEdits => Self::AcceptEdits,
            SessionMode::FullAccess => Self::FullAccess,
            SessionMode::Custom(name) => Self::Custom { name },
        }
    }
}

impl From<FfiSessionMode> for SessionMode {
    fn from(v: FfiSessionMode) -> Self {
        match v {
            FfiSessionMode::Default => Self::Default,
            FfiSessionMode::Auto => Self::Auto,
            FfiSessionMode::Plan => Self::Plan,
            FfiSessionMode::AcceptEdits => Self::AcceptEdits,
            FfiSessionMode::FullAccess => Self::FullAccess,
            FfiSessionMode::Custom { name } => Self::Custom(name),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiHostRouteMode {
    Auto,
    SshTunnel,
    DirectOnly,
    RelayOnly,
}

impl From<HostRouteMode> for FfiHostRouteMode {
    fn from(v: HostRouteMode) -> Self {
        match v {
            HostRouteMode::Auto => Self::Auto,
            HostRouteMode::SshTunnel => Self::SshTunnel,
            HostRouteMode::DirectOnly => Self::DirectOnly,
            HostRouteMode::RelayOnly => Self::RelayOnly,
        }
    }
}

impl From<FfiHostRouteMode> for HostRouteMode {
    fn from(v: FfiHostRouteMode) -> Self {
        match v {
            FfiHostRouteMode::Auto => Self::Auto,
            FfiHostRouteMode::SshTunnel => Self::SshTunnel,
            FfiHostRouteMode::DirectOnly => Self::DirectOnly,
            FfiHostRouteMode::RelayOnly => Self::RelayOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiConnectionState {
    Disconnected,
    ConnectingDirect,
    ConnectingRelay,
    OnlineDirect,
    OnlineRelay,
}

impl From<HostConnectionState> for FfiConnectionState {
    fn from(v: HostConnectionState) -> Self {
        match v {
            HostConnectionState::Disconnected => Self::Disconnected,
            HostConnectionState::ConnectingDirect => Self::ConnectingDirect,
            HostConnectionState::ConnectingRelay => Self::ConnectingRelay,
            HostConnectionState::OnlineDirect => Self::OnlineDirect,
            HostConnectionState::OnlineRelay => Self::OnlineRelay,
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiCoreEvent {
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

#[uniffi::export(with_foreign)]
pub trait MobileCoreListener: Send + Sync {
    fn on_core_event(&self, event: FfiCoreEvent);
}

struct BridgeSubscriber {
    foreign: Arc<dyn MobileCoreListener>,
}

impl CoreEventSubscriber for BridgeSubscriber {
    fn on_notification(&self, event: CoreNotification) {
        let mapped = match event {
            CoreNotification::FleetChanged { revision } => FfiCoreEvent::FleetChanged { revision },
            CoreNotification::HostChanged { revision, host_id } => {
                FfiCoreEvent::HostChanged { revision, host_id }
            }
            CoreNotification::TimelineChanged {
                revision,
                host_id,
                agent_id,
            } => FfiCoreEvent::TimelineChanged {
                revision,
                host_id,
                agent_id,
            },
            CoreNotification::GitChanged {
                revision,
                host_id,
                cwd,
            } => FfiCoreEvent::GitChanged {
                revision,
                host_id,
                cwd,
            },
            CoreNotification::TerminalChanged {
                revision,
                host_id,
                slot,
                ..
            } => FfiCoreEvent::TerminalChanged {
                revision,
                host_id,
                slot,
            },
        };
        self.foreign.on_core_event(mapped);
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiClientProject {
    pub project_id: String,
    pub host_id: String,
    pub name: String,
    pub directories: Vec<String>,
    pub updated_at_ms: i64,
}

impl From<ClientProjectView> for FfiClientProject {
    fn from(v: ClientProjectView) -> Self {
        Self {
            project_id: v.project_id,
            host_id: v.host_id,
            name: v.name,
            directories: v.directories,
            updated_at_ms: v.updated_at_ms,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiHostCard {
    pub host_id: String,
    pub display_name: String,
    pub hostname: String,
    pub custom_name: Option<String>,
    pub ssh_alias: Option<String>,
    pub route_mode: FfiHostRouteMode,
    pub remote_daemon_port: u16,
    pub custom_direct_endpoint: Option<String>,
    pub via_ssh_tunnel: bool,
    pub last_ping_ms: Option<u32>,
    pub daemon_version: Option<String>,
    pub daemon_protocol_version: Option<u32>,
    pub client_protocol_version: u32,
    pub os_arch: Option<String>,
    pub daemon_pubkey_b64: String,
    pub direct_endpoints: Vec<String>,
    pub relay_url: Option<String>,
    pub connection_state: FfiConnectionState,
    pub last_error: Option<String>,
    pub available_providers: Vec<FfiProviderKind>,
    pub workspace_count: u32,
    pub agent_count: u32,
    pub pending_approval_count: u32,
    pub supports_server_queue: bool,
    pub is_local: bool,
}

impl From<HostCardView> for FfiHostCard {
    fn from(v: HostCardView) -> Self {
        Self {
            host_id: v.host_id,
            display_name: v.display_name,
            hostname: v.hostname,
            custom_name: v.custom_name,
            ssh_alias: v.ssh_alias,
            route_mode: v.route_mode.into(),
            remote_daemon_port: v.remote_daemon_port,
            custom_direct_endpoint: v.custom_direct_endpoint,
            via_ssh_tunnel: v.via_ssh_tunnel,
            last_ping_ms: v.last_ping_ms,
            daemon_version: v.daemon_version,
            daemon_protocol_version: v.daemon_protocol_version,
            client_protocol_version: v.client_protocol_version,
            os_arch: v.os_arch,
            daemon_pubkey_b64: v.daemon_pubkey_b64,
            direct_endpoints: v.direct_endpoints,
            relay_url: v.relay_url,
            connection_state: v.connection_state.into(),
            last_error: v.last_error,
            available_providers: v.available_providers.into_iter().map(Into::into).collect(),
            workspace_count: v.workspace_count,
            agent_count: v.agent_count,
            pending_approval_count: v.pending_approval_count,
            supports_server_queue: v.supports_server_queue,
            is_local: v.is_local,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiFleetOverview {
    pub revision: u64,
    pub total_pending_approvals: u32,
    pub hosts: Vec<FfiHostCard>,
    pub projects: Vec<FfiClientProject>,
    pub project_order: Vec<String>,
}

impl From<FleetOverviewView> for FfiFleetOverview {
    fn from(v: FleetOverviewView) -> Self {
        Self {
            revision: v.revision,
            total_pending_approvals: v.total_pending_approvals,
            hosts: v.hosts.into_iter().map(Into::into).collect(),
            projects: v.projects.into_iter().map(Into::into).collect(),
            project_order: v.project_order,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiNestedGitRepo {
    pub path: String,
    pub relative_path: String,
    pub name: String,
    pub git_branch: Option<String>,
}

impl From<NestedGitRepo> for FfiNestedGitRepo {
    fn from(v: NestedGitRepo) -> Self {
        Self {
            path: v.path,
            relative_path: v.relative_path,
            name: v.name,
            git_branch: v.git_branch,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiGitReposInspection {
    pub path: String,
    pub is_git_repo: bool,
    pub git_branch: Option<String>,
    pub nested_git_repos: Vec<FfiNestedGitRepo>,
}

impl From<GitReposInspectionView> for FfiGitReposInspection {
    fn from(v: GitReposInspectionView) -> Self {
        Self {
            path: v.path,
            is_git_repo: v.is_git_repo,
            git_branch: v.git_branch,
            nested_git_repos: v.nested_git_repos.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiDiscoveredProject {
    pub path: String,
    pub name: String,
    pub git_branch: Option<String>,
    pub is_git_repo: bool,
    pub last_modified_unix: i64,
}

impl From<DiscoveredProject> for FfiDiscoveredProject {
    fn from(v: DiscoveredProject) -> Self {
        Self {
            path: v.path,
            name: v.name,
            git_branch: v.git_branch,
            is_git_repo: v.is_git_repo,
            last_modified_unix: v.last_modified_unix,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiDirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_git_repo: bool,
}

impl From<DirectoryEntry> for FfiDirectoryEntry {
    fn from(v: DirectoryEntry) -> Self {
        Self {
            name: v.name,
            path: v.path,
            is_git_repo: v.is_git_repo,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiDirectoryListing {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub entries: Vec<FfiDirectoryEntry>,
}

impl From<DirectoryListingView> for FfiDirectoryListing {
    fn from(v: DirectoryListingView) -> Self {
        Self {
            current_path: v.current_path,
            parent_path: v.parent_path,
            entries: v.entries.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiThinkingOptionSpec {
    pub effort: FfiReasoningEffort,
    pub label: String,
    pub description: Option<String>,
    pub is_default: bool,
}

impl From<ThinkingOptionSpec> for FfiThinkingOptionSpec {
    fn from(v: ThinkingOptionSpec) -> Self {
        Self {
            effort: v.effort.into(),
            label: v.label,
            description: v.description,
            is_default: v.is_default,
        }
    }
}

impl From<FfiThinkingOptionSpec> for ThinkingOptionSpec {
    fn from(v: FfiThinkingOptionSpec) -> Self {
        Self {
            effort: v.effort.into(),
            label: v.label,
            description: v.description,
            is_default: v.is_default,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiProviderModelSpec {
    pub id: String,
    pub label: String,
    pub description: String,
    pub is_default: bool,
    pub default_effort: Option<FfiReasoningEffort>,
    pub thinking_options: Vec<FfiThinkingOptionSpec>,
    pub supports_fast_mode: bool,
}

impl From<ProviderModelSpec> for FfiProviderModelSpec {
    fn from(v: ProviderModelSpec) -> Self {
        Self {
            id: v.id,
            label: v.label,
            description: v.description,
            is_default: v.is_default,
            default_effort: v.default_effort.map(Into::into),
            thinking_options: v.thinking_options.into_iter().map(Into::into).collect(),
            supports_fast_mode: v.supports_fast_mode,
        }
    }
}

impl From<FfiProviderModelSpec> for ProviderModelSpec {
    fn from(v: FfiProviderModelSpec) -> Self {
        Self {
            id: v.id,
            label: v.label,
            description: v.description,
            is_default: v.is_default,
            default_effort: v.default_effort.map(Into::into),
            thinking_options: v.thinking_options.into_iter().map(Into::into).collect(),
            supports_fast_mode: v.supports_fast_mode,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiProviderModelCatalog {
    pub provider: FfiProviderKind,
    pub models: Vec<FfiProviderModelSpec>,
    pub configured_default_model: Option<String>,
    pub configured_default_effort: Option<FfiReasoningEffort>,
}

impl From<ProviderModelCatalog> for FfiProviderModelCatalog {
    fn from(v: ProviderModelCatalog) -> Self {
        Self {
            provider: v.provider.into(),
            models: v.models.into_iter().map(Into::into).collect(),
            configured_default_model: v.configured_default_model,
            configured_default_effort: v.configured_default_effort.map(Into::into),
        }
    }
}

impl From<FfiProviderModelCatalog> for ProviderModelCatalog {
    fn from(v: FfiProviderModelCatalog) -> Self {
        Self {
            provider: v.provider.into(),
            models: v.models.into_iter().map(Into::into).collect(),
            configured_default_model: v.configured_default_model,
            configured_default_effort: v.configured_default_effort.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTerminalSummary {
    pub terminal_id: String,
    pub cwd: String,
    pub title: String,
    pub cols: u16,
    pub rows: u16,
    pub created_at_rfc3339: String,
}

impl From<TerminalSummary> for FfiTerminalSummary {
    fn from(v: TerminalSummary) -> Self {
        Self {
            terminal_id: v.terminal_id,
            cwd: v.cwd,
            title: v.title,
            cols: v.cols,
            rows: v.rows,
            created_at_rfc3339: v.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkspaceSummary {
    pub workspace_id: String,
    pub title: String,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub is_worktree: bool,
    pub mission_goal: Option<String>,
    pub project_root: Option<String>,
    pub source_cwd: Option<String>,
    pub board_revision: u64,
    pub is_curated: bool,
    pub updated_at_rfc3339: String,
}

impl From<WorkspaceSummary> for FfiWorkspaceSummary {
    fn from(v: WorkspaceSummary) -> Self {
        Self {
            workspace_id: v.workspace_id,
            title: v.title,
            cwd: v.cwd,
            git_branch: v.git_branch,
            is_worktree: v.is_worktree,
            mission_goal: v.mission_goal,
            project_root: v.project_root,
            source_cwd: v.source_cwd,
            board_revision: v.board_revision,
            is_curated: v.is_curated,
            updated_at_rfc3339: v.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkspaceClusterSuggestion {
    pub target_workspace_id: String,
    pub target_workspace_title: String,
    pub confidence: f32,
    pub reason: String,
    pub auto_applied: bool,
    pub previous_workspace_id: Option<String>,
}

impl From<WorkspaceClusterSuggestion> for FfiWorkspaceClusterSuggestion {
    fn from(v: WorkspaceClusterSuggestion) -> Self {
        Self {
            target_workspace_id: v.target_workspace_id,
            target_workspace_title: v.target_workspace_title,
            confidence: v.confidence,
            reason: v.reason,
            auto_applied: v.auto_applied,
            previous_workspace_id: v.previous_workspace_id,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkspaceBoardSnapshot {
    pub workspace_id: String,
    pub content: String,
    pub revision: u64,
    pub updated_by: Option<String>,
    pub updated_at_rfc3339: String,
    pub board_path: Option<String>,
}

impl From<WorkspaceBoardSnapshot> for FfiWorkspaceBoardSnapshot {
    fn from(v: WorkspaceBoardSnapshot) -> Self {
        Self {
            workspace_id: v.workspace_id,
            content: v.content,
            revision: v.revision,
            updated_by: v.updated_by,
            updated_at_rfc3339: v.updated_at.to_rfc3339(),
            board_path: v.board_path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiWorkspaceEventKind {
    PeerJoined,
    PeerFinished,
    BoardUpdated,
    PeerMessage,
    FileEdited,
    ConflictWarning,
    WorkspaceMerged,
}

impl From<WorkspaceEventKind> for FfiWorkspaceEventKind {
    fn from(v: WorkspaceEventKind) -> Self {
        match v {
            WorkspaceEventKind::PeerJoined => Self::PeerJoined,
            WorkspaceEventKind::PeerFinished => Self::PeerFinished,
            WorkspaceEventKind::BoardUpdated => Self::BoardUpdated,
            WorkspaceEventKind::PeerMessage => Self::PeerMessage,
            WorkspaceEventKind::FileEdited => Self::FileEdited,
            WorkspaceEventKind::ConflictWarning => Self::ConflictWarning,
            WorkspaceEventKind::WorkspaceMerged => Self::WorkspaceMerged,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkspaceEvent {
    pub seq: u64,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub agent_handle: Option<String>,
    pub kind: FfiWorkspaceEventKind,
    pub summary: String,
    pub file_path: Option<String>,
    pub created_at_rfc3339: String,
}

impl From<WorkspaceEventRecord> for FfiWorkspaceEvent {
    fn from(v: WorkspaceEventRecord) -> Self {
        Self {
            seq: v.seq,
            workspace_id: v.workspace_id,
            agent_id: v.agent_id,
            agent_handle: v.agent_handle,
            kind: v.kind.into(),
            summary: v.summary,
            file_path: v.file_path,
            created_at_rfc3339: v.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiWorkspaceOrganizePhase {
    Running,
    Completed,
    Failed,
}

impl From<WorkspaceOrganizePhase> for FfiWorkspaceOrganizePhase {
    fn from(v: WorkspaceOrganizePhase) -> Self {
        match v {
            WorkspaceOrganizePhase::Running => Self::Running,
            WorkspaceOrganizePhase::Completed => Self::Completed,
            WorkspaceOrganizePhase::Failed => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkspaceOrganizeStatus {
    pub project_root: String,
    pub phase: FfiWorkspaceOrganizePhase,
    pub message: String,
    pub created_workspaces: u32,
    pub grouped_agents: u32,
    pub summarized_agents: u32,
}

impl From<WorkspaceOrganizeEvent> for FfiWorkspaceOrganizeStatus {
    fn from(v: WorkspaceOrganizeEvent) -> Self {
        Self {
            project_root: v.project_root,
            phase: v.phase.into(),
            message: v.message,
            created_workspaces: v.created_workspaces,
            grouped_agents: v.grouped_agents,
            summarized_agents: v.summarized_agents,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiProviderSubagentStatus {
    Running,
    Completed,
    Failed,
    Canceled,
}

impl From<ProviderSubagentStatus> for FfiProviderSubagentStatus {
    fn from(v: ProviderSubagentStatus) -> Self {
        match v {
            ProviderSubagentStatus::Running => Self::Running,
            ProviderSubagentStatus::Completed => Self::Completed,
            ProviderSubagentStatus::Failed => Self::Failed,
            ProviderSubagentStatus::Canceled => Self::Canceled,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiProviderSubagentDescriptor {
    pub subagent_id: String,
    pub parent_agent_id: String,
    pub parent_subagent_id: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub status: FfiProviderSubagentStatus,
    pub created_at_rfc3339: String,
    pub updated_at_rfc3339: String,
}

impl From<ProviderSubagentDescriptor> for FfiProviderSubagentDescriptor {
    fn from(v: ProviderSubagentDescriptor) -> Self {
        Self {
            subagent_id: v.subagent_id,
            parent_agent_id: v.parent_agent_id,
            parent_subagent_id: v.parent_subagent_id,
            title: v.title,
            subtitle: v.subtitle,
            status: v.status.into(),
            created_at_rfc3339: v.created_at.to_rfc3339(),
            updated_at_rfc3339: v.updated_at.to_rfc3339(),
        }
    }
}

fn summarize_provider_subagent_payload(payload: &TimelinePayload) -> String {
    match payload {
        TimelinePayload::UserPrompt { text, .. } => text.trim().chars().take(280).collect(),
        TimelinePayload::AssistantThinking { text, .. } => {
            format!(
                "Thinking: {}",
                text.trim().chars().take(240).collect::<String>()
            )
        }
        TimelinePayload::AssistantMessage { text, .. } => text.trim().chars().take(280).collect(),
        TimelinePayload::ToolCall { detail, .. } => match detail {
            ToolCallDetail::Shell { command, .. } => format!("$ {command}"),
            ToolCallDetail::FileRead { path, .. } => format!("Read {path}"),
            ToolCallDetail::FileEdit { path, .. } => format!("Edit {path}"),
            ToolCallDetail::Search { query, .. } => format!("Search {query}"),
            ToolCallDetail::Subagent { role, .. } => format!("Subagent {role}"),
            ToolCallDetail::Generic { tool_name, .. } => format!("Tool {tool_name}"),
        },
        TimelinePayload::PermissionRequest { title, .. } => format!("Permission: {title}"),
        TimelinePayload::PermissionResolved { decision, .. } => format!("Decision: {decision}"),
        TimelinePayload::QuestionRequest { questions, .. } => questions
            .first()
            .map(|q| q.question.clone())
            .unwrap_or_else(|| "Question requested".into()),
        TimelinePayload::QuestionResolved { .. } => "Answered question".into(),
        TimelinePayload::Notice { message, .. } => message.trim().chars().take(240).collect(),
        TimelinePayload::AgentMessage { text, .. } => text.trim().chars().take(280).collect(),
        TimelinePayload::AgentMessageDeliveryUpdated { status, .. } => {
            format!("Delivery: {status:?}")
        }
        TimelinePayload::StatusChanged { status } => format!("Status: {status:?}"),
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiProviderSubagentEvent {
    pub seq: u64,
    pub parent_agent_id: String,
    pub subagent_id: String,
    pub timestamp_rfc3339: String,
    pub summary_text: String,
}

impl From<ProviderSubagentEvent> for FfiProviderSubagentEvent {
    fn from(v: ProviderSubagentEvent) -> Self {
        let summary_text = summarize_provider_subagent_payload(&v.payload);
        Self {
            seq: v.seq,
            parent_agent_id: v.parent_agent_id,
            subagent_id: v.subagent_id,
            timestamp_rfc3339: v.timestamp.to_rfc3339(),
            summary_text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiScheduleKind {
    Schedule,
    Heartbeat,
}

impl From<ScheduleKind> for FfiScheduleKind {
    fn from(v: ScheduleKind) -> Self {
        match v {
            ScheduleKind::Schedule => Self::Schedule,
            ScheduleKind::Heartbeat => Self::Heartbeat,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiScheduleStatus {
    Active,
    Paused,
    Completed,
}

impl From<ScheduleStatus> for FfiScheduleStatus {
    fn from(v: ScheduleStatus) -> Self {
        match v {
            ScheduleStatus::Active => Self::Active,
            ScheduleStatus::Paused => Self::Paused,
            ScheduleStatus::Completed => Self::Completed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiScheduleRunStatus {
    Running,
    Succeeded,
    Failed,
    Skipped,
}

impl From<ScheduleRunStatus> for FfiScheduleRunStatus {
    fn from(v: ScheduleRunStatus) -> Self {
        match v {
            ScheduleRunStatus::Running => Self::Running,
            ScheduleRunStatus::Succeeded => Self::Succeeded,
            ScheduleRunStatus::Failed => Self::Failed,
            ScheduleRunStatus::Skipped => Self::Skipped,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiScheduleSpec {
    pub schedule_id: String,
    pub kind: FfiScheduleKind,
    pub name: String,
    pub cron_expr: String,
    pub timezone: Option<String>,
    pub prompt: String,
    pub status: FfiScheduleStatus,
    pub cwd: Option<String>,
    pub workspace_id: Option<String>,
    pub provider: Option<FfiProviderKind>,
    pub mode: Option<FfiSessionMode>,
    pub worktree: bool,
    pub archive_on_finish: bool,
    pub target_agent_id: Option<String>,
    pub max_runs: Option<u32>,
    pub run_count: u32,
    pub last_run_at_rfc3339: Option<String>,
    pub next_run_at_rfc3339: Option<String>,
}

impl From<ScheduleSpec> for FfiScheduleSpec {
    fn from(v: ScheduleSpec) -> Self {
        let (cwd, workspace_id, provider, mode, worktree, archive_on_finish, target_agent_id) =
            match v.target {
                ScheduleTarget::NewAgent(cfg) => (
                    Some(cfg.cwd),
                    cfg.workspace_id,
                    Some(cfg.provider.into()),
                    Some(cfg.mode.into()),
                    cfg.worktree,
                    cfg.archive_on_finish,
                    None,
                ),
                ScheduleTarget::ExistingAgent { agent_id } => {
                    (None, None, None, None, false, false, Some(agent_id))
                }
            };
        Self {
            schedule_id: v.schedule_id,
            kind: v.kind.into(),
            name: v.name,
            cron_expr: v.cron_expr,
            timezone: v.timezone,
            prompt: v.prompt,
            status: v.status.into(),
            cwd,
            workspace_id,
            provider,
            mode,
            worktree,
            archive_on_finish,
            target_agent_id,
            max_runs: v.max_runs,
            run_count: v.run_count,
            last_run_at_rfc3339: v.last_run_at.map(|d| d.to_rfc3339()),
            next_run_at_rfc3339: v.next_run_at.map(|d| d.to_rfc3339()),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiScheduleRunRecord {
    pub run_id: String,
    pub schedule_id: String,
    pub agent_id: Option<String>,
    pub workspace_id: Option<String>,
    pub status: FfiScheduleRunStatus,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub started_at_rfc3339: String,
    pub finished_at_rfc3339: Option<String>,
}

impl From<ScheduleRunRecord> for FfiScheduleRunRecord {
    fn from(v: ScheduleRunRecord) -> Self {
        Self {
            run_id: v.run_id,
            schedule_id: v.schedule_id,
            agent_id: v.agent_id,
            workspace_id: v.workspace_id,
            status: v.status.into(),
            summary: v.summary,
            error: v.error,
            started_at_rfc3339: v.started_at.to_rfc3339(),
            finished_at_rfc3339: v.finished_at.map(|d| d.to_rfc3339()),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiQueuedPrompt {
    pub queue_id: String,
    pub display_text: String,
    pub badges: Vec<FfiPromptBadge>,
    pub created_at_rfc3339: String,
}

impl From<orbit_protocol::QueuedPromptView> for FfiQueuedPrompt {
    fn from(q: orbit_protocol::QueuedPromptView) -> Self {
        Self {
            queue_id: q.queue_id,
            display_text: q.display_text,
            badges: q.badges.into_iter().map(Into::into).collect(),
            created_at_rfc3339: q.created_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAgentCard {
    pub agent_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub title: String,
    pub provider: FfiProviderKind,
    pub provider_label: String,
    pub model: Option<String>,
    pub effort: Option<FfiReasoningEffort>,
    pub fast_mode: bool,
    pub mode: FfiSessionMode,
    pub status_label: String,
    pub is_running: bool,
    pub has_background_tasks: bool,
    pub has_armed_timers: bool,
    pub is_awaiting_permission: bool,
    pub parent_agent_id: Option<String>,
    pub parent_host_id: Option<String>,
    pub archived_at_rfc3339: Option<String>,
    pub handle: Option<String>,
    pub cluster_suggestion: Option<FfiWorkspaceClusterSuggestion>,
    pub worktree_branch: Option<String>,
    pub task_summary: Option<String>,
    pub forked_from_agent_id: Option<String>,
    pub forked_at_seq: Option<u64>,
    pub acp_agent_id: Option<String>,
    pub attention_label: Option<String>,
    pub latest_seq: u64,
    pub created_at_rfc3339: Option<String>,
    pub updated_at_rfc3339: String,
    pub is_pinned: bool,
    pub seen_seq: u64,
    pub is_unread: bool,
    pub queued_prompts: Vec<FfiQueuedPrompt>,
    pub labels: std::collections::HashMap<String, String>,
    pub is_personal_router: bool,
}

impl From<AgentCardView> for FfiAgentCard {
    fn from(v: AgentCardView) -> Self {
        let attention_label = match v.attention {
            AttentionState::None => None,
            AttentionState::Finished => Some("finished".into()),
            AttentionState::Error => Some("error".into()),
            AttentionState::Permission => Some("permission".into()),
            AttentionState::ManualUnread => Some("manual_unread".into()),
        };
        let is_personal_router = v
            .labels
            .get(orbit_protocol::LABEL_AGENT_ROLE)
            .map(|r| r.trim().to_ascii_lowercase())
            .is_some_and(|r| matches!(r.as_str(), "personal_router" | "router" | "chief_of_staff"))
            || v.handle
                .as_deref()
                .is_some_and(|h| h.trim().eq_ignore_ascii_case("router"));
        Self {
            agent_id: v.agent_id,
            workspace_id: v.workspace_id,
            cwd: v.cwd,
            title: v.title,
            provider: v.provider.into(),
            provider_label: v.provider_label,
            model: v.model,
            effort: v.effort.map(Into::into),
            fast_mode: v.fast_mode,
            mode: v.mode.into(),
            status_label: v.status_label,
            is_running: v.is_running,
            has_background_tasks: v.has_background_tasks,
            has_armed_timers: v.has_armed_timers,
            is_awaiting_permission: v.is_awaiting_permission,
            parent_agent_id: v.parent_agent_id,
            parent_host_id: v.parent_host_id,
            archived_at_rfc3339: v.archived_at_rfc3339,
            handle: v.handle,
            cluster_suggestion: v.cluster_suggestion.map(Into::into),
            worktree_branch: v.worktree_branch,
            task_summary: v.task_summary,
            forked_from_agent_id: v.forked_from_agent_id,
            forked_at_seq: v.forked_at_seq,
            acp_agent_id: v.acp_agent_id,
            attention_label,
            latest_seq: v.latest_seq,
            created_at_rfc3339: v.created_at_rfc3339,
            updated_at_rfc3339: v.updated_at_rfc3339,
            is_pinned: v.is_pinned,
            seen_seq: v.seen_seq,
            is_unread: v.is_unread,
            queued_prompts: v.queued_prompts.into_iter().map(Into::into).collect(),
            labels: v.labels,
            is_personal_router,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiQuotaWindow {
    pub label: String,
    pub used_percent: f32,
    pub resets_at_rfc3339: Option<String>,
}

impl From<QuotaWindow> for FfiQuotaWindow {
    fn from(w: QuotaWindow) -> Self {
        Self {
            label: w.label,
            used_percent: w.used_percent,
            resets_at_rfc3339: w.resets_at.map(|d| d.to_rfc3339()),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiProviderQuota {
    pub provider: FfiProviderKind,
    pub provider_label: String,
    pub account_label: Option<String>,
    pub windows: Vec<FfiQuotaWindow>,
    pub credits_remaining: Option<f64>,
    pub error: Option<String>,
}

impl From<ProviderQuotaSnapshot> for FfiProviderQuota {
    fn from(s: ProviderQuotaSnapshot) -> Self {
        let provider_label = s.provider.display_name().to_string();
        Self {
            provider: s.provider.into(),
            provider_label,
            account_label: s.account_label,
            windows: s.windows.into_iter().map(Into::into).collect(),
            credits_remaining: s.credits_remaining,
            error: s.error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum FfiTimerCondition {
    Never,
    AnyMessage,
    Sender { sender_id: String },
    AllSubagentsSettled,
    AllPeersSettled,
    FileChanged { path: String },
}

impl From<TimerCondition> for FfiTimerCondition {
    fn from(v: TimerCondition) -> Self {
        match v {
            TimerCondition::Never => Self::Never,
            TimerCondition::AnyMessage => Self::AnyMessage,
            TimerCondition::Sender { sender_id } => Self::Sender { sender_id },
            TimerCondition::AllSubagentsSettled => Self::AllSubagentsSettled,
            TimerCondition::AllPeersSettled => Self::AllPeersSettled,
            TimerCondition::FileChanged { path } => Self::FileChanged { path },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiConditionalTimerState {
    Armed,
    Fired,
    CanceledEarly,
    CanceledManual,
}

impl From<ConditionalTimerState> for FfiConditionalTimerState {
    fn from(v: ConditionalTimerState) -> Self {
        match v {
            ConditionalTimerState::Armed => Self::Armed,
            ConditionalTimerState::Fired => Self::Fired,
            ConditionalTimerState::CanceledEarly => Self::CanceledEarly,
            ConditionalTimerState::CanceledManual => Self::CanceledManual,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiConditionalTimerRecord {
    pub timer_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub prompt: String,
    pub duration_seconds: u64,
    pub condition: FfiTimerCondition,
    pub condition_label: String,
    pub state: FfiConditionalTimerState,
    pub created_at_ms: u64,
    pub fire_at_ms: u64,
    pub resolved_at_ms: Option<u64>,
    pub resolved_reason: Option<String>,
}

impl From<ConditionalTimerRecord> for FfiConditionalTimerRecord {
    fn from(v: ConditionalTimerRecord) -> Self {
        let condition_label = v.condition.short_label();
        Self {
            timer_id: v.timer_id,
            agent_id: v.agent_id,
            workspace_id: v.workspace_id,
            prompt: v.prompt,
            duration_seconds: v.duration_seconds,
            condition: v.condition.into(),
            condition_label,
            state: v.state.into(),
            created_at_ms: v.created_at_ms,
            fire_at_ms: v.fire_at_ms,
            resolved_at_ms: v.resolved_at_ms,
            resolved_reason: v.resolved_reason,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiGoalStatus {
    Running,
    Verifying,
    Paused,
    Succeeded,
    Failed,
    Canceled,
}

impl From<GoalStatus> for FfiGoalStatus {
    fn from(v: GoalStatus) -> Self {
        match v {
            GoalStatus::Running => Self::Running,
            GoalStatus::Verifying => Self::Verifying,
            GoalStatus::Paused => Self::Paused,
            GoalStatus::Succeeded => Self::Succeeded,
            GoalStatus::Failed => Self::Failed,
            GoalStatus::Canceled => Self::Canceled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiGoalControlAction {
    Pause,
    Resume,
    Cancel,
    VerifyNow,
}

impl From<FfiGoalControlAction> for GoalControlAction {
    fn from(v: FfiGoalControlAction) -> Self {
        match v {
            FfiGoalControlAction::Pause => Self::Pause,
            FfiGoalControlAction::Resume => Self::Resume,
            FfiGoalControlAction::Cancel => Self::Cancel,
            FfiGoalControlAction::VerifyNow => Self::VerifyNow,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiGoalCriterion {
    pub id: String,
    pub label: String,
    pub passed: bool,
}

impl From<GoalCriterion> for FfiGoalCriterion {
    fn from(v: GoalCriterion) -> Self {
        Self {
            id: v.id,
            label: v.label,
            passed: v.passed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiGoalRunRecord {
    pub goal_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub objective: String,
    pub criteria: Vec<FfiGoalCriterion>,
    pub verify_command: Option<String>,
    pub max_iterations: u32,
    pub current_iteration: u32,
    pub max_duration_secs: u64,
    pub status: FfiGoalStatus,
    pub last_verify_exit_code: Option<i32>,
    pub last_verify_output: Option<String>,
    pub consecutive_identical_failures: u32,
    pub pause_reason: Option<String>,
    pub paused_by_user: bool,
    pub active_duration_secs: u64,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub finished_at_ms: Option<u64>,
}

impl From<GoalRunRecord> for FfiGoalRunRecord {
    fn from(v: GoalRunRecord) -> Self {
        Self {
            goal_id: v.goal_id,
            agent_id: v.agent_id,
            workspace_id: v.workspace_id,
            objective: v.objective,
            criteria: v.criteria.into_iter().map(Into::into).collect(),
            verify_command: v.verify_command,
            max_iterations: v.max_iterations,
            current_iteration: v.current_iteration,
            max_duration_secs: v.max_duration_secs,
            status: v.status.into(),
            last_verify_exit_code: v.last_verify_exit_code,
            last_verify_output: v.last_verify_output,
            consecutive_identical_failures: v.consecutive_identical_failures,
            pause_reason: v.pause_reason,
            paused_by_user: v.paused_by_user,
            active_duration_secs: v.active_duration_secs,
            started_at_ms: v.started_at_ms,
            updated_at_ms: v.updated_at_ms,
            finished_at_ms: v.finished_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum FfiMonitorKind {
    CommandStream {
        command: String,
        args: Vec<String>,
        pattern: Option<String>,
    },
    LogFileTail {
        path: String,
        pattern: String,
    },
    HttpHealth {
        url: String,
        expect_status: u16,
        interval_secs: u64,
    },
}

impl From<MonitorKind> for FfiMonitorKind {
    fn from(v: MonitorKind) -> Self {
        match v {
            MonitorKind::CommandStream {
                command,
                args,
                pattern,
            } => Self::CommandStream {
                command,
                args,
                pattern,
            },
            MonitorKind::LogFileTail { path, pattern } => Self::LogFileTail { path, pattern },
            MonitorKind::HttpHealth {
                url,
                expect_status,
                interval_secs,
            } => Self::HttpHealth {
                url,
                expect_status,
                interval_secs,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiMonitorStatus {
    Active,
    Paused,
    Triggered,
    Stopped,
    Error,
}

impl From<MonitorStatus> for FfiMonitorStatus {
    fn from(v: MonitorStatus) -> Self {
        match v {
            MonitorStatus::Active => Self::Active,
            MonitorStatus::Paused => Self::Paused,
            MonitorStatus::Triggered => Self::Triggered,
            MonitorStatus::Stopped => Self::Stopped,
            MonitorStatus::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiMonitorWatchRecord {
    pub monitor_id: String,
    pub agent_id: String,
    pub name: String,
    pub kind: FfiMonitorKind,
    pub status: FfiMonitorStatus,
    pub match_count: u64,
    pub last_event_preview: Option<String>,
    pub last_triggered_at_ms: Option<u64>,
    pub expires_at_ms: Option<u64>,
    pub created_at_ms: u64,
}

impl From<MonitorWatchRecord> for FfiMonitorWatchRecord {
    fn from(v: MonitorWatchRecord) -> Self {
        Self {
            monitor_id: v.monitor_id,
            agent_id: v.agent_id,
            name: v.name,
            kind: v.kind.into(),
            status: v.status.into(),
            match_count: v.match_count,
            last_event_preview: v.last_event_preview,
            last_triggered_at_ms: v.last_triggered_at_ms,
            expires_at_ms: v.expires_at_ms,
            created_at_ms: v.created_at_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiWorkflowKind {
    BatchCsv,
    Dag,
    BestOfN,
}

impl From<WorkflowKind> for FfiWorkflowKind {
    fn from(v: WorkflowKind) -> Self {
        match v {
            WorkflowKind::BatchCsv => Self::BatchCsv,
            WorkflowKind::Dag => Self::Dag,
            WorkflowKind::BestOfN => Self::BestOfN,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiWorkflowRunStatus {
    Pending,
    Running,
    Succeeded,
    PartialFailure,
    Failed,
    Canceled,
}

impl From<WorkflowRunStatus> for FfiWorkflowRunStatus {
    fn from(v: WorkflowRunStatus) -> Self {
        match v {
            WorkflowRunStatus::Pending => Self::Pending,
            WorkflowRunStatus::Running => Self::Running,
            WorkflowRunStatus::Succeeded => Self::Succeeded,
            WorkflowRunStatus::PartialFailure => Self::PartialFailure,
            WorkflowRunStatus::Failed => Self::Failed,
            WorkflowRunStatus::Canceled => Self::Canceled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiWorkflowStepStatus {
    Pending,
    Running,
    Verifying,
    Succeeded,
    Failed,
    Canceled,
    Skipped,
}

impl From<WorkflowStepStatus> for FfiWorkflowStepStatus {
    fn from(v: WorkflowStepStatus) -> Self {
        match v {
            WorkflowStepStatus::Pending => Self::Pending,
            WorkflowStepStatus::Running => Self::Running,
            WorkflowStepStatus::Verifying => Self::Verifying,
            WorkflowStepStatus::Succeeded => Self::Succeeded,
            WorkflowStepStatus::Failed => Self::Failed,
            WorkflowStepStatus::Canceled => Self::Canceled,
            WorkflowStepStatus::Skipped => Self::Skipped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiWorkflowStepRecord {
    pub run_id: String,
    pub step_id: String,
    pub name: String,
    pub status: FfiWorkflowStepStatus,
    pub depends_on: Vec<String>,
    pub agent_id: Option<String>,
    pub worktree_cwd: Option<String>,
    pub provider: FfiProviderKind,
    pub model: Option<String>,
    pub output_preview: Option<String>,
    pub diff_additions: u32,
    pub diff_deletions: u32,
    pub diff_files: u32,
    pub verify_passed: Option<bool>,
    pub verify_output: Option<String>,
    pub error: Option<String>,
    pub is_winner: bool,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
}

impl From<WorkflowStepRecord> for FfiWorkflowStepRecord {
    fn from(v: WorkflowStepRecord) -> Self {
        Self {
            run_id: v.run_id,
            step_id: v.step_id,
            name: v.name,
            status: v.status.into(),
            depends_on: v.depends_on,
            agent_id: v.agent_id,
            worktree_cwd: v.worktree_cwd,
            provider: v.provider.into(),
            model: v.model,
            output_preview: v.output_preview,
            diff_additions: v.diff_additions,
            diff_deletions: v.diff_deletions,
            diff_files: v.diff_files,
            verify_passed: v.verify_passed,
            verify_output: v.verify_output,
            error: v.error,
            is_winner: v.is_winner,
            started_at_ms: v.started_at_ms,
            finished_at_ms: v.finished_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiWorkflowRunRecord {
    pub run_id: String,
    pub name: String,
    pub kind: FfiWorkflowKind,
    pub workspace_cwd: String,
    pub owner_agent_id: Option<String>,
    pub status: FfiWorkflowRunStatus,
    pub max_concurrency: u32,
    pub verify_command: Option<String>,
    pub winner_step_id: Option<String>,
    pub steps: Vec<FfiWorkflowStepRecord>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub finished_at_ms: Option<u64>,
}

impl From<WorkflowRunRecord> for FfiWorkflowRunRecord {
    fn from(v: WorkflowRunRecord) -> Self {
        Self {
            run_id: v.run_id,
            name: v.name,
            kind: v.kind.into(),
            workspace_cwd: v.workspace_cwd,
            owner_agent_id: v.owner_agent_id,
            status: v.status.into(),
            max_concurrency: v.max_concurrency,
            verify_command: v.verify_command,
            winner_step_id: v.winner_step_id,
            steps: v.steps.into_iter().map(Into::into).collect(),
            created_at_ms: v.created_at_ms,
            updated_at_ms: v.updated_at_ms,
            finished_at_ms: v.finished_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiPushRegistrationStatus {
    pub platform: String,
    pub token_registered: bool,
    pub token_preview: Option<String>,
    pub relay_sync_supported: bool,
}
impl From<orbit_client_core::app_core::LocalPushRegistration> for FfiPushRegistrationStatus {
    fn from(v: orbit_client_core::app_core::LocalPushRegistration) -> Self {
        Self {
            platform: v.platform,
            token_registered: v.token_registered,
            token_preview: v.token_preview,
            relay_sync_supported: v.relay_sync_supported,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiHostDetail {
    pub revision: u64,
    pub host: FfiHostCard,
    pub discovered_projects: Vec<FfiDiscoveredProject>,
    pub directory_listing: Option<FfiDirectoryListing>,
    pub provider_catalogs: Vec<FfiProviderModelCatalog>,
    pub workspaces: Vec<FfiWorkspaceSummary>,
    pub agents: Vec<FfiAgentCard>,
    pub archived_agents: Vec<FfiAgentCard>,
    pub last_created_agent_id: Option<String>,
    pub replaced_optimistic_agents: HashMap<String, String>,
    pub quotas: Vec<FfiProviderQuota>,
    pub terminals: Vec<FfiTerminalSummary>,
    pub active_terminal_id: Option<String>,
    pub active_terminal_slot: u8,
    pub schedules: Vec<FfiScheduleSpec>,
    pub provider_subagents: HashMap<String, Vec<FfiProviderSubagentDescriptor>>,
    pub git_repos_by_cwd: HashMap<String, FfiGitReposInspection>,
    pub workspace_boards: HashMap<String, FfiWorkspaceBoardSnapshot>,
    pub workspace_events: HashMap<String, Vec<FfiWorkspaceEvent>>,
    pub workspace_organize_status: HashMap<String, FfiWorkspaceOrganizeStatus>,
    pub acp_agents: Vec<FfiAcpAgentAvailability>,
    pub artifacts: Vec<FfiArtifactSummary>,
    pub conditional_timers: Vec<FfiConditionalTimerRecord>,
    pub goal_runs: Vec<FfiGoalRunRecord>,
    pub monitor_watches: Vec<FfiMonitorWatchRecord>,
    pub workflow_runs: Vec<FfiWorkflowRunRecord>,
}

impl From<HostDetailView> for FfiHostDetail {
    fn from(v: HostDetailView) -> Self {
        let mut active_agents = Vec::new();
        let mut archived_agents = Vec::new();
        for a in v.agents {
            if a.background {
                continue;
            }
            if a.archived_at_rfc3339.is_some() {
                archived_agents.push(a.into());
            } else {
                active_agents.push(a.into());
            }
        }
        Self {
            revision: v.revision,
            host: v.host.into(),
            discovered_projects: v.discovered_projects.into_iter().map(Into::into).collect(),
            directory_listing: v.directory_listing.map(Into::into),
            provider_catalogs: v.provider_catalogs.into_iter().map(Into::into).collect(),
            workspaces: v
                .workspaces
                .into_iter()
                .filter(|w| w.archived_at.is_none() && !w.background)
                .map(Into::into)
                .collect(),
            agents: active_agents,
            archived_agents,
            last_created_agent_id: v.last_created_agent_id,
            replaced_optimistic_agents: v.replaced_optimistic_agents,
            quotas: v.quotas.into_iter().map(Into::into).collect(),
            terminals: v.terminals.into_iter().map(Into::into).collect(),
            active_terminal_id: v.active_terminal_id,
            active_terminal_slot: v.active_terminal_slot,
            schedules: v.schedules.into_iter().map(Into::into).collect(),
            provider_subagents: v
                .provider_subagents
                .into_iter()
                .map(|(k, list)| (k, list.into_iter().map(Into::into).collect()))
                .collect(),
            git_repos_by_cwd: v
                .git_repos_by_cwd
                .into_iter()
                .map(|(k, val)| (k, val.into()))
                .collect(),
            workspace_boards: v
                .workspace_boards
                .into_iter()
                .map(|(k, val)| (k, val.into()))
                .collect(),
            workspace_events: v
                .workspace_events
                .into_iter()
                .map(|(k, list)| (k, list.into_iter().map(Into::into).collect()))
                .collect(),
            workspace_organize_status: v
                .workspace_organize_status
                .into_iter()
                .map(|(k, val)| (k, val.into()))
                .collect(),
            acp_agents: v.acp_agents.into_iter().map(Into::into).collect(),
            artifacts: v.artifacts.into_iter().map(Into::into).collect(),
            conditional_timers: v.conditional_timers.into_iter().map(Into::into).collect(),
            goal_runs: v.goal_runs.into_iter().map(Into::into).collect(),
            monitor_watches: v.monitor_watches.into_iter().map(Into::into).collect(),
            workflow_runs: v.workflow_runs.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiArtifactKind {
    Markdown,
    Html,
    Code,
    Svg,
    Mermaid,
    McpApp,
}

impl From<ArtifactKind> for FfiArtifactKind {
    fn from(v: ArtifactKind) -> Self {
        match v {
            ArtifactKind::Markdown => Self::Markdown,
            ArtifactKind::Html => Self::Html,
            ArtifactKind::Code => Self::Code,
            ArtifactKind::Svg => Self::Svg,
            ArtifactKind::Mermaid => Self::Mermaid,
            ArtifactKind::McpApp => Self::McpApp,
        }
    }
}

impl From<FfiArtifactKind> for ArtifactKind {
    fn from(v: FfiArtifactKind) -> Self {
        match v {
            FfiArtifactKind::Markdown => Self::Markdown,
            FfiArtifactKind::Html => Self::Html,
            FfiArtifactKind::Code => Self::Code,
            FfiArtifactKind::Svg => Self::Svg,
            FfiArtifactKind::Mermaid => Self::Mermaid,
            FfiArtifactKind::McpApp => Self::McpApp,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiArtifactSummary {
    pub artifact_id: String,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub title: String,
    pub kind: FfiArtifactKind,
    pub language: Option<String>,
    pub mime_type: Option<String>,
    pub resource_uri: Option<String>,
    pub version: u32,
    pub byte_len: u64,
    pub created_at_rfc3339: String,
    pub updated_at_rfc3339: String,
}

impl From<ArtifactSummary> for FfiArtifactSummary {
    fn from(v: ArtifactSummary) -> Self {
        Self {
            artifact_id: v.artifact_id,
            workspace_id: v.workspace_id,
            agent_id: v.agent_id,
            title: v.title,
            kind: v.kind.into(),
            language: v.language,
            mime_type: v.mime_type,
            resource_uri: v.resource_uri,
            version: v.version,
            byte_len: v.byte_len as u64,
            created_at_rfc3339: v.created_at.to_rfc3339(),
            updated_at_rfc3339: v.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiArtifactRevisionMeta {
    pub version: u32,
    pub created_at_rfc3339: String,
    pub summary_note: Option<String>,
    pub byte_len: u64,
}

impl From<ArtifactRevisionMeta> for FfiArtifactRevisionMeta {
    fn from(v: ArtifactRevisionMeta) -> Self {
        Self {
            version: v.version,
            created_at_rfc3339: v.created_at.to_rfc3339(),
            summary_note: v.summary_note,
            byte_len: v.byte_len as u64,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiArtifactDetail {
    pub summary: FfiArtifactSummary,
    pub content: String,
    pub revisions: Vec<FfiArtifactRevisionMeta>,
}

impl From<ArtifactDetail> for FfiArtifactDetail {
    fn from(v: ArtifactDetail) -> Self {
        Self {
            summary: v.summary.into(),
            content: v.content,
            revisions: v.revisions.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiUpsertArtifactRequest {
    pub host_id: String,
    pub artifact_id: Option<String>,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub title: String,
    pub kind: FfiArtifactKind,
    pub language: Option<String>,
    pub mime_type: Option<String>,
    pub resource_uri: Option<String>,
    pub content: String,
    pub summary_note: Option<String>,
}

impl From<FfiUpsertArtifactRequest> for UpsertArtifactRequest {
    fn from(v: FfiUpsertArtifactRequest) -> Self {
        Self {
            host_id: v.host_id,
            artifact_id: v.artifact_id,
            workspace_id: v.workspace_id,
            agent_id: v.agent_id,
            title: v.title,
            kind: v.kind.into(),
            language: v.language,
            mime_type: v.mime_type,
            resource_uri: v.resource_uri,
            mcp_server_name: None,
            content: v.content,
            summary_note: v.summary_note,
        }
    }
}

// --- Rich Markdown, Code Highlighting & Diff Records ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiPromptBadge {
    pub kind: String,
    pub label: String,
    pub path: Option<String>,
}

impl From<PromptBadge> for FfiPromptBadge {
    fn from(v: PromptBadge) -> Self {
        Self {
            kind: v.kind,
            label: v.label,
            path: v.path,
        }
    }
}

impl From<FfiPromptBadge> for PromptBadge {
    fn from(v: FfiPromptBadge) -> Self {
        Self {
            kind: v.kind,
            label: v.label,
            path: v.path,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiInlineSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub inline_code: bool,
    pub link_url: Option<String>,
    pub math_ml: Option<String>,
}

impl From<InlineSpanView> for FfiInlineSpan {
    fn from(v: InlineSpanView) -> Self {
        Self {
            text: v.text,
            bold: v.bold,
            italic: v.italic,
            strikethrough: v.strikethrough,
            inline_code: v.inline_code,
            link_url: v.link_url,
            math_ml: v.math_ml,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiHighlightedToken {
    pub text: String,
    pub fg_hex: String,
    pub bold: bool,
    pub italic: bool,
}

impl From<HighlightedTokenView> for FfiHighlightedToken {
    fn from(v: HighlightedTokenView) -> Self {
        Self {
            text: v.text,
            fg_hex: v.fg_hex,
            bold: v.bold,
            italic: v.italic,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiHighlightedLine {
    pub line_number: u32,
    pub tokens: Vec<FfiHighlightedToken>,
}

impl From<HighlightedLineView> for FfiHighlightedLine {
    fn from(v: HighlightedLineView) -> Self {
        Self {
            line_number: v.line_number,
            tokens: v.tokens.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiTableAlignment {
    None,
    Left,
    Center,
    Right,
}

impl From<TableAlignmentView> for FfiTableAlignment {
    fn from(v: TableAlignmentView) -> Self {
        match v {
            TableAlignmentView::None => Self::None,
            TableAlignmentView::Left => Self::Left,
            TableAlignmentView::Center => Self::Center,
            TableAlignmentView::Right => Self::Right,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTableCell {
    pub spans: Vec<FfiInlineSpan>,
}

impl From<TableCellView> for FfiTableCell {
    fn from(v: TableCellView) -> Self {
        Self {
            spans: v.spans.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTableRow {
    pub cells: Vec<FfiTableCell>,
}

impl From<TableRowView> for FfiTableRow {
    fn from(v: TableRowView) -> Self {
        Self {
            cells: v.cells.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiRichBlock {
    Heading {
        level: u8,
        spans: Vec<FfiInlineSpan>,
    },
    Paragraph {
        spans: Vec<FfiInlineSpan>,
    },
    CodeBlock {
        language: String,
        raw_code: String,
        lines: Vec<FfiHighlightedLine>,
    },
    ListItem {
        ordered: bool,
        index: u32,
        checked: Option<bool>,
        depth: u8,
        spans: Vec<FfiInlineSpan>,
    },
    BlockQuote {
        spans: Vec<FfiInlineSpan>,
    },
    Table {
        alignments: Vec<FfiTableAlignment>,
        headers: Vec<FfiTableCell>,
        rows: Vec<FfiTableRow>,
    },
    HorizontalRule,
    MathBlock {
        tex: String,
        math_ml: String,
    },
}

impl From<RichBlockView> for FfiRichBlock {
    fn from(v: RichBlockView) -> Self {
        match v {
            RichBlockView::Heading { level, spans } => Self::Heading {
                level,
                spans: spans.into_iter().map(Into::into).collect(),
            },
            RichBlockView::Paragraph { spans } => Self::Paragraph {
                spans: spans.into_iter().map(Into::into).collect(),
            },
            RichBlockView::CodeBlock {
                language,
                raw_code,
                lines,
            } => Self::CodeBlock {
                language,
                raw_code,
                lines: lines.into_iter().map(Into::into).collect(),
            },
            RichBlockView::ListItem {
                ordered,
                index,
                checked,
                depth,
                spans,
            } => Self::ListItem {
                ordered,
                index,
                checked,
                depth,
                spans: spans.into_iter().map(Into::into).collect(),
            },
            RichBlockView::BlockQuote { spans } => Self::BlockQuote {
                spans: spans.into_iter().map(Into::into).collect(),
            },
            RichBlockView::Table {
                alignments,
                headers,
                rows,
            } => Self::Table {
                alignments: alignments.into_iter().map(Into::into).collect(),
                headers: headers.into_iter().map(Into::into).collect(),
                rows: rows.into_iter().map(Into::into).collect(),
            },
            RichBlockView::MathBlock { tex, math_ml } => Self::MathBlock { tex, math_ml },
            RichBlockView::HorizontalRule => Self::HorizontalRule,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiDiffLineKind {
    Meta,
    HunkHeader,
    Context,
    Added,
    Removed,
}

impl From<DiffLineKind> for FfiDiffLineKind {
    fn from(k: DiffLineKind) -> Self {
        match k {
            DiffLineKind::Meta => Self::Meta,
            DiffLineKind::HunkHeader => Self::HunkHeader,
            DiffLineKind::Context => Self::Context,
            DiffLineKind::Added => Self::Added,
            DiffLineKind::Removed => Self::Removed,
        }
    }
}

impl From<FfiDiffLineKind> for DiffLineKind {
    fn from(k: FfiDiffLineKind) -> Self {
        match k {
            FfiDiffLineKind::Meta => Self::Meta,
            FfiDiffLineKind::HunkHeader => Self::HunkHeader,
            FfiDiffLineKind::Context => Self::Context,
            FfiDiffLineKind::Added => Self::Added,
            FfiDiffLineKind::Removed => Self::Removed,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiDiffLine {
    pub kind: FfiDiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub content: String,
}

impl From<DiffLineView> for FfiDiffLine {
    fn from(v: DiffLineView) -> Self {
        Self {
            kind: v.kind.into(),
            old_line: v.old_line,
            new_line: v.new_line,
            content: v.content,
        }
    }
}

impl From<FfiDiffLine> for DiffLineView {
    fn from(v: FfiDiffLine) -> Self {
        Self {
            kind: v.kind.into(),
            old_line: v.old_line,
            new_line: v.new_line,
            content: v.content,
            tokens: Vec::new(),
            inline_changes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiDiffFile {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub lines: Vec<FfiDiffLine>,
}

impl From<DiffFileView> for FfiDiffFile {
    fn from(v: DiffFileView) -> Self {
        Self {
            path: v.path,
            additions: v.additions,
            deletions: v.deletions,
            lines: v.lines.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<FfiDiffFile> for DiffFileView {
    fn from(v: FfiDiffFile) -> Self {
        Self {
            path: v.path,
            additions: v.additions,
            deletions: v.deletions,
            lines: v.lines.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiToolStatus {
    Running,
    Completed,
    Failed,
    Denied,
}

impl From<ToolCallStatus> for FfiToolStatus {
    fn from(s: ToolCallStatus) -> Self {
        match s {
            ToolCallStatus::Running => Self::Running,
            ToolCallStatus::Completed => Self::Completed,
            ToolCallStatus::Failed => Self::Failed,
            ToolCallStatus::Denied => Self::Denied,
        }
    }
}

impl From<FfiToolStatus> for ToolCallStatus {
    fn from(s: FfiToolStatus) -> Self {
        match s {
            FfiToolStatus::Running => Self::Running,
            FfiToolStatus::Completed => Self::Completed,
            FfiToolStatus::Failed => Self::Failed,
            FfiToolStatus::Denied => Self::Denied,
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiToolCallDetail {
    Shell {
        command: String,
        cwd: Option<String>,
        exit_code: Option<i32>,
        output_preview: Option<String>,
    },
    FileRead {
        path: String,
        start_line: Option<u32>,
        end_line: Option<u32>,
    },
    FileEdit {
        path: String,
        unified_diff: Option<String>,
    },
    Search {
        query: String,
        scope: Option<String>,
    },
    Subagent {
        child_agent_id: Option<String>,
        role: String,
        prompt_preview: String,
    },
    Generic {
        tool_name: String,
        arguments_json: String,
    },
}

impl From<ToolCallDetail> for FfiToolCallDetail {
    fn from(v: ToolCallDetail) -> Self {
        match v {
            ToolCallDetail::Shell {
                command,
                cwd,
                exit_code,
                output_preview,
            } => Self::Shell {
                command,
                cwd,
                exit_code,
                output_preview,
            },
            ToolCallDetail::FileRead {
                path,
                start_line,
                end_line,
            } => Self::FileRead {
                path,
                start_line,
                end_line,
            },
            ToolCallDetail::FileEdit { path, unified_diff } => {
                Self::FileEdit { path, unified_diff }
            }
            ToolCallDetail::Search { query, scope } => Self::Search { query, scope },
            ToolCallDetail::Subagent {
                child_agent_id,
                role,
                prompt_preview,
            } => Self::Subagent {
                child_agent_id,
                role,
                prompt_preview,
            },
            ToolCallDetail::Generic {
                tool_name,
                arguments_json,
            } => Self::Generic {
                tool_name,
                arguments_json,
            },
        }
    }
}

impl From<FfiToolCallDetail> for ToolCallDetail {
    fn from(v: FfiToolCallDetail) -> Self {
        match v {
            FfiToolCallDetail::Shell {
                command,
                cwd,
                exit_code,
                output_preview,
            } => Self::Shell {
                command,
                cwd,
                exit_code,
                output_preview,
            },
            FfiToolCallDetail::FileRead {
                path,
                start_line,
                end_line,
            } => Self::FileRead {
                path,
                start_line,
                end_line,
            },
            FfiToolCallDetail::FileEdit { path, unified_diff } => {
                Self::FileEdit { path, unified_diff }
            }
            FfiToolCallDetail::Search { query, scope } => Self::Search { query, scope },
            FfiToolCallDetail::Subagent {
                child_agent_id,
                role,
                prompt_preview,
            } => Self::Subagent {
                child_agent_id,
                role,
                prompt_preview,
            },
            FfiToolCallDetail::Generic {
                tool_name,
                arguments_json,
            } => Self::Generic {
                tool_name,
                arguments_json,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiToolStepKind {
    FileRead,
    FileCreate,
    FileEdit,
    Search,
    Shell,
    Subagent,
    Generic,
}

impl From<ToolStepKind> for FfiToolStepKind {
    fn from(v: ToolStepKind) -> Self {
        match v {
            ToolStepKind::FileRead => Self::FileRead,
            ToolStepKind::FileCreate => Self::FileCreate,
            ToolStepKind::FileEdit => Self::FileEdit,
            ToolStepKind::Search => Self::Search,
            ToolStepKind::Shell => Self::Shell,
            ToolStepKind::Subagent => Self::Subagent,
            ToolStepKind::Generic => Self::Generic,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiToolStepDescriptor {
    pub kind: FfiToolStepKind,
    pub verb: String,
    pub target: String,
    pub secondary_meta: Option<String>,
    pub open_path: Option<String>,
    pub additions: u32,
    pub deletions: u32,
    pub exit_code: Option<i32>,
    pub is_running: bool,
    pub is_failed: bool,
    pub is_denied: bool,
    pub has_expandable_detail: bool,
}

impl From<ToolStepDescriptorView> for FfiToolStepDescriptor {
    fn from(v: ToolStepDescriptorView) -> Self {
        Self {
            kind: v.kind.into(),
            verb: v.verb,
            target: v.target,
            secondary_meta: v.secondary_meta,
            open_path: v.open_path,
            additions: v.additions,
            deletions: v.deletions,
            exit_code: v.exit_code,
            is_running: v.is_running,
            is_failed: v.is_failed,
            is_denied: v.is_denied,
            has_expandable_detail: v.has_expandable_detail,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiQuestionItem {
    pub id: String,
    pub header: Option<String>,
    pub question: String,
    pub options: Vec<String>,
    pub is_multi_select: bool,
}

impl From<QuestionItem> for FfiQuestionItem {
    fn from(q: QuestionItem) -> Self {
        Self {
            id: q.id,
            header: q.header,
            question: q.question,
            options: q.options,
            is_multi_select: q.is_multi_select,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiAgentMessageKind {
    DirectMessage,
    FinishNotification,
    Heartbeat,
}

impl From<AgentMessageKind> for FfiAgentMessageKind {
    fn from(v: AgentMessageKind) -> Self {
        match v {
            AgentMessageKind::DirectMessage => Self::DirectMessage,
            AgentMessageKind::FinishNotification => Self::FinishNotification,
            AgentMessageKind::Heartbeat => Self::Heartbeat,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiAgentMessageDeliveryStatus {
    Queued,
    Dispatched,
    Settled,
    Dropped,
}

impl From<AgentMessageDeliveryStatus> for FfiAgentMessageDeliveryStatus {
    fn from(v: AgentMessageDeliveryStatus) -> Self {
        match v {
            AgentMessageDeliveryStatus::Queued => Self::Queued,
            AgentMessageDeliveryStatus::Dispatched => Self::Dispatched,
            AgentMessageDeliveryStatus::Settled => Self::Settled,
            AgentMessageDeliveryStatus::Dropped => Self::Dropped,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiPermissionCheckpointRef {
    pub child_agent_id: String,
    pub child_host_id: Option<String>,
    pub permission_request_id: String,
    pub title: String,
    pub allowed_decisions: Vec<String>,
}

impl From<PermissionCheckpointRef> for FfiPermissionCheckpointRef {
    fn from(v: PermissionCheckpointRef) -> Self {
        Self {
            child_agent_id: v.agent_id,
            child_host_id: v.host_id,
            permission_request_id: v.request_id,
            title: v.title,
            allowed_decisions: v.allowed_decisions,
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiTimelineItem {
    UserPrompt {
        seq: u64,
        message_id: String,
        text: String,
        badges: Vec<FfiPromptBadge>,
        blocks: Vec<FfiRichBlock>,
        timestamp_ms: Option<i64>,
    },
    AssistantThinking {
        seq: u64,
        item_id: String,
        text: String,
        headline: String,
        timestamp_ms: Option<i64>,
    },
    AssistantMessage {
        seq: u64,
        item_id: String,
        text: String,
        streaming: bool,
        blocks: Vec<FfiRichBlock>,
        timestamp_ms: Option<i64>,
    },
    ToolCall {
        seq: u64,
        call_id: String,
        status: FfiToolStatus,
        summary_title: String,
        preview_body: Option<String>,
        detail: FfiToolCallDetail,
        step_descriptor: Option<FfiToolStepDescriptor>,
        diff_files: Vec<FfiDiffFile>,
        timestamp_ms: Option<i64>,
    },
    PermissionRequest {
        seq: u64,
        request_id: String,
        title: String,
        command_or_path: String,
        detail: FfiToolCallDetail,
        allowed_decisions: Vec<String>,
        resolved_decision: Option<String>,
        timestamp_ms: Option<i64>,
    },
    QuestionRequest {
        seq: u64,
        request_id: String,
        questions: Vec<FfiQuestionItem>,
        resolved_answers: Option<Vec<Vec<String>>>,
        timestamp_ms: Option<i64>,
    },
    Notice {
        seq: u64,
        level: String,
        message: String,
        timestamp_ms: Option<i64>,
    },
    AgentMessage {
        seq: u64,
        message_id: String,
        kind: FfiAgentMessageKind,
        source_agent_id: Option<String>,
        source_host_id: Option<String>,
        source_title: Option<String>,
        event: Option<String>,
        text: String,
        permission_context: Option<FfiPermissionCheckpointRef>,
        delivery_status: Option<FfiAgentMessageDeliveryStatus>,
        delivery_turn_id: Option<String>,
        blocking_turn_id: Option<String>,
        queued_at_ms: Option<i64>,
        dispatched_at_ms: Option<i64>,
        settled_at_ms: Option<i64>,
        blocks: Vec<FfiRichBlock>,
        timestamp_ms: Option<i64>,
    },
}

fn tool_preview_text(detail: &ToolCallDetail) -> Option<String> {
    match detail {
        ToolCallDetail::Shell { output_preview, .. } => output_preview.clone(),
        ToolCallDetail::FileRead {
            start_line: Some(s),
            end_line: Some(e),
            ..
        } => Some(format!("Lines {s}–{e}")),
        ToolCallDetail::Subagent { prompt_preview, .. } => Some(prompt_preview.clone()),
        ToolCallDetail::Generic { arguments_json, .. } => Some(arguments_json.clone()),
        _ => None,
    }
}

fn tool_primary_target(detail: &ToolCallDetail) -> String {
    match detail {
        ToolCallDetail::Shell { command, .. } => command.clone(),
        ToolCallDetail::FileRead { path, .. } | ToolCallDetail::FileEdit { path, .. } => {
            path.clone()
        }
        ToolCallDetail::Search { query, .. } => query.clone(),
        ToolCallDetail::Subagent { role, .. } => role.clone(),
        ToolCallDetail::Generic {
            tool_name,
            arguments_json,
        } => format!("{tool_name}({arguments_json})"),
    }
}

impl From<TimelineItemView> for FfiTimelineItem {
    fn from(v: TimelineItemView) -> Self {
        match v {
            TimelineItemView::UserPrompt {
                seq,
                message_id,
                text,
                badges,
                blocks,
                timestamp_ms,
            } => Self::UserPrompt {
                seq,
                message_id,
                text,
                badges: badges.into_iter().map(Into::into).collect(),
                blocks: blocks.into_iter().map(Into::into).collect(),
                timestamp_ms,
            },
            TimelineItemView::AssistantThinking {
                seq,
                item_id,
                text,
                timestamp_ms,
            } => {
                let headline = extract_thinking_headline(&text, true);
                Self::AssistantThinking {
                    seq,
                    item_id,
                    text,
                    headline,
                    timestamp_ms,
                }
            }
            TimelineItemView::AssistantMessage {
                seq,
                item_id,
                text,
                streaming,
                blocks,
                timestamp_ms,
            } => Self::AssistantMessage {
                seq,
                item_id,
                text,
                streaming,
                blocks: blocks.into_iter().map(Into::into).collect(),
                timestamp_ms,
            },
            TimelineItemView::ToolCall {
                seq,
                call_id,
                status,
                summary_title,
                detail,
                step_descriptor,
                diff_files,
                timestamp_ms,
                ..
            } => {
                let resolved_desc = step_descriptor
                    .or_else(|| {
                        Some(describe_tool_detail(
                            status,
                            &summary_title,
                            &detail,
                            &diff_files,
                            None,
                        ))
                    })
                    .map(Into::into);
                let preview_body = tool_preview_text(&detail);
                Self::ToolCall {
                    seq,
                    call_id,
                    status: status.into(),
                    summary_title,
                    preview_body,
                    detail: detail.into(),
                    step_descriptor: resolved_desc,
                    diff_files: diff_files.into_iter().map(Into::into).collect(),
                    timestamp_ms,
                }
            }
            TimelineItemView::PermissionRequest {
                seq,
                request_id,
                title,
                detail,
                allowed_decisions,
                resolved_decision,
                timestamp_ms,
                ..
            } => {
                let command_or_path = tool_primary_target(&detail);
                Self::PermissionRequest {
                    seq,
                    request_id,
                    title,
                    command_or_path,
                    detail: detail.into(),
                    allowed_decisions,
                    resolved_decision,
                    timestamp_ms,
                }
            }
            TimelineItemView::QuestionRequest {
                seq,
                request_id,
                questions,
                resolved_answers,
                timestamp_ms,
            } => Self::QuestionRequest {
                seq,
                request_id,
                questions: questions.into_iter().map(Into::into).collect(),
                resolved_answers,
                timestamp_ms,
            },
            TimelineItemView::Notice {
                seq,
                level,
                message,
                timestamp_ms,
            } => Self::Notice {
                seq,
                level,
                message,
                timestamp_ms,
            },
            TimelineItemView::AgentMessage {
                seq,
                message_id,
                kind,
                source_agent_id,
                source_host_id,
                source_title,
                event,
                text,
                permission_context,
                delivery_status,
                delivery_turn_id,
                blocking_turn_id,
                queued_at_ms,
                dispatched_at_ms,
                settled_at_ms,
                blocks,
                timestamp_ms,
            } => Self::AgentMessage {
                seq,
                message_id,
                kind: kind.into(),
                source_agent_id,
                source_host_id,
                source_title,
                event,
                text,
                permission_context: permission_context.map(Into::into),
                delivery_status: delivery_status.map(Into::into),
                delivery_turn_id,
                blocking_turn_id,
                queued_at_ms,
                dispatched_at_ms,
                settled_at_ms,
                blocks: blocks.into_iter().map(Into::into).collect(),
                timestamp_ms,
            },
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAgentTimeline {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<FfiAgentCard>,
    pub items: Vec<FfiTimelineItem>,
}

impl From<AgentTimelineView> for FfiAgentTimeline {
    fn from(v: AgentTimelineView) -> Self {
        Self {
            revision: v.revision,
            host_id: v.host_id,
            agent: v.agent.map(Into::into),
            items: v.items.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAgentTimelineDelta {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<FfiAgentCard>,
    pub latest_seq: u64,
    pub upserted_items: Vec<FfiTimelineItem>,
    pub resolved_permissions: HashMap<String, String>,
    pub resolved_questions: HashMap<String, Vec<Vec<String>>>,
}

impl From<AgentTimelineDeltaView> for FfiAgentTimelineDelta {
    fn from(v: AgentTimelineDeltaView) -> Self {
        Self {
            revision: v.revision,
            host_id: v.host_id,
            agent: v.agent.map(Into::into),
            latest_seq: v.latest_seq,
            upserted_items: v.upserted_items.into_iter().map(Into::into).collect(),
            resolved_permissions: v.resolved_permissions,
            resolved_questions: v.resolved_questions,
        }
    }
}

// --- Turn-Coalesced Agent Stage FFI Types ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTurnFileDiffSummary {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
}

impl From<TurnFileDiffSummaryView> for FfiTurnFileDiffSummary {
    fn from(v: TurnFileDiffSummaryView) -> Self {
        Self {
            path: v.path,
            additions: v.additions,
            deletions: v.deletions,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTurnWorkCounts {
    pub edited_file_count: u32,
    pub created_file_count: u32,
    pub command_count: u32,
    pub read_file_count: u32,
    pub search_count: u32,
    pub subagent_count: u32,
    pub mcp_server_count: u32,
    pub other_tool_count: u32,
    pub thought_count: u32,
    pub narration_count: u32,
    pub action_steps: u32,
    pub total_steps: u32,
    pub failed_count: u32,
    pub denied_count: u32,
    pub unrecovered_failure_count: u32,
    pub additions: u32,
    pub deletions: u32,
}

impl From<TurnWorkCountsView> for FfiTurnWorkCounts {
    fn from(v: TurnWorkCountsView) -> Self {
        Self {
            edited_file_count: v.edited_file_count,
            created_file_count: v.created_file_count,
            command_count: v.command_count,
            read_file_count: v.read_file_count,
            search_count: v.search_count,
            subagent_count: v.subagent_count,
            mcp_server_count: v.mcp_server_count,
            other_tool_count: v.other_tool_count,
            thought_count: v.thought_count,
            narration_count: v.narration_count,
            action_steps: v.action_steps,
            total_steps: v.total_steps,
            failed_count: v.failed_count,
            denied_count: v.denied_count,
            unrecovered_failure_count: v.unrecovered_failure_count,
            additions: v.additions,
            deletions: v.deletions,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkSummaryResult {
    pub counts: FfiTurnWorkCounts,
    pub label: String,
    pub live_label: String,
    pub live_detail: Option<String>,
    pub label_includes_failure: bool,
}

impl From<WorkSummaryResultView> for FfiWorkSummaryResult {
    fn from(v: WorkSummaryResultView) -> Self {
        Self {
            counts: v.counts.into(),
            label: v.label,
            live_label: v.live_label,
            live_detail: v.live_detail,
            label_includes_failure: v.label_includes_failure,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTurnCompactionItem {
    pub seq: u64,
    pub label: String,
}

impl From<TurnCompactionItemView> for FfiTurnCompactionItem {
    fn from(v: TurnCompactionItemView) -> Self {
        Self {
            seq: v.seq,
            label: v.label,
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiTurnLiveSegment {
    StepGroup {
        key: String,
        first_seq: u64,
        items: Vec<FfiTimelineItem>,
        summary: FfiWorkSummaryResult,
        thought_duration_ms: Option<i64>,
        work_duration_ms: Option<i64>,
        started_at_ms: Option<i64>,
        latest_activity_ms: Option<i64>,
        has_following_content: bool,
        has_running_step: bool,
    },
    AssistantMessage {
        key: String,
        item: FfiTimelineItem,
    },
    AgentMessage {
        key: String,
        item: FfiTimelineItem,
    },
    Notice {
        key: String,
        item: FfiTimelineItem,
    },
    Compaction {
        key: String,
        label: String,
    },
    Wakeup {
        key: String,
        label: String,
    },
}

impl From<TurnLiveSegmentView> for FfiTurnLiveSegment {
    fn from(v: TurnLiveSegmentView) -> Self {
        match v {
            TurnLiveSegmentView::StepGroup {
                key,
                first_seq,
                items,
                summary,
                thought_duration_ms,
                work_duration_ms,
                started_at_ms,
                latest_activity_ms,
                has_following_content,
                has_running_step,
            } => Self::StepGroup {
                key,
                first_seq,
                items: items.into_iter().map(Into::into).collect(),
                summary: summary.into(),
                thought_duration_ms,
                work_duration_ms,
                started_at_ms,
                latest_activity_ms,
                has_following_content,
                has_running_step,
            },
            TurnLiveSegmentView::AssistantMessage { key, item } => Self::AssistantMessage {
                key,
                item: item.into(),
            },
            TurnLiveSegmentView::AgentMessage { key, item } => Self::AgentMessage {
                key,
                item: item.into(),
            },
            TurnLiveSegmentView::Notice { key, item } => Self::Notice {
                key,
                item: item.into(),
            },
            TurnLiveSegmentView::Compaction { key, label } => Self::Compaction { key, label },
            TurnLiveSegmentView::Wakeup { key, label } => Self::Wakeup { key, label },
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiStageTurn {
    pub key: String,
    pub turn_index: u32,
    pub anchor_seq: u64,
    pub user_prompt: Option<FfiTimelineItem>,
    pub agent_message: Option<FfiTimelineItem>,
    pub is_compaction_command: bool,
    pub folded_steps: Vec<FfiTimelineItem>,
    pub work_counts: FfiTurnWorkCounts,
    pub work_summary_label: String,
    pub work_summary_live_label: String,
    pub work_summary_live_detail: Option<String>,
    pub work_summary_includes_failure: bool,
    pub thought_duration_ms: Option<i64>,
    pub work_duration_ms: Option<i64>,
    pub turn_duration_ms: Option<i64>,
    pub terminal_reply: Option<FfiTimelineItem>,
    pub last_assistant_reply: Option<FfiTimelineItem>,
    pub notices: Vec<FfiTimelineItem>,
    pub compactions: Vec<FfiTurnCompactionItem>,
    pub live_segments: Vec<FfiTurnLiveSegment>,
    pub turn_diffs: Vec<FfiTurnFileDiffSummary>,
    pub has_error: bool,
    pub has_unrecovered_error: bool,
    pub has_active_item: bool,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
    pub latest_activity_ms: Option<i64>,
    pub latest_visible_progress_ms: Option<i64>,
}

impl From<StageTurnView> for FfiStageTurn {
    fn from(v: StageTurnView) -> Self {
        Self {
            key: v.key,
            turn_index: v.turn_index,
            anchor_seq: v.anchor_seq,
            user_prompt: v.user_prompt.map(Into::into),
            agent_message: v.agent_message.map(Into::into),
            is_compaction_command: v.is_compaction_command,
            folded_steps: v.folded_steps.into_iter().map(Into::into).collect(),
            work_counts: v.work_counts.into(),
            work_summary_label: v.work_summary_label,
            work_summary_live_label: v.work_summary_live_label,
            work_summary_live_detail: v.work_summary_live_detail,
            work_summary_includes_failure: v.work_summary_includes_failure,
            thought_duration_ms: v.thought_duration_ms,
            work_duration_ms: v.work_duration_ms,
            turn_duration_ms: v.turn_duration_ms,
            terminal_reply: v.terminal_reply.map(Into::into),
            last_assistant_reply: v.last_assistant_reply.map(Into::into),
            notices: v.notices.into_iter().map(Into::into).collect(),
            compactions: v.compactions.into_iter().map(Into::into).collect(),
            live_segments: v.live_segments.into_iter().map(Into::into).collect(),
            turn_diffs: v.turn_diffs.into_iter().map(Into::into).collect(),
            has_error: v.has_error,
            has_unrecovered_error: v.has_unrecovered_error,
            has_active_item: v.has_active_item,
            started_at_ms: v.started_at_ms,
            completed_at_ms: v.completed_at_ms,
            latest_activity_ms: v.latest_activity_ms,
            latest_visible_progress_ms: v.latest_visible_progress_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiSessionTouchedFileAction {
    Created,
    Edited,
    Read,
}

impl From<SessionTouchedFileAction> for FfiSessionTouchedFileAction {
    fn from(v: SessionTouchedFileAction) -> Self {
        match v {
            SessionTouchedFileAction::Created => Self::Created,
            SessionTouchedFileAction::Edited => Self::Edited,
            SessionTouchedFileAction::Read => Self::Read,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSessionTouchedFile {
    pub path: String,
    pub display_path: String,
    pub action: FfiSessionTouchedFileAction,
    pub additions: u32,
    pub deletions: u32,
    pub last_seq: u64,
}

impl From<SessionTouchedFileView> for FfiSessionTouchedFile {
    fn from(v: SessionTouchedFileView) -> Self {
        Self {
            path: v.path,
            display_path: v.display_path,
            action: v.action.into(),
            additions: v.additions,
            deletions: v.deletions,
            last_seq: v.last_seq,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiSessionBackgroundTaskKind {
    Shell,
    Wakeup,
    Queued,
}

impl From<SessionBackgroundTaskKind> for FfiSessionBackgroundTaskKind {
    fn from(v: SessionBackgroundTaskKind) -> Self {
        match v {
            SessionBackgroundTaskKind::Shell => Self::Shell,
            SessionBackgroundTaskKind::Wakeup => Self::Wakeup,
            SessionBackgroundTaskKind::Queued => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiSessionBackgroundTaskStatus {
    Running,
    Queued,
}

impl From<SessionBackgroundTaskStatus> for FfiSessionBackgroundTaskStatus {
    fn from(v: SessionBackgroundTaskStatus) -> Self {
        match v {
            SessionBackgroundTaskStatus::Running => Self::Running,
            SessionBackgroundTaskStatus::Queued => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSessionBackgroundTask {
    pub id: String,
    pub kind: FfiSessionBackgroundTaskKind,
    pub title: String,
    pub subtitle: String,
    pub status: FfiSessionBackgroundTaskStatus,
    pub started_at_ms: Option<i64>,
    pub command: Option<String>,
    pub output_preview: Option<String>,
    pub log_path: Option<String>,
}

impl From<SessionBackgroundTaskView> for FfiSessionBackgroundTask {
    fn from(v: SessionBackgroundTaskView) -> Self {
        Self {
            id: v.id,
            kind: v.kind.into(),
            title: v.title,
            subtitle: v.subtitle,
            status: v.status.into(),
            started_at_ms: v.started_at_ms,
            command: v.command,
            output_preview: v.output_preview,
            log_path: v.log_path,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSessionSubagentEntry {
    pub id: String,
    pub role: String,
    pub prompt_preview: String,
    pub status: FfiToolStatus,
    pub child_agent_id: Option<String>,
    pub provider_subagent_id: Option<String>,
    pub last_seq: u64,
}

impl From<SessionSubagentEntryView> for FfiSessionSubagentEntry {
    fn from(v: SessionSubagentEntryView) -> Self {
        Self {
            id: v.id,
            role: v.role,
            prompt_preview: v.prompt_preview,
            status: v.status.into(),
            child_agent_id: v.child_agent_id,
            provider_subagent_id: v.provider_subagent_id,
            last_seq: v.last_seq,
        }
    }
}

use orbit_client_core::stage::SessionPlanStepStatus;
#[uniffi::remote(Enum)]
pub enum SessionPlanStepStatus {
    Pending,
    InProgress,
    Completed,
    Skipped,
    Failed,
}
use orbit_client_core::stage::SessionPlanStepView;
#[uniffi::remote(Record)]
pub struct SessionPlanStepView {
    pub id: String,
    pub title: String,
    pub status: SessionPlanStepStatus,

    pub active_form: Option<String>,
    pub updated_seq: u64,
}
use orbit_client_core::stage::SessionOutlinePromptView;
#[uniffi::remote(Record)]
pub struct SessionOutlinePromptView {
    pub seq: u64,
    pub preview: String,
}
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSessionInsights {
    pub touched_files: Vec<FfiSessionTouchedFile>,
    pub background_tasks: Vec<FfiSessionBackgroundTask>,
    pub subagents: Vec<FfiSessionSubagentEntry>,
    pub detected_local_urls: Vec<String>,
    #[uniffi(default = [])]
    pub plan_steps: Vec<SessionPlanStepView>,
    #[uniffi(default = [])]
    pub outline_prompts: Vec<SessionOutlinePromptView>,
}

impl From<SessionInsightsView> for FfiSessionInsights {
    fn from(v: SessionInsightsView) -> Self {
        Self {
            plan_steps: v.plan_steps,
            outline_prompts: v.outline_prompts,
            touched_files: v.touched_files.into_iter().map(Into::into).collect(),
            background_tasks: v.background_tasks.into_iter().map(Into::into).collect(),
            subagents: v.subagents.into_iter().map(Into::into).collect(),
            detected_local_urls: v.detected_local_urls,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAgentStage {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<FfiAgentCard>,
    pub turns: Vec<FfiStageTurn>,
    pub pending_permissions: Vec<FfiTimelineItem>,
    pub pending_questions: Vec<FfiTimelineItem>,
    pub session_turn_diffs: Vec<FfiDiffFile>,
    pub insights: FfiSessionInsights,
    pub latest_seq: u64,
}

impl From<AgentStageView> for FfiAgentStage {
    fn from(v: AgentStageView) -> Self {
        Self {
            revision: v.revision,
            host_id: v.host_id,
            agent: v.agent.map(Into::into),
            turns: v.turns.into_iter().map(Into::into).collect(),
            pending_permissions: v.pending_permissions.into_iter().map(Into::into).collect(),
            pending_questions: v.pending_questions.into_iter().map(Into::into).collect(),
            session_turn_diffs: v.session_turn_diffs.into_iter().map(Into::into).collect(),
            insights: v.insights.into(),
            latest_seq: v.latest_seq,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAgentStageDelta {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<FfiAgentCard>,
    pub replace_from_turn_index: u32,
    pub changed_turns: Vec<FfiStageTurn>,
    pub total_turn_count: u32,
    pub pending_permissions: Vec<FfiTimelineItem>,
    pub pending_questions: Vec<FfiTimelineItem>,
    pub session_turn_diffs: Vec<FfiDiffFile>,
    pub insights: FfiSessionInsights,
    pub latest_seq: u64,
    pub reset: bool,
    pub aux_revision: u64,
    pub aux_unchanged: bool,
    pub window_start_turn_index: u32,
    pub has_older_turns: bool,
    pub oldest_loaded_seq: u64,
}

impl From<AgentStageDeltaView> for FfiAgentStageDelta {
    fn from(v: AgentStageDeltaView) -> Self {
        Self {
            revision: v.revision,
            host_id: v.host_id,
            agent: v.agent.map(Into::into),
            replace_from_turn_index: v.replace_from_turn_index,
            changed_turns: v.changed_turns.into_iter().map(Into::into).collect(),
            total_turn_count: v.total_turn_count,
            pending_permissions: v.pending_permissions.into_iter().map(Into::into).collect(),
            pending_questions: v.pending_questions.into_iter().map(Into::into).collect(),
            session_turn_diffs: v.session_turn_diffs.into_iter().map(Into::into).collect(),
            insights: v.insights.into(),
            latest_seq: v.latest_seq,
            reset: v.reset,
            aux_revision: v.aux_revision,
            aux_unchanged: v.aux_unchanged,
            window_start_turn_index: v.window_start_turn_index,
            has_older_turns: v.has_older_turns,
            oldest_loaded_seq: v.oldest_loaded_seq,
        }
    }
}

// --- Unified Fleet Sidebar, Thread Prefs & Cross-Platform RPC FFI Types ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiSidebarProjectSort {
    Recent,
    Alphabetical,
    Manual,
}

impl From<FfiSidebarProjectSort> for SidebarProjectSort {
    fn from(v: FfiSidebarProjectSort) -> Self {
        match v {
            FfiSidebarProjectSort::Recent => Self::Recent,
            FfiSidebarProjectSort::Alphabetical => Self::Alphabetical,
            FfiSidebarProjectSort::Manual => Self::Manual,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiSidebarThreadFilter {
    All,
    ActiveUnread,
}

impl From<FfiSidebarThreadFilter> for SidebarThreadFilter {
    fn from(v: FfiSidebarThreadFilter) -> Self {
        match v {
            FfiSidebarThreadFilter::All => Self::All,
            FfiSidebarThreadFilter::ActiveUnread => Self::ActiveUnread,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSidebarConversation {
    pub host_id: String,
    pub agent: FfiAgentCard,
    pub is_worktree: bool,
    pub is_pinned: bool,
    pub is_unread: bool,
    pub seen_seq: u64,
}

impl From<SidebarConversationView> for FfiSidebarConversation {
    fn from(v: SidebarConversationView) -> Self {
        Self {
            host_id: v.host_id,
            agent: v.agent.into(),
            is_worktree: v.is_worktree,
            is_pinned: v.is_pinned,
            is_unread: v.is_unread,
            seen_seq: v.seen_seq,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSidebarWorkspaceGroup {
    pub workspace_id: String,
    pub title: String,
    pub mission_goal: Option<String>,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub is_worktree: bool,
    pub board_revision: u64,
    pub has_conflict: bool,
    pub peer_count: u32,
    pub running_count: u32,
    pub unread_count: u32,
    pub is_multi_agent: bool,
    pub is_curated: bool,
    pub conversations: Vec<FfiSidebarConversation>,
}

impl From<SidebarWorkspaceGroupView> for FfiSidebarWorkspaceGroup {
    fn from(v: SidebarWorkspaceGroupView) -> Self {
        Self {
            workspace_id: v.workspace_id,
            title: v.title,
            mission_goal: v.mission_goal,
            cwd: v.cwd,
            git_branch: v.git_branch,
            is_worktree: v.is_worktree,
            board_revision: v.board_revision,
            has_conflict: v.has_conflict,
            peer_count: v.peer_count,
            running_count: v.running_count,
            unread_count: v.unread_count,
            is_multi_agent: v.is_multi_agent,
            is_curated: v.is_curated,
            conversations: v.conversations.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSidebarProjectGroup {
    pub id: String,
    pub host_id: String,
    pub name: String,
    pub primary_cwd: String,
    pub directories: Vec<String>,
    pub git_branch: Option<String>,
    pub is_git_repo: bool,
    pub nested_git_repos: Vec<FfiNestedGitRepo>,
    pub pinned_project_id: Option<String>,
    pub is_home_workspace: bool,
    pub sort_weight_ms: i64,
    pub sort_order_key: String,
    pub conversations: Vec<FfiSidebarConversation>,
    pub workspaces: Vec<FfiSidebarWorkspaceGroup>,
    pub unclassified_conversations: Vec<FfiSidebarConversation>,
}

impl From<SidebarProjectGroupView> for FfiSidebarProjectGroup {
    fn from(v: SidebarProjectGroupView) -> Self {
        Self {
            id: v.id,
            host_id: v.host_id,
            name: v.name,
            primary_cwd: v.primary_cwd,
            directories: v.directories,
            git_branch: v.git_branch,
            is_git_repo: v.is_git_repo,
            nested_git_repos: v.nested_git_repos.into_iter().map(Into::into).collect(),
            pinned_project_id: v.pinned_project_id,
            is_home_workspace: v.is_home_workspace,
            sort_weight_ms: v.sort_weight_ms,
            sort_order_key: v.sort_order_key,
            conversations: v.conversations.into_iter().map(Into::into).collect(),
            workspaces: v.workspaces.into_iter().map(Into::into).collect(),
            unclassified_conversations: v
                .unclassified_conversations
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiLaunchpadProject {
    pub id: String,
    pub host_id: String,
    pub name: String,
    pub path: String,
    pub branch: Option<String>,
    pub is_git_repo: bool,
    pub nested_git_repos: Vec<FfiNestedGitRepo>,
}

impl From<LaunchpadProjectView> for FfiLaunchpadProject {
    fn from(v: LaunchpadProjectView) -> Self {
        Self {
            id: v.id,
            host_id: v.host_id,
            name: v.name,
            path: v.path,
            branch: v.branch,
            is_git_repo: v.is_git_repo,
            nested_git_repos: v.nested_git_repos.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiFleetSidebar {
    pub revision: u64,
    pub projects: Vec<FfiSidebarProjectGroup>,
    pub launchpad_projects: Vec<FfiLaunchpadProject>,
    pub needs_attention: Vec<FfiSidebarConversation>,
    pub pinned_conversations: Vec<FfiSidebarConversation>,
}

impl From<FleetSidebarView> for FfiFleetSidebar {
    fn from(v: FleetSidebarView) -> Self {
        Self {
            revision: v.revision,
            projects: v.projects.into_iter().map(Into::into).collect(),
            launchpad_projects: v.launchpad_projects.into_iter().map(Into::into).collect(),
            needs_attention: v.needs_attention.into_iter().map(Into::into).collect(),
            pinned_conversations: v.pinned_conversations.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiThreadPrefSeed {
    pub host_id: String,
    pub agent_id: String,
    pub seen_seq: u64,
    pub is_pinned: bool,
    pub pinned_order: i64,
    pub updated_at_ms: i64,
}

impl From<FfiThreadPrefSeed> for ClientThreadPref {
    fn from(v: FfiThreadPrefSeed) -> Self {
        Self {
            host_id: v.host_id,
            agent_id: v.agent_id,
            seen_seq: v.seen_seq,
            is_pinned: v.is_pinned,
            pinned_order: v.pinned_order,
            updated_at_ms: v.updated_at_ms,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiRewindAgentResult {
    pub agent: FfiAgentCard,
    pub restored_files: u32,
    pub restored_prompt: Option<String>,
    pub notice: Option<String>,
}

impl From<RewindAgentResultView> for FfiRewindAgentResult {
    fn from(v: RewindAgentResultView) -> Self {
        Self {
            agent: v.agent.into(),
            restored_files: v.restored_files,
            restored_prompt: v.restored_prompt,
            notice: v.notice,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiForkAgentResult {
    pub agent: FfiAgentCard,
    pub workspace: Option<FfiWorkspaceSummary>,
}

impl From<ForkAgentResultView> for FfiForkAgentResult {
    fn from(v: ForkAgentResultView) -> Self {
        Self {
            agent: v.agent.into(),
            workspace: v.workspace.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiWorktreeHandoffDirection {
    WorktreeToLocal,
    LocalToWorktree,
}

impl From<FfiWorktreeHandoffDirection> for WorktreeHandoffDirection {
    fn from(v: FfiWorktreeHandoffDirection) -> Self {
        match v {
            FfiWorktreeHandoffDirection::WorktreeToLocal => Self::WorktreeToLocal,
            FfiWorktreeHandoffDirection::LocalToWorktree => Self::LocalToWorktree,
        }
    }
}

impl From<WorktreeHandoffDirection> for FfiWorktreeHandoffDirection {
    fn from(v: WorktreeHandoffDirection) -> Self {
        match v {
            WorktreeHandoffDirection::WorktreeToLocal => Self::WorktreeToLocal,
            WorktreeHandoffDirection::LocalToWorktree => Self::LocalToWorktree,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiWorktreeHandoffMode {
    Apply,
    Move,
}

impl From<FfiWorktreeHandoffMode> for WorktreeHandoffMode {
    fn from(v: FfiWorktreeHandoffMode) -> Self {
        match v {
            FfiWorktreeHandoffMode::Apply => Self::Apply,
            FfiWorktreeHandoffMode::Move => Self::Move,
        }
    }
}

impl From<WorktreeHandoffMode> for FfiWorktreeHandoffMode {
    fn from(v: WorktreeHandoffMode) -> Self {
        match v {
            WorktreeHandoffMode::Apply => Self::Apply,
            WorktreeHandoffMode::Move => Self::Move,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiHandoffFileChange {
    pub path: String,
    pub status: String,
    pub insertions: u32,
    pub deletions: u32,
    pub is_untracked: bool,
    pub has_conflict: bool,
}

impl From<HandoffFileChange> for FfiHandoffFileChange {
    fn from(v: HandoffFileChange) -> Self {
        Self {
            path: v.path,
            status: v.status,
            insertions: v.insertions,
            deletions: v.deletions,
            is_untracked: v.is_untracked,
            has_conflict: v.has_conflict,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorktreeHandoffPreview {
    pub direction: FfiWorktreeHandoffDirection,
    pub source_cwd: String,
    pub target_cwd: String,
    pub source_branch: Option<String>,
    pub target_branch: Option<String>,
    pub commit_count: u32,
    pub files: Vec<FfiHandoffFileChange>,
    pub total_insertions: u32,
    pub total_deletions: u32,
    pub target_dirty: bool,
    pub conflicts: Vec<String>,
    pub can_apply_cleanly: bool,
    pub warning: Option<String>,
}

impl From<WorktreeHandoffPreview> for FfiWorktreeHandoffPreview {
    fn from(v: WorktreeHandoffPreview) -> Self {
        Self {
            direction: v.direction.into(),
            source_cwd: v.source_cwd,
            target_cwd: v.target_cwd,
            source_branch: v.source_branch,
            target_branch: v.target_branch,
            commit_count: v.commit_count,
            files: v.files.into_iter().map(Into::into).collect(),
            total_insertions: v.total_insertions,
            total_deletions: v.total_deletions,
            target_dirty: v.target_dirty,
            conflicts: v.conflicts,
            can_apply_cleanly: v.can_apply_cleanly,
            warning: v.warning,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorktreeHandoffOutcome {
    pub direction: FfiWorktreeHandoffDirection,
    pub mode: FfiWorktreeHandoffMode,
    pub applied_files: u32,
    pub backup_ref: Option<String>,
    pub target_workspace: Option<FfiWorkspaceSummary>,
    pub target_cwd: String,
    pub had_conflicts: bool,
    pub conflict_files: Vec<String>,
    pub summary: String,
}

impl From<WorktreeHandoffOutcome> for FfiWorktreeHandoffOutcome {
    fn from(v: WorktreeHandoffOutcome) -> Self {
        Self {
            direction: v.direction.into(),
            mode: v.mode.into(),
            applied_files: v.applied_files,
            backup_ref: v.backup_ref,
            target_workspace: v.target_workspace.map(Into::into),
            target_cwd: v.target_cwd,
            had_conflicts: v.had_conflicts,
            conflict_files: v.conflict_files,
            summary: v.summary,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkspaceFile {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub resolved_path: String,
    pub language: String,
    pub content_kind: String,
    pub mime_type: Option<String>,
    pub content: String,
    pub size_bytes: u64,
    pub truncated: bool,
    pub etag: Option<String>,
    pub mtime_unix_ms: Option<i64>,
    pub writable: bool,
    pub line_ending: Option<String>,
    pub highlighted_lines: Vec<FfiHighlightedLine>,
    pub markdown_blocks: Vec<FfiRichBlock>,
}

impl From<WorkspaceFileView> for FfiWorkspaceFile {
    fn from(v: WorkspaceFileView) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            path: v.path,
            resolved_path: v.resolved_path,
            language: v.language,
            content_kind: v.content_kind,
            mime_type: v.mime_type,
            content: v.content,
            size_bytes: v.size_bytes,
            truncated: v.truncated,
            etag: v.etag,
            mtime_unix_ms: v.mtime_unix_ms,
            writable: v.writable,
            line_ending: v.line_ending,
            highlighted_lines: v.highlighted_lines.into_iter().map(Into::into).collect(),
            markdown_blocks: v.markdown_blocks.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkspaceFileConflict {
    pub current_etag: String,
    pub current_content: Option<String>,
    pub current_size_bytes: u64,
    pub current_mtime_unix_ms: Option<i64>,
}

impl From<WorkspaceFileConflict> for FfiWorkspaceFileConflict {
    fn from(v: WorkspaceFileConflict) -> Self {
        Self {
            current_etag: v.current_etag,
            current_content: v.current_content,
            current_size_bytes: v.current_size_bytes,
            current_mtime_unix_ms: v.current_mtime_unix_ms,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkspaceFileWriteResult {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub etag: String,
    pub size_bytes: u64,
    pub mtime_unix_ms: Option<i64>,
    pub conflict: Option<FfiWorkspaceFileConflict>,
}

impl From<WorkspaceFileWriteResultView> for FfiWorkspaceFileWriteResult {
    fn from(v: WorkspaceFileWriteResultView) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            path: v.path,
            etag: v.etag,
            size_bytes: v.size_bytes,
            mtime_unix_ms: v.mtime_unix_ms,
            conflict: v.conflict.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWorkspaceEntryMutationResult {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub new_path: Option<String>,
    pub action: String,
}

impl From<WorkspaceEntryMutationResultView> for FfiWorkspaceEntryMutationResult {
    fn from(v: WorkspaceEntryMutationResultView) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            path: v.path,
            new_path: v.new_path,
            action: v.action,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiDiagramKind {
    Mermaid,
    Svg,
    Graphviz,
}

impl From<DiagramKind> for FfiDiagramKind {
    fn from(v: DiagramKind) -> Self {
        match v {
            DiagramKind::Mermaid => Self::Mermaid,
            DiagramKind::Svg => Self::Svg,
            DiagramKind::Graphviz => Self::Graphviz,
        }
    }
}

impl From<FfiDiagramKind> for DiagramKind {
    fn from(v: FfiDiagramKind) -> Self {
        match v {
            FfiDiagramKind::Mermaid => Self::Mermaid,
            FfiDiagramKind::Svg => Self::Svg,
            FfiDiagramKind::Graphviz => Self::Graphviz,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiDiagramTheme {
    Light,
    Dark,
}

impl From<DiagramTheme> for FfiDiagramTheme {
    fn from(v: DiagramTheme) -> Self {
        match v {
            DiagramTheme::Light => Self::Light,
            DiagramTheme::Dark => Self::Dark,
        }
    }
}

impl From<FfiDiagramTheme> for DiagramTheme {
    fn from(v: FfiDiagramTheme) -> Self {
        match v {
            FfiDiagramTheme::Light => Self::Light,
            FfiDiagramTheme::Dark => Self::Dark,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiRenderDiagramRequest {
    pub kind: FfiDiagramKind,
    pub source: String,
    pub theme: FfiDiagramTheme,
}

impl From<FfiRenderDiagramRequest> for RenderDiagramRequest {
    fn from(v: FfiRenderDiagramRequest) -> Self {
        Self {
            kind: v.kind.into(),
            source: v.source,
            theme: v.theme.into(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiRenderDiagramView {
    pub kind: FfiDiagramKind,
    pub theme: FfiDiagramTheme,
    pub svg: Option<String>,
    pub width: f32,
    pub height: f32,
    pub error: Option<String>,
    pub diagram_type_label: Option<String>,
}

impl From<RenderDiagramView> for FfiRenderDiagramView {
    fn from(v: RenderDiagramView) -> Self {
        Self {
            kind: v.kind.into(),
            theme: v.theme.into(),
            svg: v.svg,
            width: v.width,
            height: v.height,
            error: v.error,
            diagram_type_label: v.diagram_type_label,
        }
    }
}

#[uniffi::export]
pub fn detect_diagram_kind(language: String, source: String) -> Option<FfiDiagramKind> {
    core_detect_diagram_kind(&language, &source).map(Into::into)
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAskSideQuestionRequest {
    pub host_id: String,
    pub agent_id: Option<String>,
    pub cwd: String,
    pub provider: FfiProviderKind,
    pub model: Option<String>,
    pub question: String,
    pub context_summary: String,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiSideQuestionEvent {
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

impl From<SideQuestionStreamEvent> for FfiSideQuestionEvent {
    fn from(v: SideQuestionStreamEvent) -> Self {
        match v {
            SideQuestionStreamEvent::Delta {
                request_id,
                text_delta,
            } => Self::Delta {
                request_id,
                text_delta,
            },
            SideQuestionStreamEvent::Done {
                request_id,
                full_text,
                error,
            } => Self::Done {
                request_id,
                full_text,
                error,
            },
        }
    }
}

#[uniffi::export(with_foreign)]
pub trait SideQuestionListener: Send + Sync {
    fn on_side_question_event(&self, event: FfiSideQuestionEvent);
}

// --- Client Bridge Protocol v2 Canonical Records & Requests ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiBridgeInfo {
    pub protocol_version: u32,
    pub core_version: String,
    pub capabilities: Vec<String>,
}

impl From<BridgeInfoView> for FfiBridgeInfo {
    fn from(v: BridgeInfoView) -> Self {
        Self {
            protocol_version: v.protocol_version,
            core_version: v.core_version,
            capabilities: v.capabilities,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiClientPlatform {
    Desktop,
    Ios,
    Android,
    Harmony,
}

impl From<FfiClientPlatform> for ClientPlatform {
    fn from(v: FfiClientPlatform) -> Self {
        match v {
            FfiClientPlatform::Desktop => Self::Desktop,
            FfiClientPlatform::Ios => Self::Ios,
            FfiClientPlatform::Android => Self::Android,
            FfiClientPlatform::Harmony => Self::Harmony,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiSlashDirective {
    Plan,
    Goal,
    Review,
    Compact,
    Orbit,
    OrbitHandoff,
    OrbitCommittee,
    OrbitAdvisor,
}

impl From<SlashDirective> for FfiSlashDirective {
    fn from(v: SlashDirective) -> Self {
        match v {
            SlashDirective::Plan => Self::Plan,
            SlashDirective::Goal => Self::Goal,
            SlashDirective::Review => Self::Review,
            SlashDirective::Compact => Self::Compact,
            SlashDirective::Orbit => Self::Orbit,
            SlashDirective::OrbitHandoff => Self::OrbitHandoff,
            SlashDirective::OrbitCommittee => Self::OrbitCommittee,
            SlashDirective::OrbitAdvisor => Self::OrbitAdvisor,
        }
    }
}

impl From<FfiSlashDirective> for SlashDirective {
    fn from(v: FfiSlashDirective) -> Self {
        match v {
            FfiSlashDirective::Plan => Self::Plan,
            FfiSlashDirective::Goal => Self::Goal,
            FfiSlashDirective::Review => Self::Review,
            FfiSlashDirective::Compact => Self::Compact,
            FfiSlashDirective::Orbit => Self::Orbit,
            FfiSlashDirective::OrbitHandoff => Self::OrbitHandoff,
            FfiSlashDirective::OrbitCommittee => Self::OrbitCommittee,
            FfiSlashDirective::OrbitAdvisor => Self::OrbitAdvisor,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSlashCommand {
    pub id: String,
    pub command: String,
    pub label: String,
    pub description: String,
    pub shortcut_key: Option<String>,
    pub icon_key: String,
    pub keywords: Vec<String>,
    pub desktop_only: bool,
}

impl From<SlashCommandView> for FfiSlashCommand {
    fn from(v: SlashCommandView) -> Self {
        Self {
            id: v.id,
            command: v.command,
            label: v.label,
            description: v.description,
            shortcut_key: v.shortcut_key,
            icon_key: v.icon_key,
            keywords: v.keywords,
            desktop_only: v.desktop_only,
        }
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiComposerContextInput {
    Image {
        path: String,
        filename: String,
    },
    StagedFile {
        path: String,
        filename: String,
    },
    WorkspaceFile {
        path: String,
    },
    GitDiff {
        staged: bool,
        label: String,
        diff_text: Option<String>,
    },
    TerminalOutput {
        label: String,
        content: String,
    },
    AssistantQuote {
        label: String,
        content: String,
    },
}

impl From<FfiComposerContextInput> for ComposerContextInput {
    fn from(v: FfiComposerContextInput) -> Self {
        match v {
            FfiComposerContextInput::Image { path, filename } => Self::Image { path, filename },
            FfiComposerContextInput::StagedFile { path, filename } => {
                Self::StagedFile { path, filename }
            }
            FfiComposerContextInput::WorkspaceFile { path } => Self::WorkspaceFile { path },
            FfiComposerContextInput::GitDiff {
                staged,
                label,
                diff_text,
            } => Self::GitDiff {
                staged,
                label,
                diff_text,
            },
            FfiComposerContextInput::TerminalOutput { label, content } => {
                Self::TerminalOutput { label, content }
            }
            FfiComposerContextInput::AssistantQuote { label, content } => {
                Self::AssistantQuote { label, content }
            }
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiComposerQuoteInput {
    pub path: String,
    pub line_number: Option<u32>,
    pub code: String,
}

impl From<FfiComposerQuoteInput> for ComposerQuoteInput {
    fn from(v: FfiComposerQuoteInput) -> Self {
        Self {
            path: v.path,
            line_number: v.line_number,
            code: v.code,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiComposerPasteInput {
    pub label: String,
    pub filename: Option<String>,
    pub content: String,
}

impl From<FfiComposerPasteInput> for ComposerPasteInput {
    fn from(v: FfiComposerPasteInput) -> Self {
        Self {
            label: v.label,
            filename: v.filename,
            content: v.content,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiComposePromptRequest {
    pub provider: FfiProviderKind,
    pub raw_text: String,
    pub explicit_directive: Option<FfiSlashDirective>,
    pub host_id: Option<String>,
    pub cwd: Option<String>,
    pub contexts: Vec<FfiComposerContextInput>,
    pub quotes: Vec<FfiComposerQuoteInput>,
    pub pastes: Vec<FfiComposerPasteInput>,
}

impl From<FfiComposePromptRequest> for ComposePromptRequest {
    fn from(v: FfiComposePromptRequest) -> Self {
        Self {
            provider: v.provider.into(),
            raw_text: v.raw_text,
            explicit_directive: v.explicit_directive.map(Into::into),
            host_id: v.host_id,
            cwd: v.cwd,
            contexts: v.contexts.into_iter().map(Into::into).collect(),
            quotes: v.quotes.into_iter().map(Into::into).collect(),
            pastes: v.pastes.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiFastModeSlashAction {
    Toggle,
    Enable,
    Disable,
}

impl From<FastModeSlashAction> for FfiFastModeSlashAction {
    fn from(v: FastModeSlashAction) -> Self {
        match v {
            FastModeSlashAction::Toggle => Self::Toggle,
            FastModeSlashAction::Enable => Self::Enable,
            FastModeSlashAction::Disable => Self::Disable,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiComposedPrompt {
    pub wire_prompt: String,
    pub display_prompt: String,
    pub badges: Vec<FfiPromptBadge>,
    pub effective_directive: Option<FfiSlashDirective>,
    pub is_compact: bool,
    pub is_btw: bool,
    pub btw_question: Option<String>,
    pub is_fork: bool,
    pub fork_worktree: bool,
    pub fork_branch: Option<String>,
    pub fork_prompt: Option<String>,
    pub fast_mode_action: Option<FfiFastModeSlashAction>,
    pub is_empty: bool,
}

impl From<ComposedPromptView> for FfiComposedPrompt {
    fn from(v: ComposedPromptView) -> Self {
        Self {
            wire_prompt: v.wire_prompt,
            display_prompt: v.display_prompt,
            badges: v.badges.into_iter().map(Into::into).collect(),
            effective_directive: v.effective_directive.map(Into::into),
            is_compact: v.is_compact,
            is_btw: v.is_btw,
            btw_question: v.btw_question,
            is_fork: v.is_fork,
            fork_worktree: v.fork_worktree,
            fork_branch: v.fork_branch,
            fork_prompt: v.fork_prompt,
            fast_mode_action: v.fast_mode_action.map(Into::into),
            is_empty: v.is_empty,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiMentionCandidate {
    pub key: String,
    pub kind: String,
    pub token_label: String,
    pub title: String,
    pub subtitle: String,
    pub file_path: String,
    pub git_status: String,
    pub score: i32,
}

impl From<MentionCandidateView> for FfiMentionCandidate {
    fn from(v: MentionCandidateView) -> Self {
        Self {
            key: v.key,
            kind: v.kind,
            token_label: v.token_label,
            title: v.title,
            subtitle: v.subtitle,
            file_path: v.file_path,
            git_status: v.git_status,
            score: v.score,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiMentionRankRequest {
    pub host_id: String,
    pub cwd: String,
    pub query: String,
    pub limit: u32,
}

impl From<FfiMentionRankRequest> for MentionRankRequest {
    fn from(v: FfiMentionRankRequest) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            query: v.query,
            limit: v.limit as usize,
            workspace_id: None,
            active_agent_id: None,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiModelSelectionRequest {
    pub host_id: Option<String>,
    pub provider: FfiProviderKind,
    pub desired_model: Option<String>,
    pub desired_effort: Option<FfiReasoningEffort>,
}

impl From<FfiModelSelectionRequest> for ModelSelectionRequest {
    fn from(v: FfiModelSelectionRequest) -> Self {
        Self {
            host_id: v.host_id,
            provider: v.provider.into(),
            desired_model: v.desired_model,
            desired_effort: v.desired_effort.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiResolvedModelSelection {
    pub provider: FfiProviderKind,
    pub catalog: FfiProviderModelCatalog,
    pub selected_model_id: Option<String>,
    pub selected_model_label: String,
    pub selected_effort: Option<FfiReasoningEffort>,
    pub selected_effort_label: Option<String>,
    pub thinking_options: Vec<FfiThinkingOptionSpec>,
    pub supports_fast_mode: bool,
}

impl From<ResolvedModelSelectionView> for FfiResolvedModelSelection {
    fn from(v: ResolvedModelSelectionView) -> Self {
        Self {
            provider: v.provider.into(),
            catalog: v.catalog.into(),
            selected_model_id: v.selected_model_id,
            selected_model_label: v.selected_model_label,
            selected_effort: v.selected_effort.map(Into::into),
            selected_effort_label: v.selected_effort_label,
            thinking_options: v.thinking_options.into_iter().map(Into::into).collect(),
            supports_fast_mode: v.supports_fast_mode,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAcpRegistryRefreshResult {
    pub agents: Vec<FfiAcpAgentAvailability>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiCreateSessionRequest {
    pub host_id: String,
    pub cwd: String,
    pub extra_dirs: Vec<String>,
    pub provider: FfiProviderKind,
    pub acp_agent_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<FfiReasoningEffort>,
    pub fast_mode: Option<bool>,
    pub mode: FfiSessionMode,
    pub initial_prompt: String,
    pub initial_display_prompt: Option<String>,
    pub initial_badges: Vec<FfiPromptBadge>,
    pub worktree_branch: Option<String>,
}

impl From<FfiCreateSessionRequest> for CreateSessionRequest {
    fn from(v: FfiCreateSessionRequest) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            extra_dirs: v.extra_dirs,
            provider: v.provider.into(),
            acp_agent_id: v.acp_agent_id,
            model: v.model,
            effort: v.effort.map(Into::into),
            fast_mode: v.fast_mode,
            mode: v.mode.into(),
            initial_prompt: v.initial_prompt,
            initial_display_prompt: v.initial_display_prompt,
            initial_badges: v.initial_badges.into_iter().map(Into::into).collect(),
            worktree_branch: v.worktree_branch,
            workspace_id: None,
            handle: None,
            auto_cluster: None,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSendPromptRequest {
    pub host_id: String,
    pub agent_id: String,
    pub prompt: String,
    pub display_prompt: Option<String>,
    pub badges: Vec<FfiPromptBadge>,
}

impl From<FfiSendPromptRequest> for SendPromptRequest {
    fn from(v: FfiSendPromptRequest) -> Self {
        Self {
            host_id: v.host_id,
            agent_id: v.agent_id,
            prompt: v.prompt,
            display_prompt: v.display_prompt,
            badges: v.badges.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiUpdateHostConfigRequest {
    pub host_id: String,
    pub custom_name: Option<String>,
    pub route_mode: Option<FfiHostRouteMode>,
    pub ssh_alias: Option<String>,
    pub remote_daemon_port: Option<u16>,
    pub custom_direct_endpoint: Option<String>,
    pub relay_url: Option<String>,
    pub reconnect: bool,
}

impl From<FfiUpdateHostConfigRequest> for UpdateHostConfigRequest {
    fn from(v: FfiUpdateHostConfigRequest) -> Self {
        Self {
            host_id: v.host_id,
            custom_name: v.custom_name,
            route_mode: v.route_mode.map(Into::into),
            ssh_alias: v.ssh_alias,
            remote_daemon_port: v.remote_daemon_port,
            custom_direct_endpoint: v.custom_direct_endpoint,
            relay_url: v.relay_url,
            reconnect: v.reconnect,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiFleetSidebarRequest {
    pub sort: FfiSidebarProjectSort,
    pub filter: FfiSidebarThreadFilter,
    pub query: String,
    pub active_host_id: Option<String>,
    pub active_agent_id: Option<String>,
    pub active_cwd: Option<String>,
}

impl From<FfiFleetSidebarRequest> for FleetSidebarRequest {
    fn from(v: FfiFleetSidebarRequest) -> Self {
        Self {
            sort: v.sort.into(),
            filter: v.filter.into(),
            query: v.query,
            active_host_id: v.active_host_id,
            active_agent_id: v.active_agent_id,
            active_cwd: v.active_cwd,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiAgentStageDeltaRequest {
    pub host_id: String,
    pub agent_id: String,
    pub since_revision: u64,
    pub since_aux_revision: Option<u64>,
    pub turn_limit: Option<u32>,
    pub since_window_start_turn_index: Option<u32>,
}

impl From<FfiAgentStageDeltaRequest> for AgentStageDeltaRequest {
    fn from(v: FfiAgentStageDeltaRequest) -> Self {
        Self {
            host_id: v.host_id,
            agent_id: v.agent_id,
            since_revision: v.since_revision,
            since_aux_revision: v.since_aux_revision,
            turn_limit: v.turn_limit,
            since_window_start_turn_index: v.since_window_start_turn_index,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiRewindAgentRequest {
    pub host_id: String,
    pub agent_id: String,
    pub to_before_seq: u64,
    pub restore_files: bool,
}

impl From<FfiRewindAgentRequest> for RewindAgentRequest {
    fn from(v: FfiRewindAgentRequest) -> Self {
        Self {
            host_id: v.host_id,
            agent_id: v.agent_id,
            to_before_seq: v.to_before_seq,
            restore_files: v.restore_files,
            restore_paths: None,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiForkAgentRequest {
    pub host_id: String,
    pub source_agent_id: String,
    pub up_to_seq: Option<u64>,
    pub worktree: bool,
    pub branch: Option<String>,
    pub base_ref: Option<String>,
    pub title: Option<String>,
    pub initial_prompt: Option<String>,
}

impl From<FfiForkAgentRequest> for ForkAgentRequest {
    fn from(v: FfiForkAgentRequest) -> Self {
        let target = if v.worktree || v.branch.is_some() || v.base_ref.is_some() {
            ForkTarget::NewWorktree {
                branch: v.branch,
                base_ref: v.base_ref,
            }
        } else {
            ForkTarget::SameDirectory
        };
        Self {
            host_id: v.host_id,
            source_agent_id: v.source_agent_id,
            up_to_seq: v.up_to_seq,
            target,
            title: v.title,
            initial_prompt: v.initial_prompt,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiPreviewWorktreeHandoffRequest {
    pub host_id: String,
    pub workspace_id: String,
    pub source_agent_id: Option<String>,
    pub direction: FfiWorktreeHandoffDirection,
    pub target_workspace_id: Option<String>,
    pub include_committed: bool,
}

impl From<FfiPreviewWorktreeHandoffRequest> for PreviewWorktreeHandoffRequest {
    fn from(v: FfiPreviewWorktreeHandoffRequest) -> Self {
        Self {
            host_id: v.host_id,
            workspace_id: v.workspace_id,
            source_agent_id: v.source_agent_id,
            direction: v.direction.into(),
            target_workspace_id: v.target_workspace_id,
            include_committed: v.include_committed,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiExecuteWorktreeHandoffRequest {
    pub host_id: String,
    pub workspace_id: String,
    pub source_agent_id: Option<String>,
    pub direction: FfiWorktreeHandoffDirection,
    pub mode: FfiWorktreeHandoffMode,
    pub target_workspace_id: Option<String>,
    pub include_committed: bool,
    pub branch: Option<String>,
    pub allow_three_way_conflicts: bool,
}

impl From<FfiExecuteWorktreeHandoffRequest> for ExecuteWorktreeHandoffRequest {
    fn from(v: FfiExecuteWorktreeHandoffRequest) -> Self {
        Self {
            host_id: v.host_id,
            workspace_id: v.workspace_id,
            source_agent_id: v.source_agent_id,
            direction: v.direction.into(),
            mode: v.mode.into(),
            target_workspace_id: v.target_workspace_id,
            include_committed: v.include_committed,
            branch: v.branch,
            allow_three_way_conflicts: v.allow_three_way_conflicts,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiReadWorkspaceFileRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
}

impl From<FfiReadWorkspaceFileRequest> for ReadWorkspaceFileRequest {
    fn from(v: FfiReadWorkspaceFileRequest) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            path: v.path,
            offset_bytes: None,
            max_bytes: None,
            expected_etag: None,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiWriteWorkspaceFileRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub content: String,
    pub expected_etag: Option<String>,
    pub force: bool,
}

impl From<FfiWriteWorkspaceFileRequest> for WriteWorkspaceFileRequest {
    fn from(v: FfiWriteWorkspaceFileRequest) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            path: v.path,
            content: v.content,
            expected_etag: v.expected_etag,
            force: v.force,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiCreateWorkspaceEntryRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
    pub is_dir: bool,
    pub initial_content: Option<String>,
}

impl From<FfiCreateWorkspaceEntryRequest> for CreateWorkspaceEntryRequest {
    fn from(v: FfiCreateWorkspaceEntryRequest) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            path: v.path,
            is_dir: v.is_dir,
            initial_content: v.initial_content,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiRenameWorkspaceEntryRequest {
    pub host_id: String,
    pub cwd: String,
    pub old_path: String,
    pub new_path: String,
}

impl From<FfiRenameWorkspaceEntryRequest> for RenameWorkspaceEntryRequest {
    fn from(v: FfiRenameWorkspaceEntryRequest) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            old_path: v.old_path,
            new_path: v.new_path,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiDeleteWorkspaceEntryRequest {
    pub host_id: String,
    pub cwd: String,
    pub path: String,
}

impl From<FfiDeleteWorkspaceEntryRequest> for DeleteWorkspaceEntryRequest {
    fn from(v: FfiDeleteWorkspaceEntryRequest) -> Self {
        Self {
            host_id: v.host_id,
            cwd: v.cwd,
            path: v.path,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSpawnWorkspacePeerRequest {
    pub host_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub extra_dirs: Vec<String>,
    pub provider: FfiProviderKind,
    pub acp_agent_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<FfiReasoningEffort>,
    pub fast_mode: Option<bool>,
    pub mode: FfiSessionMode,
    pub initial_prompt: String,
    pub worktree_branch: Option<String>,
    pub handle: Option<String>,
    pub profile_id: Option<String>,
}

impl From<FfiSpawnWorkspacePeerRequest> for SpawnWorkspacePeerRequest {
    fn from(v: FfiSpawnWorkspacePeerRequest) -> Self {
        Self {
            host_id: v.host_id,
            workspace_id: v.workspace_id,
            cwd: v.cwd,
            extra_dirs: v.extra_dirs,
            provider: v.provider.into(),
            acp_agent_id: v.acp_agent_id,
            model: v.model,
            effort: v.effort.map(Into::into),
            fast_mode: v.fast_mode,
            mode: v.mode.into(),
            initial_prompt: v.initial_prompt,
            worktree_branch: v.worktree_branch,
            handle: v.handle,
            profile_id: v.profile_id,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiCreateScheduleRequest {
    pub host_id: String,
    pub name: String,
    pub cron_expr: String,
    pub timezone: Option<String>,
    pub prompt: String,
    pub cwd: String,
    pub workspace_id: Option<String>,
    pub provider: FfiProviderKind,
    pub acp_agent_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<FfiReasoningEffort>,
    pub fast_mode: bool,
    pub mode: FfiSessionMode,
    pub worktree: bool,
    pub archive_on_finish: bool,
    pub max_runs: Option<u32>,
}

// --- Git Review & Terminal Grid ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiGitFileEntry {
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub untracked: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiGitReview {
    pub revision: u64,
    pub host_id: String,
    pub cwd: String,
    pub is_git_repo: bool,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub selected_path: Option<String>,
    pub status_reverify_pending: bool,
    pub raw_diff: String,
    pub changed_files: Vec<FfiGitFileEntry>,
    pub diff_files: Vec<FfiDiffFile>,
}

impl From<GitReviewView> for FfiGitReview {
    fn from(v: GitReviewView) -> Self {
        let (is_git_repo, branch, upstream, ahead, behind, changed_files) = match v.status {
            Some(s) => (
                s.is_git_repo,
                s.branch,
                s.upstream,
                s.ahead,
                s.behind,
                s.files
                    .into_iter()
                    .map(|f| FfiGitFileEntry {
                        path: f.path,
                        status: f.status,
                        staged: f.staged,
                        untracked: f.untracked,
                    })
                    .collect(),
            ),
            None => (false, None, None, 0, 0, Vec::new()),
        };
        Self {
            revision: v.revision,
            host_id: v.host_id,
            cwd: v.cwd,
            is_git_repo,
            branch,
            upstream,
            ahead,
            behind,
            selected_path: v.selected_path,
            status_reverify_pending: v.status_reverify_pending,
            raw_diff: v.raw_diff,
            changed_files,
            diff_files: v.diff_files.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTerminalSpan {
    pub text: String,
    pub fg_hex: String,
    pub bg_hex: Option<String>,
    pub bold: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl From<TerminalSpanView> for FfiTerminalSpan {
    fn from(s: TerminalSpanView) -> Self {
        Self {
            text: s.text,
            fg_hex: s.fg_hex,
            bg_hex: s.bg_hex,
            bold: s.bold,
            underline: s.underline,
            inverse: s.inverse,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTerminalRow {
    pub row_index: u16,
    pub spans: Vec<FfiTerminalSpan>,
}

impl From<TerminalRowView> for FfiTerminalRow {
    fn from(r: TerminalRowView) -> Self {
        Self {
            row_index: r.row_index,
            spans: r.spans.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTerminalScreen {
    pub cols: u16,
    pub rows: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub rows_data: Vec<FfiTerminalRow>,
}

impl From<TerminalScreenView> for FfiTerminalScreen {
    fn from(v: TerminalScreenView) -> Self {
        Self {
            cols: v.cols,
            rows: v.rows,
            cursor_row: v.cursor_row,
            cursor_col: v.cursor_col,
            rows_data: v.rows_data.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiStagedAttachment {
    pub path: String,
    pub filename: String,
    pub size_bytes: u64,
}

impl From<StagedAttachmentView> for FfiStagedAttachment {
    fn from(v: StagedAttachmentView) -> Self {
        Self {
            path: v.path,
            filename: v.filename,
            size_bytes: v.size_bytes,
        }
    }
}

// --- Pure-Rust Catalog & Formatting Helpers Exported to Swift & Kotlin ---

#[uniffi::export]
pub fn fallback_provider_catalog(provider: FfiProviderKind) -> FfiProviderModelCatalog {
    core_fallback_provider_catalog(provider.into()).into()
}

#[uniffi::export]
pub fn fallback_provider_catalogs() -> Vec<FfiProviderModelCatalog> {
    ProviderKind::all()
        .iter()
        .copied()
        .map(|p| core_fallback_provider_catalog(p).into())
        .collect()
}

#[uniffi::export]
pub fn format_effort_label(effort: FfiReasoningEffort) -> String {
    core_format_effort_label(effort.into()).to_string()
}

#[uniffi::export]
pub fn default_effort_description(effort: FfiReasoningEffort) -> String {
    core_default_effort_description(effort.into()).to_string()
}

#[uniffi::export]
pub fn format_catalog_model_label(
    catalog: FfiProviderModelCatalog,
    raw_model_id: Option<String>,
) -> String {
    let core_cat: ProviderModelCatalog = catalog.into();
    core_format_catalog_model_label(&core_cat, raw_model_id.as_deref())
}

#[uniffi::export]
pub fn resolve_supported_effort_for_model(
    spec: Option<FfiProviderModelSpec>,
    desired: Option<FfiReasoningEffort>,
    catalog_default: Option<FfiReasoningEffort>,
) -> Option<FfiReasoningEffort> {
    let core_spec: Option<ProviderModelSpec> = spec.map(Into::into);
    core_resolve_supported_effort_for_model(
        core_spec.as_ref(),
        desired.map(Into::into),
        catalog_default.map(Into::into),
    )
    .map(Into::into)
}

#[uniffi::export]
pub fn parse_markdown_preview(markdown: String, streaming: bool) -> Vec<FfiRichBlock> {
    core_parse_markdown_blocks(&markdown, streaming)
        .into_iter()
        .map(Into::into)
        .collect()
}

struct ThreadWaker(thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Blocks the calling FFI thread on a future that spawns its real work onto `OrbitAppCore`'s
/// internal Tokio runtime and only awaits a bounded `tokio::sync::oneshot::Receiver`.
///
/// Precondition: MUST ONLY be called with `OrbitAppCore::*_on_host` RPC wrappers whose top-level
/// future performs no direct Tokio timer/IO reactor calls outside `self.runtime.spawn(...)`.
fn block_on_oneshot_future<F: Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => thread::park(),
        }
    }
}

#[derive(uniffi::Object)]
pub struct OrbitMobileCore {
    inner: Arc<OrbitAppCore>,
}

#[uniffi::export]
impl OrbitMobileCore {
    #[uniffi::constructor]
    pub fn new_with_master_key(
        db_path: String,
        master_key_bytes: Vec<u8>,
    ) -> Result<Arc<Self>, FfiOrbitError> {
        let raw = zeroize::Zeroizing::new(master_key_bytes);
        let key: [u8; 32] = raw.as_slice().try_into().map_err(|_| FfiOrbitError::Core {
            detail: "expected a 32-byte master key".into(),
        })?;
        let key = zeroize::Zeroizing::new(key);
        let inner = OrbitAppCore::open_with_master_key(db_path, Some(*key)).map_err(|e| {
            FfiOrbitError::Core {
                detail: e.to_string(),
            }
        })?;
        Ok(Arc::new(Self { inner }))
    }

    #[uniffi::constructor]
    pub fn open(db_path: String) -> Result<Arc<Self>, FfiOrbitError> {
        let inner = OrbitAppCore::open(&db_path, false).map_err(|e| FfiOrbitError::Core {
            detail: e.to_string(),
        })?;
        Ok(Arc::new(Self { inner }))
    }

    #[uniffi::constructor]
    pub fn open_with_options(
        db_path: String,
        auto_discover_local: bool,
    ) -> Result<Arc<Self>, FfiOrbitError> {
        let inner =
            OrbitAppCore::open(&db_path, auto_discover_local).map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })?;
        Ok(Arc::new(Self { inner }))
    }

    #[uniffi::constructor]
    pub fn open_in_memory() -> Result<Arc<Self>, FfiOrbitError> {
        let inner = OrbitAppCore::open_in_memory().map_err(|e| FfiOrbitError::Core {
            detail: e.to_string(),
        })?;
        Ok(Arc::new(Self { inner }))
    }

    pub fn subscribe(&self, listener: Arc<dyn MobileCoreListener>) {
        self.inner
            .subscribe(Arc::new(BridgeSubscriber { foreign: listener }));
    }

    pub fn subscribe_throttled(&self, listener: Arc<dyn MobileCoreListener>, interval_ms: u32) {
        self.inner.subscribe_coalesced(
            u64::from(interval_ms),
            Arc::new(BridgeSubscriber { foreign: listener }),
        );
    }

    pub fn discover_local_daemon_uri(&self) -> Option<String> {
        self.inner.discover_local_daemon_uri()
    }

    pub fn fleet_overview(&self) -> FfiFleetOverview {
        self.inner.fleet_overview().into()
    }

    pub fn bridge_info(&self) -> FfiBridgeInfo {
        self.inner.bridge_info().into()
    }

    pub fn search_messages(&self, query: String, limit: u32) -> Vec<FfiConversationSearchHit> {
        self.inner
            .search_messages(query, limit)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn export_agent_transcript(
        &self,
        host_id: String,
        agent_id: String,
        options: FfiExportOptions,
    ) -> Result<FfiExportedTranscript, FfiOrbitError> {
        let core_opts: ExportOptions = options.into();
        self.inner
            .export_agent_transcript(&host_id, &agent_id, &core_opts)
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn list_agent_code_blocks(
        &self,
        host_id: String,
        agent_id: String,
    ) -> Result<Vec<FfiExtractedCodeBlock>, FfiOrbitError> {
        self.inner
            .list_agent_code_blocks(&host_id, &agent_id)
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn fleet_sidebar_request(&self, req: FfiFleetSidebarRequest) -> FfiFleetSidebar {
        let core_req: FleetSidebarRequest = req.into();
        self.inner.fleet_sidebar_by_request(&core_req).into()
    }

    pub fn agent_stage_delta_request(&self, req: FfiAgentStageDeltaRequest) -> FfiAgentStageDelta {
        let core_req: AgentStageDeltaRequest = req.into();
        self.inner.agent_stage_delta_by_request(&core_req).into()
    }

    pub fn agent_stage_delta_with_aux(
        &self,
        host_id: String,
        agent_id: String,
        since_revision: u64,
        since_aux_revision: Option<u64>,
    ) -> FfiAgentStageDelta {
        self.inner
            .agent_stage_delta_with_aux(&host_id, &agent_id, since_revision, since_aux_revision)
            .into()
    }

    pub fn create_session(&self, req: FfiCreateSessionRequest) -> String {
        self.inner.create_session(req.into())
    }

    pub fn send_prompt_request(&self, req: FfiSendPromptRequest) {
        self.inner.send_prompt_request(req.into());
    }

    pub fn update_host_config_request(
        &self,
        req: FfiUpdateHostConfigRequest,
    ) -> Result<FfiHostCard, FfiOrbitError> {
        self.inner
            .update_host_config_request(req.into())
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn rewind_agent_request(
        &self,
        req: FfiRewindAgentRequest,
    ) -> Result<FfiRewindAgentResult, FfiOrbitError> {
        self.rewind_agent(
            req.host_id,
            req.agent_id,
            req.to_before_seq,
            req.restore_files,
        )
    }

    pub fn fork_agent_request(
        &self,
        req: FfiForkAgentRequest,
    ) -> Result<FfiForkAgentResult, FfiOrbitError> {
        self.inner
            .fork_agent_blocking(req.into())
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn preview_worktree_handoff(
        &self,
        req: FfiPreviewWorktreeHandoffRequest,
    ) -> Result<FfiWorktreeHandoffPreview, FfiOrbitError> {
        self.inner
            .preview_worktree_handoff_blocking(req.into())
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn execute_worktree_handoff(
        &self,
        req: FfiExecuteWorktreeHandoffRequest,
    ) -> Result<FfiWorktreeHandoffOutcome, FfiOrbitError> {
        self.inner
            .execute_worktree_handoff_blocking(req.into())
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn read_workspace_file_request(
        &self,
        req: FfiReadWorkspaceFileRequest,
    ) -> Result<FfiWorkspaceFile, FfiOrbitError> {
        self.read_workspace_file(req.host_id, req.cwd, req.path)
    }

    pub fn resolve_model_selection(
        &self,
        req: FfiModelSelectionRequest,
    ) -> FfiResolvedModelSelection {
        let core_req: ModelSelectionRequest = req.into();
        self.inner.resolve_model_selection(&core_req).into()
    }

    pub fn slash_commands(
        &self,
        platform: FfiClientPlatform,
        query: Option<String>,
    ) -> Vec<FfiSlashCommand> {
        self.inner
            .slash_commands(platform.into(), query.as_deref())
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn compose_prompt(&self, req: FfiComposePromptRequest) -> FfiComposedPrompt {
        let core_req: ComposePromptRequest = req.into();
        self.inner.compose_prompt_for_host(&core_req).into()
    }

    pub fn rank_mention_candidates(&self, req: FfiMentionRankRequest) -> Vec<FfiMentionCandidate> {
        let core_req: MentionRankRequest = req.into();
        self.inner
            .rank_mention_candidates(&core_req)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn host_detail(&self, host_id: String) -> Option<FfiHostDetail> {
        self.inner.host_detail(&host_id).map(Into::into)
    }

    pub fn agent_timeline(&self, host_id: String, agent_id: String) -> FfiAgentTimeline {
        self.inner.agent_timeline(&host_id, &agent_id).into()
    }

    pub fn agent_timeline_delta(
        &self,
        host_id: String,
        agent_id: String,
        since_seq: u64,
    ) -> FfiAgentTimelineDelta {
        self.inner
            .agent_timeline_delta(&host_id, &agent_id, since_seq)
            .into()
    }

    pub fn agent_stage(&self, host_id: String, agent_id: String) -> FfiAgentStage {
        self.inner.agent_stage(&host_id, &agent_id).into()
    }

    pub fn agent_stage_delta(
        &self,
        host_id: String,
        agent_id: String,
        since_revision: u64,
    ) -> FfiAgentStageDelta {
        self.inner
            .agent_stage_delta(&host_id, &agent_id, since_revision)
            .into()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fleet_sidebar(
        &self,
        sort: FfiSidebarProjectSort,
        filter: FfiSidebarThreadFilter,
        query: String,
        active_host_id: Option<String>,
        active_agent_id: Option<String>,
        active_cwd: Option<String>,
    ) -> FfiFleetSidebar {
        self.inner
            .fleet_sidebar(
                sort.into(),
                filter.into(),
                &query,
                active_host_id.as_deref(),
                active_agent_id.as_deref(),
                active_cwd.as_deref(),
            )
            .into()
    }

    pub fn mark_agent_seen(&self, host_id: String, agent_id: String, seq: u64) {
        self.inner.mark_agent_seen(&host_id, &agent_id, seq);
    }

    pub fn mark_agent_unread(&self, host_id: String, agent_id: String) {
        self.inner.mark_agent_unread(&host_id, &agent_id);
    }

    pub fn set_agent_pinned(&self, host_id: String, agent_id: String, pinned: bool) {
        self.inner.set_agent_pinned(&host_id, &agent_id, pinned);
    }

    pub fn toggle_pin_agent(&self, host_id: String, agent_id: String) -> bool {
        self.inner.toggle_pin_agent(&host_id, &agent_id)
    }

    pub fn seed_thread_prefs(&self, seeds: Vec<FfiThreadPrefSeed>) {
        self.inner
            .seed_thread_prefs(seeds.into_iter().map(Into::into).collect());
    }

    pub fn fallback_provider_catalogs(&self) -> Vec<FfiProviderModelCatalog> {
        self.inner
            .fallback_provider_catalogs()
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn describe_tool_item(
        &self,
        item: FfiTimelineItem,
        cwd: Option<String>,
    ) -> Option<FfiToolStepDescriptor> {
        match item {
            FfiTimelineItem::ToolCall {
                status,
                summary_title,
                detail,
                diff_files,
                ..
            } => {
                let core_detail: ToolCallDetail = detail.into();
                let core_diffs: Vec<DiffFileView> =
                    diff_files.into_iter().map(Into::into).collect();
                Some(
                    describe_tool_detail(
                        status.into(),
                        &summary_title,
                        &core_detail,
                        &core_diffs,
                        cwd.as_deref(),
                    )
                    .into(),
                )
            }
            _ => None,
        }
    }

    pub fn git_review(&self, host_id: String, cwd: String, path: Option<String>) -> FfiGitReview {
        self.inner
            .git_review(&host_id, &cwd, path.as_deref())
            .into()
    }

    pub fn terminal_screen(&self, host_id: String) -> Option<FfiTerminalScreen> {
        self.inner.terminal_screen(&host_id).map(Into::into)
    }

    pub fn terminal_scrollback(&self, host_id: String) -> Vec<String> {
        self.inner.terminal_scrollback(&host_id)
    }

    pub fn terminal_screen_slot(&self, host_id: String, slot: u8) -> Option<FfiTerminalScreen> {
        self.inner
            .terminal_screen_slot(&host_id, slot)
            .map(Into::into)
    }

    pub fn pair_host(&self, pairing_uri: String) -> Result<FfiHostCard, FfiOrbitError> {
        self.inner
            .pair_host(&pairing_uri)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn remove_host(&self, host_id: String) -> Result<(), FfiOrbitError> {
        self.inner
            .remove_host(&host_id)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn update_host_relay(
        &self,
        host_id: String,
        relay_url: Option<String>,
    ) -> Result<(), FfiOrbitError> {
        self.inner
            .update_host_relay(&host_id, relay_url.as_deref())
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn apply_relay_to_fleet(&self, raw_relay_url: String) -> Result<u32, FfiOrbitError> {
        self.inner
            .apply_relay_to_fleet(&raw_relay_url)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_host_config(
        &self,
        host_id: String,
        custom_name: Option<String>,
        route_mode: Option<FfiHostRouteMode>,
        ssh_alias: Option<String>,
        remote_daemon_port: Option<u16>,
        custom_direct_endpoint: Option<String>,
        relay_url: Option<String>,
        reconnect: bool,
    ) -> Result<FfiHostCard, FfiOrbitError> {
        self.inner
            .update_host_config(
                &host_id,
                custom_name,
                route_mode.map(Into::into),
                ssh_alias,
                remote_daemon_port,
                custom_direct_endpoint,
                relay_url,
                reconnect,
            )
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn upsert_project(
        &self,
        project_id: Option<String>,
        host_id: String,
        name: String,
        directories: Vec<String>,
    ) -> Result<FfiClientProject, FfiOrbitError> {
        self.inner
            .upsert_project(project_id.as_deref(), &host_id, &name, directories)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn remove_project(&self, project_id: String) -> Result<(), FfiOrbitError> {
        self.inner
            .remove_project(&project_id)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn list_directory(&self, host_id: String, path: Option<String>) {
        self.inner.list_directory(&host_id, path);
    }

    pub fn connect_all_hosts(&self) {
        self.inner.connect_all_hosts();
    }

    pub fn connect_host(&self, host_id: String) {
        self.inner.connect_host_async(host_id);
    }

    pub fn refresh_host(&self, host_id: String) {
        self.inner.refresh_host(&host_id);
    }

    pub fn refresh_quotas(&self, host_id: String) {
        self.inner.refresh_quotas(&host_id);
    }

    pub fn refresh_provider_models(&self, host_id: String, force_refresh: bool) {
        self.inner.refresh_provider_models(&host_id, force_refresh);
    }

    pub fn create_agent(
        &self,
        host_id: String,
        cwd: String,
        provider: FfiProviderKind,
        mode: FfiSessionMode,
        initial_prompt: String,
        worktree_branch: Option<String>,
    ) {
        self.inner.create_agent(
            &host_id,
            &cwd,
            provider.into(),
            mode.into(),
            &initial_prompt,
            worktree_branch.as_deref(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_full(
        &self,
        host_id: String,
        cwd: String,
        extra_dirs: Vec<String>,
        provider: FfiProviderKind,
        acp_agent_id: Option<String>,
        model: Option<String>,
        effort: Option<FfiReasoningEffort>,
        mode: FfiSessionMode,
        initial_prompt: String,
        initial_display_prompt: Option<String>,
        initial_badges: Vec<FfiPromptBadge>,
        worktree_branch: Option<String>,
    ) -> String {
        self.inner.create_agent_with_config(CreateAgentRequest {
            host_id: &host_id,
            cwd: &cwd,
            extra_dirs,
            provider: provider.into(),
            acp_agent_id,
            model,
            effort: effort.map(Into::into),
            fast_mode: None,
            mode: mode.into(),
            initial_prompt: &initial_prompt,
            initial_display_prompt,
            initial_badges: initial_badges.into_iter().map(Into::into).collect(),
            worktree_branch: worktree_branch.as_deref(),
            workspace_id: None,
            handle: None,
            auto_cluster: None,
        })
    }

    pub fn open_agent_timeline(&self, host_id: String, agent_id: String) {
        self.inner.open_agent_timeline(&host_id, &agent_id);
    }

    pub fn load_older_agent_timeline(
        &self,
        host_id: String,
        agent_id: String,
        limit_events: u32,
    ) -> bool {
        self.inner
            .load_older_agent_timeline(&host_id, &agent_id, limit_events)
    }

    pub fn get_tool_call_diff(
        &self,
        host_id: String,
        agent_id: String,
        seq: u64,
    ) -> Vec<FfiDiffFile> {
        self.inner
            .get_tool_call_diff(&host_id, &agent_id, seq)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn send_prompt(&self, host_id: String, agent_id: String, prompt: String) {
        self.inner.send_prompt(&host_id, &agent_id, &prompt);
    }

    pub fn set_account_default_mode(&self, mode: FfiSessionMode, updated_at_ms: i64) {
        self.inner
            .set_account_default_mode(mode.into(), updated_at_ms);
    }

    pub fn send_prompt_with_meta(
        &self,
        host_id: String,
        agent_id: String,
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<FfiPromptBadge>,
    ) {
        self.inner.send_prompt_with_meta(
            &host_id,
            &agent_id,
            &prompt,
            display_prompt,
            badges.into_iter().map(Into::into).collect(),
        );
    }

    pub fn enqueue_prompt(
        &self,
        host_id: String,
        agent_id: String,
        prompt: String,
        display_prompt: Option<String>,
        badges: Vec<FfiPromptBadge>,
        queue_id: Option<String>,
    ) -> String {
        self.inner.enqueue_prompt(
            &host_id,
            &agent_id,
            &prompt,
            display_prompt,
            badges.into_iter().map(Into::into).collect(),
            queue_id,
        )
    }

    pub fn remove_queued_prompt(&self, host_id: String, agent_id: String, queue_id: String) {
        self.inner
            .remove_queued_prompt(&host_id, &agent_id, &queue_id);
    }

    pub fn flush_queued_prompt_now(&self, host_id: String, agent_id: String, queue_id: String) {
        self.inner
            .flush_queued_prompt_now(&host_id, &agent_id, &queue_id);
    }

    pub fn reorder_queued_prompts(
        &self,
        host_id: String,
        agent_id: String,
        queue_ids: Vec<String>,
    ) {
        self.inner
            .reorder_queued_prompts(&host_id, &agent_id, queue_ids);
    }

    pub fn upgrade_host_daemon_over_wire(
        &self,
        host_id: String,
        target_version: Option<String>,
    ) -> Result<String, FfiOrbitError> {
        self.inner
            .upgrade_host_daemon_over_wire_blocking(&host_id, target_version)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn interrupt_agent(&self, host_id: String, agent_id: String) {
        self.inner.interrupt_agent(&host_id, &agent_id);
    }

    pub fn interrupt_agent_and_continue(&self, host_id: String, agent_id: String) {
        self.inner.interrupt_agent_and_continue(&host_id, &agent_id);
    }

    pub fn set_session_mode(&self, host_id: String, agent_id: String, mode: FfiSessionMode) {
        self.inner
            .set_session_mode(&host_id, &agent_id, mode.into());
    }

    pub fn set_agent_config(
        &self,
        host_id: String,
        agent_id: String,
        model: Option<String>,
        effort: Option<FfiReasoningEffort>,
    ) {
        self.inner
            .set_agent_config(&host_id, &agent_id, model, effort.map(Into::into));
    }

    pub fn set_agent_fast_mode(&self, host_id: String, agent_id: String, fast_mode: bool) {
        self.inner
            .set_agent_fast_mode(&host_id, &agent_id, fast_mode);
    }

    pub fn switch_agent_provider(
        &self,
        host_id: String,
        agent_id: String,
        provider: FfiProviderKind,
        model: Option<String>,
        effort: Option<FfiReasoningEffort>,
        acp_agent_id: Option<String>,
    ) {
        self.inner.switch_agent_provider_with_config(
            &host_id,
            &agent_id,
            provider.into(),
            model,
            effort.map(Into::into),
            acp_agent_id,
        );
    }

    pub fn archive_agent(&self, host_id: String, agent_id: String) {
        self.inner.archive_agent(&host_id, &agent_id);
    }

    pub fn resolve_permission(
        &self,
        host_id: String,
        agent_id: String,
        permission_request_id: String,
        decision: String,
    ) {
        self.inner
            .resolve_permission(&host_id, &agent_id, &permission_request_id, &decision);
    }

    pub fn answer_question(
        &self,
        host_id: String,
        agent_id: String,
        question_request_id: String,
        answers: Vec<Vec<String>>,
    ) {
        self.inner
            .answer_question(&host_id, &agent_id, &question_request_id, answers);
    }

    pub fn fetch_git_status(&self, host_id: String, cwd: String) {
        self.inner.fetch_git_status(&host_id, &cwd);
    }

    pub fn fetch_git_diff(&self, host_id: String, cwd: String, path: Option<String>, staged: bool) {
        self.inner
            .fetch_git_diff(&host_id, &cwd, path.as_deref(), staged);
    }

    pub fn fetch_workspace_files(&self, host_id: String, cwd: String) {
        self.inner.fetch_workspace_files(&host_id, &cwd);
    }

    pub fn workspace_files(&self, host_id: String, cwd: String) -> Vec<String> {
        self.inner.workspace_files(&host_id, &cwd)
    }

    pub fn stage_attachment(
        &self,
        host_id: String,
        filename: String,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    ) -> Result<FfiStagedAttachment, FfiOrbitError> {
        self.inner
            .stage_attachment(&host_id, &filename, mime_type.as_deref(), bytes)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn start_terminal(&self, host_id: String, cwd: String, cols: u16, rows: u16) {
        self.inner.start_terminal(&host_id, &cwd, cols, rows);
    }

    pub fn attach_terminal(&self, host_id: String, terminal_id: String, slot: u8) {
        self.inner.attach_terminal(&host_id, &terminal_id, slot);
    }

    pub fn kill_terminal(&self, host_id: String, terminal_id: String) {
        self.inner.kill_terminal(&host_id, &terminal_id);
    }

    pub fn send_terminal_keys(&self, host_id: String, keys: String) {
        self.inner.send_terminal_input(&host_id, keys.into_bytes());
    }

    pub fn send_terminal_keys_slot(&self, host_id: String, slot: u8, keys: String) {
        self.inner
            .send_terminal_input_slot(&host_id, slot, keys.into_bytes());
    }

    pub fn send_terminal_bytes(&self, host_id: String, slot: u8, bytes: Vec<u8>) {
        self.inner.send_terminal_input_slot(&host_id, slot, bytes);
    }

    pub fn resize_terminal(&self, host_id: String, cols: u16, rows: u16) {
        self.inner.resize_terminal(&host_id, cols, rows);
    }

    pub fn resize_terminal_slot(&self, host_id: String, slot: u8, cols: u16, rows: u16) {
        self.inner.resize_terminal_slot(&host_id, slot, cols, rows);
    }

    pub fn rewind_agent(
        &self,
        host_id: String,
        agent_id: String,
        to_before_seq: u64,
        restore_files: bool,
    ) -> Result<FfiRewindAgentResult, FfiOrbitError> {
        self.inner
            .rewind_agent_blocking(&host_id, &agent_id, to_before_seq, restore_files)
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn read_workspace_file(
        &self,
        host_id: String,
        cwd: String,
        path: String,
    ) -> Result<FfiWorkspaceFile, FfiOrbitError> {
        self.inner
            .read_workspace_file_blocking(&host_id, &cwd, &path)
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn write_workspace_file(
        &self,
        req: FfiWriteWorkspaceFileRequest,
    ) -> Result<FfiWorkspaceFileWriteResult, FfiOrbitError> {
        self.inner
            .write_workspace_file_blocking(req.into())
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn create_workspace_entry(
        &self,
        req: FfiCreateWorkspaceEntryRequest,
    ) -> Result<FfiWorkspaceEntryMutationResult, FfiOrbitError> {
        self.inner
            .create_workspace_entry_blocking(req.into())
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn rename_workspace_entry(
        &self,
        req: FfiRenameWorkspaceEntryRequest,
    ) -> Result<FfiWorkspaceEntryMutationResult, FfiOrbitError> {
        self.inner
            .rename_workspace_entry_blocking(req.into())
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn delete_workspace_entry(
        &self,
        req: FfiDeleteWorkspaceEntryRequest,
    ) -> Result<FfiWorkspaceEntryMutationResult, FfiOrbitError> {
        self.inner
            .delete_workspace_entry_blocking(req.into())
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn build_side_question_context(
        &self,
        host_id: String,
        agent_id: String,
        max_chars: Option<u32>,
    ) -> String {
        self.inner
            .build_side_question_context(&host_id, &agent_id, max_chars.map(|v| v as usize))
    }

    pub fn ask_side_question(
        &self,
        req: FfiAskSideQuestionRequest,
        listener: Arc<dyn SideQuestionListener>,
    ) -> Result<String, FfiOrbitError> {
        let context_summary = if req.context_summary.trim().is_empty() {
            req.agent_id
                .as_deref()
                .map(|ag_id| {
                    self.inner
                        .build_side_question_context(&req.host_id, ag_id, Some(3000))
                })
                .unwrap_or_default()
        } else {
            req.context_summary
        };
        self.inner
            .ask_side_question_with_callback(
                AskSideQuestionRequest {
                    host_id: req.host_id,
                    agent_id: req.agent_id,
                    cwd: req.cwd,
                    provider: req.provider.into(),
                    model: req.model,
                    question: req.question,
                    context_summary,
                },
                move |ev| {
                    listener.on_side_question_event(ev.into());
                },
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn on_app_foreground(&self) {
        self.inner.on_app_foreground();
    }

    pub fn on_app_background(&self) {
        self.inner.on_app_background();
    }

    pub fn on_network_changed(&self) {
        self.inner.on_network_changed();
    }

    pub fn render_diagram(&self, req: FfiRenderDiagramRequest) -> FfiRenderDiagramView {
        self.inner.render_diagram(req.into()).into()
    }

    pub fn reorder_projects(
        &self,
        ordered_keys: Vec<String>,
    ) -> Result<Vec<String>, FfiOrbitError> {
        self.inner
            .reorder_projects(ordered_keys)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn project_order(&self) -> Vec<String> {
        self.inner.project_order()
    }

    pub fn move_agent_to_workspace(
        &self,
        host_id: String,
        agent_id: String,
        target_workspace_id: Option<String>,
    ) {
        self.inner
            .move_agent_to_workspace(&host_id, &agent_id, target_workspace_id);
    }

    pub fn merge_workspaces(
        &self,
        host_id: String,
        source_workspace_id: String,
        target_workspace_id: String,
    ) {
        self.inner
            .merge_workspaces(&host_id, &source_workspace_id, &target_workspace_id);
    }

    pub fn dismiss_cluster_suggestion(&self, host_id: String, agent_id: String) {
        self.inner.dismiss_cluster_suggestion(&host_id, &agent_id);
    }

    pub fn create_session_in_workspace(
        &self,
        req: FfiCreateSessionRequest,
        workspace_id: Option<String>,
        handle: Option<String>,
        auto_cluster: Option<bool>,
    ) -> String {
        let mut core_req: CreateSessionRequest = req.into();
        core_req.workspace_id = workspace_id;
        core_req.handle = handle;
        core_req.auto_cluster = auto_cluster;
        self.inner.create_session(core_req)
    }

    pub fn rank_mention_candidates_with_workspace(
        &self,
        host_id: String,
        cwd: String,
        query: String,
        limit: u32,
        workspace_id: Option<String>,
        active_agent_id: Option<String>,
    ) -> Vec<FfiMentionCandidate> {
        let core_req = MentionRankRequest {
            host_id,
            cwd,
            query,
            limit: limit as usize,
            workspace_id,
            active_agent_id,
        };
        self.inner
            .rank_mention_candidates(&core_req)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn unarchive_agent(&self, host_id: String, agent_id: String) {
        self.inner.unarchive_agent(&host_id, &agent_id);
    }

    pub fn delete_agent(&self, host_id: String, agent_id: String) {
        self.inner.delete_agent(&host_id, &agent_id);
    }

    pub fn detach_agent(&self, host_id: String, agent_id: String) {
        self.inner.detach_agent(&host_id, &agent_id);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_agent_identity(
        &self,
        host_id: String,
        agent_id: String,
        handle: Option<String>,
        title: Option<String>,
        task_summary: Option<String>,
        lock_handle: bool,
        lock_title: bool,
    ) {
        self.inner.update_agent_identity(
            &host_id,
            &agent_id,
            handle,
            title,
            task_summary,
            lock_handle,
            lock_title,
        );
    }

    pub fn summarize_agent(&self, host_id: String, agent_id: String) {
        self.inner.summarize_agent(&host_id, &agent_id);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_mission_workspace(
        &self,
        host_id: String,
        source_cwd: String,
        title: String,
        mission_goal: Option<String>,
        use_worktree: bool,
        worktree_branch: Option<String>,
        base_ref: Option<String>,
    ) {
        let isolation = if use_worktree {
            WorkspaceIsolation::Worktree
        } else {
            WorkspaceIsolation::Local
        };
        self.inner.create_mission_workspace(
            &host_id,
            &source_cwd,
            title,
            mission_goal,
            isolation,
            worktree_branch,
            base_ref,
        );
    }

    pub fn rename_workspace(&self, host_id: String, workspace_id: String, title: String) {
        self.inner.rename_workspace(&host_id, &workspace_id, &title);
    }

    pub fn update_workspace_goal(&self, host_id: String, workspace_id: String, goal: String) {
        self.inner
            .update_workspace_goal(&host_id, &workspace_id, goal);
    }

    pub fn archive_workspace(
        &self,
        host_id: String,
        workspace_id: String,
        keep_agents_unclassified: bool,
    ) {
        self.inner
            .archive_workspace_v2(&host_id, &workspace_id, keep_agents_unclassified);
    }

    pub fn move_agent_to_workspace_v2(
        &self,
        host_id: String,
        agent_id: String,
        target_workspace_id: Option<String>,
        new_workspace_title: Option<String>,
        make_unclassified: bool,
    ) {
        self.inner.move_agent_to_workspace_v2(
            &host_id,
            &agent_id,
            target_workspace_id,
            new_workspace_title,
            make_unclassified,
        );
    }

    pub fn organize_project_workspaces(
        &self,
        host_id: String,
        project_root: String,
        agent_ids: Vec<String>,
        auto_apply: bool,
    ) {
        self.inner
            .organize_project_workspaces(&host_id, &project_root, agent_ids, auto_apply);
    }

    pub fn spawn_workspace_peer(&self, req: FfiSpawnWorkspacePeerRequest) -> String {
        self.inner.spawn_workspace_peer(req.into())
    }

    pub fn fetch_workspace_board(&self, host_id: String, workspace_id: String) {
        self.inner.fetch_workspace_board(&host_id, &workspace_id);
    }

    pub fn workspace_board(
        &self,
        host_id: String,
        workspace_id: String,
    ) -> Option<FfiWorkspaceBoardSnapshot> {
        self.inner
            .workspace_board(&host_id, &workspace_id)
            .map(Into::into)
    }

    pub fn update_workspace_board(
        &self,
        host_id: String,
        workspace_id: String,
        content: String,
        section: Option<String>,
        updated_by: Option<String>,
    ) {
        self.inner
            .update_workspace_board(&host_id, &workspace_id, content, section, updated_by);
    }

    pub fn fetch_workspace_events(
        &self,
        host_id: String,
        workspace_id: String,
        limit: Option<u32>,
    ) {
        self.inner
            .fetch_workspace_events(&host_id, &workspace_id, limit);
    }

    pub fn workspace_events(
        &self,
        host_id: String,
        workspace_id: String,
    ) -> Vec<FfiWorkspaceEvent> {
        self.inner
            .workspace_events(&host_id, &workspace_id)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn request_git_repos_inspection(
        &self,
        host_id: String,
        path: String,
        extra_dirs: Vec<String>,
    ) {
        self.inner
            .request_git_repos_inspection(&host_id, &path, extra_dirs);
    }

    pub fn git_repos_inspection(
        &self,
        host_id: String,
        path: String,
    ) -> Option<FfiGitReposInspection> {
        self.inner
            .git_repos_inspection(&host_id, &path)
            .map(Into::into)
    }

    // --- First-Class Artifacts & SEP-1865 MCP Apps ---

    pub fn list_artifacts(
        &self,
        host_id: String,
        workspace_id: Option<String>,
        agent_id: Option<String>,
    ) -> Result<Vec<FfiArtifactSummary>, FfiOrbitError> {
        self.inner
            .list_artifacts_blocking(&host_id, workspace_id, agent_id)
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn get_artifact(
        &self,
        host_id: String,
        artifact_id: String,
        version: Option<u32>,
    ) -> Result<FfiArtifactDetail, FfiOrbitError> {
        self.inner
            .get_artifact_blocking(&host_id, &artifact_id, version)
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn upsert_artifact(
        &self,
        req: FfiUpsertArtifactRequest,
    ) -> Result<FfiArtifactSummary, FfiOrbitError> {
        self.inner
            .upsert_artifact_blocking(req.into())
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn delete_artifact(
        &self,
        host_id: String,
        artifact_id: String,
    ) -> Result<String, FfiOrbitError> {
        self.inner
            .delete_artifact_blocking(&host_id, &artifact_id)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    // --- Universal ACP Agent Registry & Probing ---

    pub fn list_acp_agents(&self, host_id: String) -> Vec<FfiAcpAgentAvailability> {
        self.inner
            .list_acp_agents(&host_id)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn refresh_acp_agents(&self, host_id: String) {
        self.inner.refresh_acp_agents(&host_id);
    }

    pub fn upsert_custom_acp_agent(
        &self,
        host_id: String,
        spec: FfiAcpAgentSpec,
    ) -> Result<Vec<FfiAcpAgentAvailability>, FfiOrbitError> {
        self.inner
            .upsert_custom_acp_agent_blocking(&host_id, spec.into())
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn delete_custom_acp_agent(
        &self,
        host_id: String,
        agent_id: String,
    ) -> Result<Vec<FfiAcpAgentAvailability>, FfiOrbitError> {
        self.inner
            .delete_custom_acp_agent_blocking(&host_id, &agent_id)
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn test_acp_agent(
        &self,
        host_id: String,
        agent_id: String,
        cwd: Option<String>,
    ) -> Result<FfiAcpProbeResult, FfiOrbitError> {
        self.inner
            .test_acp_agent_blocking(&host_id, &agent_id, cwd)
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn refresh_acp_registry(
        &self,
        host_id: String,
        registry_url: Option<String>,
    ) -> Result<FfiAcpRegistryRefreshResult, FfiOrbitError> {
        self.inner
            .refresh_acp_registry_blocking(&host_id, registry_url)
            .map(|view| FfiAcpRegistryRefreshResult {
                agents: view.agents.into_iter().map(Into::into).collect(),
                warning: view.warning,
            })
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn list_provider_subagents(&self, host_id: String, parent_agent_id: String) {
        self.inner
            .list_provider_subagents(&host_id, &parent_agent_id);
    }

    pub fn open_provider_subagent_timeline(
        &self,
        host_id: String,
        parent_agent_id: String,
        subagent_id: String,
    ) {
        self.inner
            .open_provider_subagent_timeline(&host_id, &parent_agent_id, &subagent_id);
    }

    pub fn provider_subagent_timeline(
        &self,
        host_id: String,
        parent_agent_id: String,
        subagent_id: String,
    ) -> Vec<FfiProviderSubagentEvent> {
        self.inner
            .provider_subagent_timeline(&host_id, &parent_agent_id, &subagent_id)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    pub fn create_schedule(&self, req: FfiCreateScheduleRequest) {
        let target = ScheduleTarget::NewAgent(ScheduleNewAgentConfig {
            cwd: req.cwd,
            workspace_id: req.workspace_id,
            provider: req.provider.into(),
            acp_agent_id: req.acp_agent_id,
            model: req.model,
            effort: req.effort.map(Into::into),
            fast_mode: req.fast_mode,
            mode: req.mode.into(),
            profile_id: None,
            worktree: req.worktree,
            base_ref: None,
            archive_on_finish: req.archive_on_finish,
            background: false,
        });
        self.inner.create_schedule(
            &req.host_id,
            req.name,
            req.cron_expr,
            req.timezone,
            req.prompt,
            target,
            req.max_runs,
        );
    }

    pub fn pause_schedule(&self, host_id: String, schedule_id: String) {
        self.inner.pause_schedule(&host_id, &schedule_id);
    }

    pub fn resume_schedule(&self, host_id: String, schedule_id: String) {
        self.inner.resume_schedule(&host_id, &schedule_id);
    }

    pub fn run_schedule_once(&self, host_id: String, schedule_id: String) {
        self.inner.run_schedule_once(&host_id, &schedule_id);
    }

    pub fn delete_schedule(&self, host_id: String, schedule_id: String) {
        self.inner.delete_schedule(&host_id, &schedule_id);
    }

    pub fn fetch_schedule_runs(&self, host_id: String, schedule_id: String, limit: Option<u32>) {
        self.inner
            .fetch_schedule_runs(&host_id, &schedule_id, limit);
    }

    pub fn schedule_runs(&self, host_id: String, schedule_id: String) -> Vec<FfiScheduleRunRecord> {
        self.inner
            .schedule_runs(&host_id, &schedule_id)
            .into_iter()
            .map(Into::into)
            .collect()
    }

    // --- Zero-Server E2EE Cloud Sync (GitHub Device Flow + Secret Gist) ---

    pub fn cloud_sync_status(&self) -> FfiCloudSyncStatus {
        self.inner.cloud_sync_status().into()
    }

    pub fn cloud_sync_generate_passphrase(&self) -> String {
        self.inner.cloud_sync_generate_passphrase()
    }

    pub fn cloud_sync_set_github_client_id(
        &self,
        client_id: Option<String>,
    ) -> Result<FfiCloudSyncStatus, FfiOrbitError> {
        self.inner
            .cloud_sync_set_github_client_id(client_id)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_set_auto_sync(
        &self,
        enabled: bool,
    ) -> Result<FfiCloudSyncStatus, FfiOrbitError> {
        self.inner
            .cloud_sync_set_auto_sync(enabled)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_start_github_device_flow(
        &self,
        client_id_override: Option<String>,
    ) -> Result<FfiGitHubDeviceFlowStart, FfiOrbitError> {
        self.inner
            .cloud_sync_start_github_device_flow(client_id_override)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_poll_github_device_flow(
        &self,
        device_code: String,
        client_id_override: Option<String>,
    ) -> Result<FfiCloudSyncDevicePoll, FfiOrbitError> {
        self.inner
            .cloud_sync_poll_github_device_flow(&device_code, client_id_override)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_login_with_token(
        &self,
        token: String,
    ) -> Result<FfiCloudSyncStatus, FfiOrbitError> {
        self.inner
            .cloud_sync_login_with_token(&token)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_unlock_with_passphrase(
        &self,
        passphrase: String,
    ) -> Result<FfiCloudSyncResult, FfiOrbitError> {
        self.inner
            .cloud_sync_unlock_with_passphrase(&passphrase)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_now(&self) -> Result<FfiCloudSyncResult, FfiOrbitError> {
        self.inner
            .cloud_sync_now()
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_export_device_link_uri(&self) -> Result<String, FfiOrbitError> {
        self.inner
            .cloud_sync_export_device_link_uri()
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_import_device_link_uri(
        &self,
        uri: String,
    ) -> Result<FfiCloudSyncResult, FfiOrbitError> {
        self.inner
            .cloud_sync_import_device_link_uri(&uri)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_enable_manual_passphrase(
        &self,
        passphrase: String,
    ) -> Result<FfiCloudSyncStatus, FfiOrbitError> {
        self.inner
            .cloud_sync_enable_manual_passphrase(&passphrase)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_set_manual_passphrase_mode(
        &self,
        require_manual: bool,
    ) -> Result<FfiCloudSyncStatus, FfiOrbitError> {
        self.inner
            .cloud_sync_set_manual_passphrase_mode(require_manual)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_logout(
        &self,
        clear_local_hosts: bool,
    ) -> Result<FfiCloudSyncStatus, FfiOrbitError> {
        self.inner
            .cloud_sync_logout(clear_local_hosts)
            .map(Into::into)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    pub fn cloud_sync_get_preferences_json(&self) -> String {
        let prefs = self.inner.cloud_sync_get_preferences();
        serde_json::to_string(&prefs).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn cloud_sync_update_preferences_json(
        &self,
        patch_json: String,
    ) -> Result<(), FfiOrbitError> {
        let patch: HashMap<String, serde_json::Value> =
            serde_json::from_str(&patch_json).map_err(|e| FfiOrbitError::Core {
                detail: format!("invalid preferences patch JSON: {e}"),
            })?;
        self.inner
            .cloud_sync_update_preferences(patch)
            .map_err(|e| FfiOrbitError::Core {
                detail: e.to_string(),
            })
    }

    // --- Native Mobile Background Push (APNs / FCM / HarmonyOS Push Kit) ---

    pub fn register_push_device(
        &self,
        host_id: String,
        req: FfiRegisterPushDeviceRequest,
    ) -> Result<FfiPushDevicesSnapshot, FfiOrbitError> {
        self.inner
            .register_push_device_blocking(&host_id, req.into())
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn unregister_push_device(
        &self,
        host_id: String,
        device_id: String,
    ) -> Result<FfiPushDevicesSnapshot, FfiOrbitError> {
        self.inner
            .unregister_push_device_blocking(&host_id, &device_id)
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn list_push_devices(
        &self,
        host_id: String,
    ) -> Result<FfiPushDevicesSnapshot, FfiOrbitError> {
        self.inner
            .list_push_devices_blocking(&host_id)
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    // --- Conditional Timers ---

    pub fn list_conditional_timers(
        &self,
        host_id: String,
        agent_id: Option<String>,
        include_resolved: bool,
    ) -> Result<Vec<FfiConditionalTimerRecord>, FfiOrbitError> {
        block_on_oneshot_future(self.inner.list_conditional_timers_on_host(
            &host_id,
            agent_id,
            include_resolved,
        ))
        .map(|list| list.into_iter().map(Into::into).collect())
        .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn cancel_conditional_timer(
        &self,
        host_id: String,
        timer_id: String,
    ) -> Result<FfiConditionalTimerRecord, FfiOrbitError> {
        block_on_oneshot_future(
            self.inner
                .cancel_conditional_timer_on_host(&host_id, &timer_id),
        )
        .map(Into::into)
        .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn fire_conditional_timer_now(
        &self,
        host_id: String,
        timer_id: String,
    ) -> Result<FfiConditionalTimerRecord, FfiOrbitError> {
        block_on_oneshot_future(
            self.inner
                .fire_conditional_timer_now_on_host(&host_id, &timer_id),
        )
        .map(Into::into)
        .map_err(|detail| FfiOrbitError::Core { detail })
    }

    // --- Goals & Monitors ---

    pub fn list_goal_runs(
        &self,
        host_id: String,
        agent_id: Option<String>,
    ) -> Result<Vec<FfiGoalRunRecord>, FfiOrbitError> {
        block_on_oneshot_future(self.inner.list_goal_runs_on_host(&host_id, agent_id))
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn control_goal_run(
        &self,
        host_id: String,
        goal_id: String,
        action: FfiGoalControlAction,
    ) -> Result<FfiGoalRunRecord, FfiOrbitError> {
        block_on_oneshot_future(self.inner.control_goal_run_on_host(ControlGoalRunRequest {
            host_id,
            goal_id,
            action: action.into(),
        }))
        .map(Into::into)
        .map_err(|detail| FfiOrbitError::Core { detail })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_goal_run(
        &self,
        host_id: String,
        goal_id: String,
        objective: Option<String>,
        criteria: Option<Vec<String>>,
        verify_command: Option<String>,
        clear_verify_command: bool,
        max_iterations: Option<u32>,
        max_duration_secs: Option<u64>,
    ) -> Result<FfiGoalRunRecord, FfiOrbitError> {
        let verify_patch = if clear_verify_command {
            Some(None)
        } else {
            verify_command.map(Some)
        };
        block_on_oneshot_future(self.inner.update_goal_run_on_host(UpdateGoalRunRequest {
            host_id,
            goal_id,
            objective,
            criteria,
            verify_command: verify_patch,
            max_iterations,
            max_duration_secs,
        }))
        .map(Into::into)
        .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn list_monitor_watches(
        &self,
        host_id: String,
        agent_id: Option<String>,
    ) -> Result<Vec<FfiMonitorWatchRecord>, FfiOrbitError> {
        block_on_oneshot_future(self.inner.list_monitor_watches_on_host(&host_id, agent_id))
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn stop_monitor_watch(
        &self,
        host_id: String,
        monitor_id: String,
    ) -> Result<FfiMonitorWatchRecord, FfiOrbitError> {
        block_on_oneshot_future(
            self.inner
                .stop_monitor_watch_on_host(StopMonitorWatchRequest {
                    host_id,
                    monitor_id,
                }),
        )
        .map(Into::into)
        .map_err(|detail| FfiOrbitError::Core { detail })
    }

    // --- Workflows (Batch CSV, DAG, Best-of-N) ---

    pub fn list_workflow_runs(
        &self,
        host_id: String,
        include_completed: bool,
    ) -> Result<Vec<FfiWorkflowRunRecord>, FfiOrbitError> {
        block_on_oneshot_future(
            self.inner
                .list_workflow_runs_on_host(&host_id, include_completed),
        )
        .map(|list| list.into_iter().map(Into::into).collect())
        .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn cancel_workflow_run(
        &self,
        host_id: String,
        run_id: String,
    ) -> Result<FfiWorkflowRunRecord, FfiOrbitError> {
        block_on_oneshot_future(
            self.inner
                .cancel_workflow_run_on_host(CancelWorkflowRunRequest { host_id, run_id }),
        )
        .map(Into::into)
        .map_err(|detail| FfiOrbitError::Core { detail })
    }

    // --- Mobile Push Token Registration Bridge ---

    pub fn register_push_token(
        &self,
        platform: String,
        token: String,
    ) -> Result<FfiPushRegistrationStatus, FfiOrbitError> {
        self.inner
            .register_push_token(platform, token)
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn unregister_push_token(&self, platform: String) -> Result<(), FfiOrbitError> {
        self.inner
            .unregister_push_token(platform)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn push_registration_status(&self) -> FfiPushRegistrationStatus {
        self.inner.push_registration_status().into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiPushPlatform {
    Apns,
    Fcm,
    HarmonyPushKit,
}

impl From<PushPlatform> for FfiPushPlatform {
    fn from(v: PushPlatform) -> Self {
        match v {
            PushPlatform::Apns => Self::Apns,
            PushPlatform::Fcm => Self::Fcm,
            PushPlatform::HarmonyPushKit => Self::HarmonyPushKit,
        }
    }
}

impl From<FfiPushPlatform> for PushPlatform {
    fn from(v: FfiPushPlatform) -> Self {
        match v {
            FfiPushPlatform::Apns => Self::Apns,
            FfiPushPlatform::Fcm => Self::Fcm,
            FfiPushPlatform::HarmonyPushKit => Self::HarmonyPushKit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiRegisterPushDeviceRequest {
    pub device_id: String,
    pub platform: FfiPushPlatform,
    pub device_name: String,
    pub device_token: String,
    pub sandbox: bool,
    #[uniffi(default = None)]
    pub live_activity_token: Option<String>,
}

impl From<FfiRegisterPushDeviceRequest> for RegisterPushDeviceRequest {
    fn from(v: FfiRegisterPushDeviceRequest) -> Self {
        Self {
            device_id: v.device_id,
            platform: v.platform.into(),
            device_name: v.device_name,
            device_token: v.device_token,
            live_activity_token: v.live_activity_token,
            sandbox: v.sandbox,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiPushDeviceRecord {
    pub device_id: String,
    pub platform: FfiPushPlatform,
    pub device_name: String,
    pub masked_token: String,
    pub sandbox: bool,
    pub enabled: bool,
    pub last_delivered_at_ms: Option<u64>,
    pub last_error: Option<String>,
    pub registered_at_ms: u64,
    pub updated_at_ms: u64,
}

impl From<PushDeviceRecord> for FfiPushDeviceRecord {
    fn from(v: PushDeviceRecord) -> Self {
        Self {
            device_id: v.device_id,
            platform: v.platform.into(),
            device_name: v.device_name,
            masked_token: v.masked_token,
            sandbox: v.sandbox,
            enabled: v.enabled,
            last_delivered_at_ms: v.last_delivered_at_ms,
            last_error: v.last_error,
            registered_at_ms: v.registered_at_ms,
            updated_at_ms: v.updated_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiPushPreferences {
    pub enabled: bool,
    pub notify_permission_required: bool,
    pub notify_turn_completed: bool,
    pub notify_agent_error: bool,
    pub notify_goal_finished: bool,
    pub notify_timer_fired: bool,
    pub include_message_preview: bool,
    pub quiet_hours_enabled: bool,
    pub quiet_start_hour: u8,
    pub quiet_end_hour: u8,
    pub push_gateway_url: Option<String>,
    pub has_gateway_auth_token: bool,
}

impl From<PushPreferences> for FfiPushPreferences {
    fn from(v: PushPreferences) -> Self {
        Self {
            enabled: v.enabled,
            notify_permission_required: v.notify_permission_required,
            notify_turn_completed: v.notify_turn_completed,
            notify_agent_error: v.notify_agent_error,
            notify_goal_finished: v.notify_goal_finished,
            notify_timer_fired: v.notify_timer_fired,
            include_message_preview: v.include_message_preview,
            quiet_hours_enabled: v.quiet_hours_enabled,
            quiet_start_hour: v.quiet_start_hour,
            quiet_end_hour: v.quiet_end_hour,
            push_gateway_url: v.push_gateway_url,
            has_gateway_auth_token: v.has_gateway_auth_token,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiPushDevicesSnapshot {
    pub devices: Vec<FfiPushDeviceRecord>,
    pub preferences: FfiPushPreferences,
}

impl From<PushDevicesSnapshotView> for FfiPushDevicesSnapshot {
    fn from(v: PushDevicesSnapshotView) -> Self {
        Self {
            devices: v.devices.into_iter().map(Into::into).collect(),
            preferences: v.preferences.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiCloudSyncState {
    SignedOut,
    Locked,
    Ready,
    Syncing,
    Error,
}

impl From<CloudSyncState> for FfiCloudSyncState {
    fn from(v: CloudSyncState) -> Self {
        match v {
            CloudSyncState::SignedOut => Self::SignedOut,
            CloudSyncState::Locked => Self::Locked,
            CloudSyncState::Ready => Self::Ready,
            CloudSyncState::Syncing => Self::Syncing,
            CloudSyncState::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiCloudSyncDevicePollStatus {
    Pending,
    SlowDown,
    Authorized,
    Expired,
    Denied,
}

impl From<CloudSyncDevicePollStatus> for FfiCloudSyncDevicePollStatus {
    fn from(v: CloudSyncDevicePollStatus) -> Self {
        match v {
            CloudSyncDevicePollStatus::Pending => Self::Pending,
            CloudSyncDevicePollStatus::SlowDown => Self::SlowDown,
            CloudSyncDevicePollStatus::Authorized => Self::Authorized,
            CloudSyncDevicePollStatus::Expired => Self::Expired,
            CloudSyncDevicePollStatus::Denied => Self::Denied,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiCloudSyncStatus {
    pub state: FfiCloudSyncState,
    pub provider: String,
    pub github_oauth_configured: bool,
    pub github_client_id_override: Option<String>,
    pub account_login: Option<String>,
    pub account_name: Option<String>,
    pub account_avatar_url: Option<String>,
    pub gist_id: Option<String>,
    pub gist_html_url: Option<String>,
    pub remote_vault_exists: bool,
    pub auto_sync_enabled: bool,
    pub instant_unlock_enabled: bool,
    pub last_synced_at_ms: Option<i64>,
    pub last_vault_revision: u64,
    pub synced_host_count: u32,
    pub synced_project_count: u32,
    pub last_error: Option<String>,
}

impl From<CloudSyncStatusView> for FfiCloudSyncStatus {
    fn from(v: CloudSyncStatusView) -> Self {
        Self {
            state: v.state.into(),
            provider: v.provider,
            github_oauth_configured: v.github_oauth_configured,
            github_client_id_override: v.github_client_id_override,
            account_login: v.account_login,
            account_name: v.account_name,
            account_avatar_url: v.account_avatar_url,
            gist_id: v.gist_id,
            gist_html_url: v.gist_html_url,
            remote_vault_exists: v.remote_vault_exists,
            auto_sync_enabled: v.auto_sync_enabled,
            instant_unlock_enabled: v.instant_unlock_enabled,
            last_synced_at_ms: v.last_synced_at_ms,
            last_vault_revision: v.last_vault_revision,
            synced_host_count: v.synced_host_count,
            synced_project_count: v.synced_project_count,
            last_error: v.last_error,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiGitHubDeviceFlowStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in_secs: u64,
    pub interval_secs: u64,
}

impl From<GitHubDeviceFlowStartView> for FfiGitHubDeviceFlowStart {
    fn from(v: GitHubDeviceFlowStartView) -> Self {
        Self {
            device_code: v.device_code,
            user_code: v.user_code,
            verification_uri: v.verification_uri,
            expires_in_secs: v.expires_in_secs,
            interval_secs: v.interval_secs,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiCloudSyncDevicePoll {
    pub status: FfiCloudSyncDevicePollStatus,
    pub interval_secs: Option<u64>,
    pub error: Option<String>,
    pub sync_status: Option<FfiCloudSyncStatus>,
}

impl From<CloudSyncDevicePollView> for FfiCloudSyncDevicePoll {
    fn from(v: CloudSyncDevicePollView) -> Self {
        Self {
            status: v.status.into(),
            interval_secs: v.interval_secs,
            error: v.error,
            sync_status: v.sync_status.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiCloudSyncResult {
    pub hosts_added: u32,
    pub hosts_updated: u32,
    pub hosts_removed: u32,
    pub projects_added: u32,
    pub projects_updated: u32,
    pub projects_removed: u32,
    pub preferences_updated: u32,
    pub pushed_remote: bool,
    pub vault_revision: u64,
    pub synced_at_ms: i64,
    pub gist_html_url: Option<String>,
}

impl From<CloudSyncResultView> for FfiCloudSyncResult {
    fn from(v: CloudSyncResultView) -> Self {
        Self {
            hosts_added: v.hosts_added,
            hosts_updated: v.hosts_updated,
            hosts_removed: v.hosts_removed,
            projects_added: v.projects_added,
            projects_updated: v.projects_updated,
            projects_removed: v.projects_removed,
            preferences_updated: v.preferences_updated,
            pushed_remote: v.pushed_remote,
            vault_revision: v.vault_revision,
            synced_at_ms: v.synced_at_ms,
            gist_html_url: v.gist_html_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniffi_mobile_core_stage_project_and_lifecycle_roundtrip() {
        let core = OrbitMobileCore::open_in_memory().expect("open");
        let cat = fallback_provider_catalog(FfiProviderKind::ClaudeCode);
        assert!(!cat.models.is_empty());
        assert_eq!(
            format_catalog_model_label(cat, Some("claude-opus-5-5".into())),
            "Opus 5.5"
        );
        assert_eq!(
            core.fallback_provider_catalogs().len(),
            ProviderKind::all().len()
        );

        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = orbit_crypto::PairingOffer {
            host_id: "host_1".into(),
            hostname: "orbit-dev".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "secret".into(),
        };
        core.pair_host(offer.to_qr_uri().expect("uri"))
            .expect("pair host");

        let info = core.bridge_info();
        assert_eq!(info.protocol_version, 2);
        assert!(info.capabilities.contains(&"composer_v2".to_string()));

        let proj = core
            .upsert_project(
                None,
                "host_1".into(),
                "orbit".into(),
                vec!["/Users/jess/Code/orbit".into()],
            )
            .expect("upsert project");
        assert_eq!(proj.name, "orbit");
        assert_eq!(core.fleet_overview().projects.len(), 1);

        let opt_id = core.create_session(FfiCreateSessionRequest {
            host_id: "host_1".into(),
            cwd: "/Users/jess/Code/orbit".into(),
            extra_dirs: Vec::new(),
            provider: FfiProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode: FfiSessionMode::Default,
            initial_prompt: "Verify sidebar & stage delta".into(),
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
        });
        let delta0 = core.agent_stage_delta_request(FfiAgentStageDeltaRequest {
            host_id: "host_1".into(),
            agent_id: opt_id.clone(),
            since_revision: 0,
            since_aux_revision: None,
            turn_limit: None,
            since_window_start_turn_index: None,
        });
        assert_eq!(delta0.host_id, "host_1");
        assert_eq!(delta0.total_turn_count, 1);

        assert!(core.toggle_pin_agent("host_1".into(), "ag_seed".into()));
        core.mark_agent_seen("host_1".into(), "ag_seed".into(), 5);
        core.seed_thread_prefs(vec![FfiThreadPrefSeed {
            host_id: "host_1".into(),
            agent_id: "ag_seed_2".into(),
            seen_seq: 9,
            is_pinned: true,
            pinned_order: 2,
            updated_at_ms: 1000,
        }]);

        let sidebar = core.fleet_sidebar_request(FfiFleetSidebarRequest {
            sort: FfiSidebarProjectSort::Recent,
            filter: FfiSidebarThreadFilter::All,
            query: String::new(),
            active_host_id: Some("host_1".into()),
            active_agent_id: Some(opt_id),
            active_cwd: Some("/Users/jess/Code/orbit".into()),
        });
        assert_eq!(sidebar.projects.len(), 1);
        assert!(!sidebar.launchpad_projects.is_empty());

        let sel = core.resolve_model_selection(FfiModelSelectionRequest {
            host_id: Some("host_1".into()),
            provider: FfiProviderKind::ClaudeCode,
            desired_model: Some("sonnet".into()),
            desired_effort: None,
        });
        assert_eq!(sel.selected_model_id.as_deref(), Some("claude-opus-5-5"));

        let cmds = core.slash_commands(FfiClientPlatform::Ios, None);
        assert!(cmds.iter().any(|c| c.id == "btw"));
        assert!(cmds.iter().any(|c| c.id == "files"));

        let blocks = parse_markdown_preview("# Board\n\n- Task **one**".into(), false);
        assert_eq!(blocks.len(), 2);

        let reordered = core
            .reorder_projects(vec!["host_1::/Users/jess/Code/orbit".into()])
            .expect("reorder projects");
        assert_eq!(reordered, vec!["host_1::/Users/jess/Code/orbit"]);
        assert_eq!(core.project_order(), reordered);

        let mentions_ws = core.rank_mention_candidates_with_workspace(
            "host_1".into(),
            "/Users/jess/Code/orbit".into(),
            "diff".into(),
            10,
            None,
            None,
        );
        assert!(mentions_ws.iter().any(|m| m.token_label == "@working-diff"));

        let composed = core.compose_prompt(FfiComposePromptRequest {
            provider: FfiProviderKind::Codex,
            raw_text: "/plan ship contract v2".into(),
            explicit_directive: None,
            host_id: Some("host_1".into()),
            cwd: Some("/Users/jess/Code/orbit".into()),
            contexts: Vec::new(),
            quotes: Vec::new(),
            pastes: Vec::new(),
        });
        assert!(
            composed
                .wire_prompt
                .contains("<WORKFLOW_INSTRUCTION command=\"/plan\">")
        );

        let mentions = core.rank_mention_candidates(FfiMentionRankRequest {
            host_id: "host_1".into(),
            cwd: "/Users/jess/Code/orbit".into(),
            query: "diff".into(),
            limit: 10,
        });
        assert!(mentions.iter().any(|m| m.token_label == "@working-diff"));

        assert_eq!(
            detect_diagram_kind("mermaid".into(), "flowchart LR; A-->B".into()),
            Some(FfiDiagramKind::Mermaid)
        );
        let diag = core.render_diagram(FfiRenderDiagramRequest {
            kind: FfiDiagramKind::Mermaid,
            source: "flowchart LR\n  A-->B".into(),
            theme: FfiDiagramTheme::Dark,
        });
        assert!(diag.error.is_none());
        assert!(diag.svg.as_deref().is_some_and(|s| s.contains("<svg")));

        core.on_app_background();
        core.on_app_foreground();
        core.on_network_changed();
    }

    #[test]
    fn automation_records_and_push_bridge_roundtrip() {
        let timer = ConditionalTimerRecord {
            timer_id: "tmr_1".into(),
            agent_id: "ag_1".into(),
            workspace_id: "ws_1".into(),
            prompt: "Check build status".into(),
            duration_seconds: 120,
            condition: TimerCondition::AllSubagentsSettled,
            state: ConditionalTimerState::Armed,
            created_at_ms: 1000,
            fire_at_ms: 121_000,
            resolved_at_ms: None,
            resolved_reason: None,
        };
        let ffi_timer: FfiConditionalTimerRecord = timer.into();
        assert_eq!(ffi_timer.state, FfiConditionalTimerState::Armed);
        assert_eq!(ffi_timer.condition, FfiTimerCondition::AllSubagentsSettled);
        assert_eq!(ffi_timer.condition_label, "until subagents settle");

        let goal = GoalRunRecord {
            goal_id: "goal_1".into(),
            agent_id: "ag_1".into(),
            workspace_id: "ws_1".into(),
            objective: "Keep all tests green".into(),
            criteria: vec![GoalCriterion {
                id: "c1".into(),
                label: "cargo test passes".into(),
                passed: true,
            }],
            verify_command: Some("cargo test".into()),
            max_iterations: 10,
            current_iteration: 2,
            max_duration_secs: 3600,
            status: GoalStatus::Running,
            last_verify_exit_code: Some(0),
            last_verify_output: Some("ok".into()),
            consecutive_identical_failures: 0,
            pause_reason: None,
            paused_by_user: false,
            active_duration_secs: 15,
            started_at_ms: 1000,
            updated_at_ms: 2000,
            finished_at_ms: None,
        };
        let ffi_goal: FfiGoalRunRecord = goal.into();
        assert_eq!(ffi_goal.status, FfiGoalStatus::Running);
        assert_eq!(ffi_goal.criteria.len(), 1);
        assert_eq!(
            GoalControlAction::from(FfiGoalControlAction::Pause),
            GoalControlAction::Pause
        );

        let monitor = MonitorWatchRecord {
            monitor_id: "mon_1".into(),
            agent_id: "ag_1".into(),
            name: "Health Check".into(),
            kind: MonitorKind::HttpHealth {
                url: "https://example.com/health".into(),
                expect_status: 200,
                interval_secs: 30,
            },
            status: MonitorStatus::Active,
            match_count: 3,
            last_event_preview: Some("200 OK".into()),
            last_triggered_at_ms: Some(1500),
            expires_at_ms: None,
            created_at_ms: 1000,
        };
        let ffi_monitor: FfiMonitorWatchRecord = monitor.into();
        assert_eq!(ffi_monitor.status, FfiMonitorStatus::Active);

        let workflow = WorkflowRunRecord {
            run_id: "wf_1".into(),
            name: "Nightly DAG".into(),
            kind: WorkflowKind::Dag,
            workspace_cwd: "/tmp/repo".into(),
            owner_agent_id: Some("ag_1".into()),
            status: WorkflowRunStatus::Running,
            max_concurrency: 4,
            verify_command: None,
            winner_step_id: None,
            steps: vec![WorkflowStepRecord {
                run_id: "wf_1".into(),
                step_id: "step_1".into(),
                name: "Lint".into(),
                status: WorkflowStepStatus::Succeeded,
                depends_on: Vec::new(),
                agent_id: Some("ag_2".into()),
                worktree_cwd: None,
                provider: ProviderKind::ClaudeCode,
                model: None,
                output_preview: Some("clean".into()),
                diff_additions: 10,
                diff_deletions: 2,
                diff_files: 1,
                verify_passed: Some(true),
                verify_output: None,
                error: None,
                is_winner: false,
                started_at_ms: Some(1000),
                finished_at_ms: Some(1500),
            }],
            created_at_ms: 1000,
            updated_at_ms: 1500,
            finished_at_ms: None,
        };
        let ffi_wf: FfiWorkflowRunRecord = workflow.into();
        assert_eq!(ffi_wf.kind, FfiWorkflowKind::Dag);
        assert_eq!(ffi_wf.steps.len(), 1);

        let core = OrbitMobileCore::open_in_memory().expect("open in-memory");
        assert!(!core.push_registration_status().token_registered);
        let registered = core
            .register_push_token("android_fcm".into(), "fcm_token_1234567890abcdef".into())
            .expect("register push token");
        assert!(registered.token_registered);
        assert_eq!(registered.platform, "android_fcm");
        assert_eq!(registered.token_preview.as_deref(), Some("fcm_to…cdef"));

        // Short ASCII and short multi-byte UTF-8 tokens must never leak plaintext.
        let preview = |token: &str| {
            core.register_push_token("android_fcm".into(), token.into())
                .expect("register")
                .token_preview
                .unwrap_or_default()
        };
        assert_eq!(preview("short_tok"), "••••");
        assert_eq!(preview("🔑令牌测试"), "••••");
        assert_eq!(preview("🔑令牌测试数据1234567890"), "🔑令牌测试数…7890");

        core.unregister_push_token("android_fcm".into())
            .expect("unregister push token");
        assert!(!core.push_registration_status().token_registered);
    }

    #[test]
    fn ffi_git_review_projects_status_reverify_pending_true_and_false() {
        let make_view = |status_reverify_pending: bool| GitReviewView {
            revision: 7,
            host_id: "host_1".into(),
            cwd: "/tmp/repo".into(),
            status: Some(orbit_protocol::GitStatusSnapshot {
                cwd: "/tmp/repo".into(),
                is_git_repo: true,
                branch: Some("main".into()),
                upstream: Some("origin/main".into()),
                ahead: 1,
                behind: 0,
                files: vec![orbit_protocol::GitFileEntry {
                    path: "f".into(),
                    status: "M".into(),
                    staged: false,
                    untracked: false,
                }],
                nested_git_repos: Vec::new(),
                repo_prefix: Some(String::new()),
                repo_root: Some("/private/tmp/repo".into()),
            }),
            selected_path: Some("f".into()),
            staged: false,
            diff_revision: 2,
            diff_in_flight: true,
            status_revision: 3,
            status_identity_epoch: 0,
            status_reverify_pending,
            cwd_identity_epoch: 0,
            last_diff_error_revision: 0,
            last_diff_error_status_revision: 0,
            raw_diff: "diff --git a/f b/f\n+edited-after-stage\n".into(),
            diff_files: Vec::new(),
        };

        let pending = FfiGitReview::from(make_view(true));
        assert!(pending.status_reverify_pending);
        assert_eq!(pending.selected_path.as_deref(), Some("f"));
        assert!(pending.raw_diff.contains("+edited-after-stage"));
        assert_eq!(pending.changed_files.len(), 1);
        assert!(pending.is_git_repo);

        let settled = FfiGitReview::from(make_view(false));
        assert!(!settled.status_reverify_pending);
        assert_eq!(settled.selected_path.as_deref(), Some("f"));
        assert!(settled.raw_diff.contains("+edited-after-stage"));

        // Default cold `OrbitMobileCore::git_review` path reports `status_reverify_pending = false`.
        let core = OrbitMobileCore::open_in_memory().expect("open in-memory");
        let cold = core.git_review("host_1".into(), "/tmp/repo".into(), Some("f".into()));
        assert!(!cold.status_reverify_pending);
    }
}
