//! Strongly-typed UniFFI v0.32 native bridge for:
//! - iOS 16.4+ (`Swift` + `SwiftUI`)
//! - Android `minSdk 29` / `targetSdk 36` (`Kotlin 2` + `Jetpack Compose`)

use std::sync::Arc;

use orbit_client_core::HostConnectionState;
use orbit_client_core::app_core::{
    AgentCardView, AgentTimelineView, CoreEventSubscriber, CoreNotification, FleetOverviewView,
    GitReviewView, HostCardView, HostDetailView, OrbitAppCore, TimelineItemView,
};
use orbit_client_core::render::{
    DiffFileView, DiffLineKind, DiffLineView, HighlightedLineView, HighlightedTokenView,
    InlineSpanView, RichBlockView, TerminalRowView, TerminalScreenView, TerminalSpanView,
};
use orbit_protocol::{
    ProviderKind, ProviderQuotaSnapshot, QuestionItem, QuotaWindow, SessionMode, ToolCallDetail,
    ToolCallStatus, WorkspaceSummary,
};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiOrbitError {
    #[error("{message}")]
    Core { message: String },
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
pub struct FfiHostCard {
    pub host_id: String,
    pub display_name: String,
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
}

impl From<FleetOverviewView> for FfiFleetOverview {
    fn from(v: FleetOverviewView) -> Self {
        Self {
            revision: v.revision,
            total_pending_approvals: v.total_pending_approvals,
            hosts: v.hosts.into_iter().map(Into::into).collect(),
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
    pub mode: FfiSessionMode,
    pub status_label: String,
    pub is_running: bool,
    pub is_awaiting_permission: bool,
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
            mode: v.mode.into(),
            status_label: v.status_label,
            is_running: v.is_running,
            is_awaiting_permission: v.is_awaiting_permission,
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
    pub workspaces: Vec<FfiWorkspaceSummary>,
    pub agents: Vec<FfiAgentCard>,
    pub quotas: Vec<FfiProviderQuota>,
    pub active_terminal_id: Option<String>,
}

impl From<HostDetailView> for FfiHostDetail {
    fn from(v: HostDetailView) -> Self {
        Self {
            revision: v.revision,
            host: v.host.into(),
            workspaces: v.workspaces.into_iter().map(Into::into).collect(),
            agents: v.agents.into_iter().map(Into::into).collect(),
            quotas: v.quotas.into_iter().map(Into::into).collect(),
            active_terminal_id: v.active_terminal_id,
        }
    }
}

// --- Rich Markdown, Code Highlighting & Diff Records ---

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
        blocks: Vec<FfiRichBlock>,
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
        blocks: Vec<FfiRichBlock>,
    },
    ToolCall {
        seq: u64,
        call_id: String,
        status: FfiToolStatus,
        summary_title: String,
        preview_body: Option<String>,
        diff_files: Vec<FfiDiffFile>,
    },
    PermissionRequest {
        seq: u64,
        request_id: String,
        title: String,
        command_or_path: String,
        allowed_decisions: Vec<String>,
        resolved_decision: Option<String>,
    },
    QuestionRequest {
        seq: u64,
        request_id: String,
        questions: Vec<FfiQuestionItem>,
        resolved_answers: Option<Vec<Vec<String>>>,
    },
    Notice {
        seq: u64,
        level: String,
        message: String,
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
                blocks,
                ..
            } => Self::UserPrompt {
                seq,
                message_id,
                text,
                blocks: blocks.into_iter().map(Into::into).collect(),
            },
            TimelineItemView::AssistantThinking { seq, item_id, text } => {
                Self::AssistantThinking { seq, item_id, text }
            }
            TimelineItemView::AssistantMessage {
                seq,
                item_id,
                text,
                streaming,
                blocks,
            } => Self::AssistantMessage {
                seq,
                item_id,
                text,
                streaming,
                blocks: blocks.into_iter().map(Into::into).collect(),
            },
            TimelineItemView::ToolCall {
                seq,
                call_id,
                status,
                summary_title,
                detail,
                diff_files,
            } => Self::ToolCall {
                seq,
                call_id,
                status: status.into(),
                summary_title,
                preview_body: tool_preview_text(&detail),
                diff_files: diff_files.into_iter().map(Into::into).collect(),
            },
            TimelineItemView::PermissionRequest {
                seq,
                request_id,
                title,
                detail,
                allowed_decisions,
                resolved_decision,
            } => Self::PermissionRequest {
                seq,
                request_id,
                title,
                command_or_path: tool_primary_target(&detail),
                allowed_decisions,
                resolved_decision,
            },
            TimelineItemView::QuestionRequest {
                seq,
                request_id,
                questions,
                resolved_answers,
            } => Self::QuestionRequest {
                seq,
                request_id,
                questions: questions.into_iter().map(Into::into).collect(),
                resolved_answers,
            },
            TimelineItemView::Notice {
                seq,
                level,
                message,
            } => Self::Notice {
                seq,
                level,
                message,
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
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changed_files: Vec<FfiGitFileEntry>,
    pub diff_files: Vec<FfiDiffFile>,
}

impl From<GitReviewView> for FfiGitReview {
    fn from(v: GitReviewView) -> Self {
        let (branch, ahead, behind, changed_files) = match v.status {
            Some(s) => (
                s.branch,
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
            None => (None, 0, 0, Vec::new()),
        };
        Self {
            revision: v.revision,
            host_id: v.host_id,
            cwd: v.cwd,
            branch,
            ahead,
            behind,
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
}

impl From<TerminalSpanView> for FfiTerminalSpan {
    fn from(s: TerminalSpanView) -> Self {
        Self {
            text: s.text,
            fg_hex: s.fg_hex,
            bg_hex: s.bg_hex,
            bold: s.bold,
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

#[derive(uniffi::Object)]
pub struct OrbitMobileCore {
    inner: Arc<OrbitAppCore>,
}

#[uniffi::export]
impl OrbitMobileCore {
    #[uniffi::constructor]
    pub fn open(db_path: String) -> Result<Arc<Self>, FfiOrbitError> {
        let inner = OrbitAppCore::open(&db_path, false).map_err(|e| FfiOrbitError::Core {
            message: e.to_string(),
        })?;
        Ok(Arc::new(Self { inner }))
    }

    #[uniffi::constructor]
    pub fn open_in_memory() -> Result<Arc<Self>, FfiOrbitError> {
        let inner = OrbitAppCore::open_in_memory().map_err(|e| FfiOrbitError::Core {
            message: e.to_string(),
        })?;
        Ok(Arc::new(Self { inner }))
    }

    pub fn subscribe(&self, listener: Arc<dyn MobileCoreListener>) {
        self.inner
            .subscribe(Arc::new(BridgeSubscriber { foreign: listener }));
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
                message: e.to_string(),
            })
    }

    pub fn remove_host(&self, host_id: String) -> Result<(), FfiOrbitError> {
        self.inner
            .remove_host(&host_id)
            .map_err(|e| FfiOrbitError::Core {
                message: e.to_string(),
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
                message: e.to_string(),
            })
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

    pub fn open_agent_timeline(&self, host_id: String, agent_id: String) {
        self.inner.open_agent_timeline(&host_id, &agent_id);
    }

    pub fn send_prompt(&self, host_id: String, agent_id: String, prompt: String) {
        self.inner.send_prompt(&host_id, &agent_id, &prompt);
    }

    pub fn interrupt_agent(&self, host_id: String, agent_id: String) {
        self.inner.interrupt_agent(&host_id, &agent_id);
    }

    pub fn set_session_mode(&self, host_id: String, agent_id: String, mode: FfiSessionMode) {
        self.inner
            .set_session_mode(&host_id, &agent_id, mode.into());
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

    pub fn start_terminal(&self, host_id: String, cwd: String, cols: u16, rows: u16) {
        self.inner.start_terminal(&host_id, &cwd, cols, rows);
    }

    pub fn send_terminal_keys(&self, host_id: String, keys: String) {
        self.inner.send_terminal_input(&host_id, keys.into_bytes());
    }

    pub fn resize_terminal(&self, host_id: String, cols: u16, rows: u16) {
        self.inner.resize_terminal(&host_id, cols, rows);
    }
}
