//! Strongly-typed UniFFI v0.32 native bridge for:
//! - iOS 16.4+ (`Swift` + `SwiftUI`)
//! - Android `minSdk 29` / `targetSdk 36` (`Kotlin 2` + `Jetpack Compose`)

use std::collections::HashMap;
use std::sync::Arc;

use orbit_client_core::app_core::{
    AgentCardView, AgentTimelineDeltaView, AgentTimelineView, ClientProjectView,
    CoreEventSubscriber, CoreNotification, CreateAgentRequest, DirectoryListingView,
    FleetOverviewView, GitReviewView, HostCardView, HostDetailView, OrbitAppCore,
    StagedAttachmentView, TimelineItemView,
};
use orbit_client_core::catalog::{
    default_effort_description as core_default_effort_description,
    fallback_provider_catalog as core_fallback_provider_catalog,
    format_catalog_model_label as core_format_catalog_model_label,
    format_effort_label as core_format_effort_label,
    resolve_supported_effort_for_model as core_resolve_supported_effort_for_model,
};
use orbit_client_core::render::{
    DiffFileView, DiffLineKind, DiffLineView, HighlightedLineView, HighlightedTokenView,
    InlineSpanView, RichBlockView, TableAlignmentView, TableCellView, TableRowView,
    TerminalRowView, TerminalScreenView, TerminalSpanView,
};
use orbit_client_core::stage::{
    AgentStageView, StageTurnView, ToolStepDescriptorView, ToolStepKind, TurnFileDiffSummaryView,
    TurnLiveSegmentView, TurnWorkCountsView, WorkSummaryResultView, describe_tool_detail,
    extract_thinking_headline,
};
use orbit_client_core::{HostConnectionState, HostRouteMode};
use orbit_protocol::{
    DirectoryEntry, DiscoveredProject, PromptBadge, ProviderKind, ProviderModelCatalog,
    ProviderModelSpec, ProviderQuotaSnapshot, QuestionItem, QuotaWindow, ReasoningEffort,
    SessionMode, TerminalSummary, ThinkingOptionSpec, ToolCallDetail, ToolCallStatus,
    WorkspaceSummary,
};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiOrbitError {
    #[error("{detail}")]
    Core { detail: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiProviderKind {
    Antigravity,
    Codex,
    ClaudeCode,
}

impl From<ProviderKind> for FfiProviderKind {
    fn from(v: ProviderKind) -> Self {
        match v {
            ProviderKind::Antigravity => Self::Antigravity,
            ProviderKind::Codex => Self::Codex,
            ProviderKind::ClaudeCode => Self::ClaudeCode,
        }
    }
}

impl From<FfiProviderKind> for ProviderKind {
    fn from(v: FfiProviderKind) -> Self {
        match v {
            FfiProviderKind::Antigravity => Self::Antigravity,
            FfiProviderKind::Codex => Self::Codex,
            FfiProviderKind::ClaudeCode => Self::ClaudeCode,
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
}

impl From<SessionMode> for FfiSessionMode {
    fn from(v: SessionMode) -> Self {
        match v {
            SessionMode::Default => Self::Default,
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
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiFleetOverview {
    pub revision: u64,
    pub total_pending_approvals: u32,
    pub hosts: Vec<FfiHostCard>,
    pub projects: Vec<FfiClientProject>,
}

impl From<FleetOverviewView> for FfiFleetOverview {
    fn from(v: FleetOverviewView) -> Self {
        Self {
            revision: v.revision,
            total_pending_approvals: v.total_pending_approvals,
            hosts: v.hosts.into_iter().map(Into::into).collect(),
            projects: v.projects.into_iter().map(Into::into).collect(),
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
}

impl From<WorkspaceSummary> for FfiWorkspaceSummary {
    fn from(v: WorkspaceSummary) -> Self {
        Self {
            workspace_id: v.workspace_id,
            title: v.title,
            cwd: v.cwd,
            git_branch: v.git_branch,
            is_worktree: v.is_worktree,
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
    pub mode: FfiSessionMode,
    pub status_label: String,
    pub is_running: bool,
    pub is_awaiting_permission: bool,
    pub parent_agent_id: Option<String>,
    pub latest_seq: u64,
    pub updated_at_rfc3339: String,
}

impl From<AgentCardView> for FfiAgentCard {
    fn from(v: AgentCardView) -> Self {
        Self {
            agent_id: v.agent_id,
            workspace_id: v.workspace_id,
            cwd: v.cwd,
            title: v.title,
            provider: v.provider.into(),
            provider_label: v.provider_label,
            model: v.model,
            effort: v.effort.map(Into::into),
            mode: v.mode.into(),
            status_label: v.status_label,
            is_running: v.is_running,
            is_awaiting_permission: v.is_awaiting_permission,
            parent_agent_id: v.parent_agent_id,
            latest_seq: v.latest_seq,
            updated_at_rfc3339: v.updated_at_rfc3339,
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

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiHostDetail {
    pub revision: u64,
    pub host: FfiHostCard,
    pub discovered_projects: Vec<FfiDiscoveredProject>,
    pub directory_listing: Option<FfiDirectoryListing>,
    pub provider_catalogs: Vec<FfiProviderModelCatalog>,
    pub workspaces: Vec<FfiWorkspaceSummary>,
    pub agents: Vec<FfiAgentCard>,
    pub last_created_agent_id: Option<String>,
    pub replaced_optimistic_agents: HashMap<String, String>,
    pub quotas: Vec<FfiProviderQuota>,
    pub terminals: Vec<FfiTerminalSummary>,
    pub active_terminal_id: Option<String>,
    pub active_terminal_slot: u8,
}

impl From<HostDetailView> for FfiHostDetail {
    fn from(v: HostDetailView) -> Self {
        Self {
            revision: v.revision,
            host: v.host.into(),
            discovered_projects: v.discovered_projects.into_iter().map(Into::into).collect(),
            directory_listing: v.directory_listing.map(Into::into),
            provider_catalogs: v.provider_catalogs.into_iter().map(Into::into).collect(),
            workspaces: v.workspaces.into_iter().map(Into::into).collect(),
            agents: v.agents.into_iter().map(Into::into).collect(),
            last_created_agent_id: v.last_created_agent_id,
            replaced_optimistic_agents: v.replaced_optimistic_agents,
            quotas: v.quotas.into_iter().map(Into::into).collect(),
            terminals: v.terminals.into_iter().map(Into::into).collect(),
            active_terminal_id: v.active_terminal_id,
            active_terminal_slot: v.active_terminal_slot,
        }
    }
}

// --- Rich Markdown, Code Highlighting & Diff Records ---

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiPromptBadge {
    pub kind: String,
    pub label: String,
}

impl From<PromptBadge> for FfiPromptBadge {
    fn from(v: PromptBadge) -> Self {
        Self {
            kind: v.kind,
            label: v.label,
        }
    }
}

impl From<FfiPromptBadge> for PromptBadge {
    fn from(v: FfiPromptBadge) -> Self {
        Self {
            kind: v.kind,
            label: v.label,
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
                spans,
            } => Self::ListItem {
                ordered,
                index,
                checked,
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
    pub label_includes_failure: bool,
}

impl From<WorkSummaryResultView> for FfiWorkSummaryResult {
    fn from(v: WorkSummaryResultView) -> Self {
        Self {
            counts: v.counts.into(),
            label: v.label,
            live_label: v.live_label,
            label_includes_failure: v.label_includes_failure,
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
        has_following_content: bool,
        has_running_step: bool,
    },
    AssistantMessage {
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
}

impl From<TurnLiveSegmentView> for FfiTurnLiveSegment {
    fn from(v: TurnLiveSegmentView) -> Self {
        match v {
            TurnLiveSegmentView::StepGroup {
                key,
                first_seq,
                items,
                summary,
                has_following_content,
                has_running_step,
            } => Self::StepGroup {
                key,
                first_seq,
                items: items.into_iter().map(Into::into).collect(),
                summary: summary.into(),
                has_following_content,
                has_running_step,
            },
            TurnLiveSegmentView::AssistantMessage { key, item } => Self::AssistantMessage {
                key,
                item: item.into(),
            },
            TurnLiveSegmentView::Notice { key, item } => Self::Notice {
                key,
                item: item.into(),
            },
            TurnLiveSegmentView::Compaction { key, label } => Self::Compaction { key, label },
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiStageTurn {
    pub key: String,
    pub turn_index: u32,
    pub anchor_seq: u64,
    pub user_prompt: Option<FfiTimelineItem>,
    pub is_compaction_command: bool,
    pub folded_steps: Vec<FfiTimelineItem>,
    pub work_counts: FfiTurnWorkCounts,
    pub work_summary_label: String,
    pub work_summary_live_label: String,
    pub work_summary_includes_failure: bool,
    pub thought_duration_ms: Option<i64>,
    pub turn_duration_ms: Option<i64>,
    pub terminal_reply: Option<FfiTimelineItem>,
    pub notices: Vec<FfiTimelineItem>,
    pub live_segments: Vec<FfiTurnLiveSegment>,
    pub turn_diffs: Vec<FfiTurnFileDiffSummary>,
    pub has_error: bool,
    pub has_unrecovered_error: bool,
    pub has_active_item: bool,
    pub started_at_ms: Option<i64>,
    pub completed_at_ms: Option<i64>,
}

impl From<StageTurnView> for FfiStageTurn {
    fn from(v: StageTurnView) -> Self {
        Self {
            key: v.key,
            turn_index: v.turn_index,
            anchor_seq: v.anchor_seq,
            user_prompt: v.user_prompt.map(Into::into),
            is_compaction_command: v.is_compaction_command,
            folded_steps: v.folded_steps.into_iter().map(Into::into).collect(),
            work_counts: v.work_counts.into(),
            work_summary_label: v.work_summary_label,
            work_summary_live_label: v.work_summary_live_label,
            work_summary_includes_failure: v.work_summary_includes_failure,
            thought_duration_ms: v.thought_duration_ms,
            turn_duration_ms: v.turn_duration_ms,
            terminal_reply: v.terminal_reply.map(Into::into),
            notices: v.notices.into_iter().map(Into::into).collect(),
            live_segments: v.live_segments.into_iter().map(Into::into).collect(),
            turn_diffs: v.turn_diffs.into_iter().map(Into::into).collect(),
            has_error: v.has_error,
            has_unrecovered_error: v.has_unrecovered_error,
            has_active_item: v.has_active_item,
            started_at_ms: v.started_at_ms,
            completed_at_ms: v.completed_at_ms,
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
            latest_seq: v.latest_seq,
        }
    }
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

#[derive(uniffi::Object)]
pub struct OrbitMobileCore {
    inner: Arc<OrbitAppCore>,
}

#[uniffi::export]
impl OrbitMobileCore {
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
            model,
            effort: effort.map(Into::into),
            mode: mode.into(),
            initial_prompt: &initial_prompt,
            initial_display_prompt,
            initial_badges: initial_badges.into_iter().map(Into::into).collect(),
            worktree_branch: worktree_branch.as_deref(),
        })
    }

    pub fn open_agent_timeline(&self, host_id: String, agent_id: String) {
        self.inner.open_agent_timeline(&host_id, &agent_id);
    }

    pub fn send_prompt(&self, host_id: String, agent_id: String, prompt: String) {
        self.inner.send_prompt(&host_id, &agent_id, &prompt);
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

    pub fn interrupt_agent(&self, host_id: String, agent_id: String) {
        self.inner.interrupt_agent(&host_id, &agent_id);
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

    pub fn switch_agent_provider(
        &self,
        host_id: String,
        agent_id: String,
        provider: FfiProviderKind,
        model: Option<String>,
        effort: Option<FfiReasoningEffort>,
    ) {
        self.inner.switch_agent_provider_with_config(
            &host_id,
            &agent_id,
            provider.into(),
            model,
            effort.map(Into::into),
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

    pub fn send_terminal_bytes(&self, host_id: String, slot: u8, bytes: Vec<u8>) {
        self.inner.send_terminal_input_slot(&host_id, slot, bytes);
    }

    pub fn resize_terminal(&self, host_id: String, cols: u16, rows: u16) {
        self.inner.resize_terminal(&host_id, cols, rows);
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

        core.on_app_background();
        core.on_app_foreground();
        core.on_network_changed();
    }
}
