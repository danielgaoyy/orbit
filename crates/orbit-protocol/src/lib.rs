//! Core wire types, state machines, timeline events, git/worktree models, orchestration models, and binary terminal framing.

pub mod orchestration;
pub use orchestration::*;

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 4;

fn wire_default_true() -> bool {
    true
}

/// First-class supported coding agent CLIs in Orbit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[serde(alias = "agy")]
    Antigravity,
    #[serde(alias = "openai")]
    Codex,
    #[serde(alias = "claude", alias = "claude-code")]
    ClaudeCode,
    #[serde(rename = "opencode", alias = "open_code", alias = "open-code")]
    OpenCode,
    #[serde(alias = "gemini", alias = "gemini-cli")]
    GeminiCli,
    #[serde(alias = "cursor", alias = "cursor-agent")]
    CursorAgent,
    #[serde(alias = "github_copilot", alias = "github-copilot")]
    Copilot,
    Goose,
    #[serde(alias = "pi-coding-agent", alias = "pi_coding_agent")]
    Pi,
    #[serde(alias = "kimi_cli", alias = "kimi-cli")]
    Kimi,
    Cline,
    #[serde(
        alias = "vibe",
        alias = "vibe_acp",
        alias = "vibe-acp",
        alias = "mistral-vibe"
    )]
    MistralVibe,
    #[serde(alias = "auggie", alias = "augment_code", alias = "augment-code")]
    Augment,
}

impl ProviderKind {
    const ALL_PROVIDERS: [Self; 13] = [
        Self::ClaudeCode,
        Self::Codex,
        Self::Antigravity,
        Self::OpenCode,
        Self::GeminiCli,
        Self::CursorAgent,
        Self::Copilot,
        Self::Goose,
        Self::Pi,
        Self::Kimi,
        Self::Cline,
        Self::MistralVibe,
        Self::Augment,
    ];

    /// All 13 first-class native coding agent providers in canonical display order.
    pub const fn all() -> &'static [Self] {
        &Self::ALL_PROVIDERS
    }

    /// Canonical wire/config slug matching serde serialization.
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Antigravity => "antigravity",
            Self::Codex => "codex",
            Self::ClaudeCode => "claude_code",
            Self::OpenCode => "opencode",
            Self::GeminiCli => "gemini_cli",
            Self::CursorAgent => "cursor_agent",
            Self::Copilot => "copilot",
            Self::Goose => "goose",
            Self::Pi => "pi",
            Self::Kimi => "kimi",
            Self::Cline => "cline",
            Self::MistralVibe => "mistral_vibe",
            Self::Augment => "augment",
        }
    }

    /// Parse a provider slug or common CLI alias (case-insensitive).
    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "antigravity" | "agy" => Some(Self::Antigravity),
            "codex" | "openai" => Some(Self::Codex),
            "claude_code" | "claude-code" | "claude" => Some(Self::ClaudeCode),
            "opencode" | "open_code" | "open-code" => Some(Self::OpenCode),
            "gemini_cli" | "gemini-cli" | "gemini" => Some(Self::GeminiCli),
            "cursor_agent" | "cursor-agent" | "cursor" => Some(Self::CursorAgent),
            "copilot" | "github_copilot" | "github-copilot" | "gh-copilot" => Some(Self::Copilot),
            "goose" => Some(Self::Goose),
            "pi" | "pi-coding-agent" | "pi_coding_agent" => Some(Self::Pi),
            "kimi" | "kimi_cli" | "kimi-cli" => Some(Self::Kimi),
            "cline" => Some(Self::Cline),
            "mistral_vibe" | "mistral-vibe" | "vibe" | "vibe-acp" | "vibe_acp" => {
                Some(Self::MistralVibe)
            }
            "augment" | "auggie" | "augment_code" | "augment-code" => Some(Self::Augment),
            _ => None,
        }
    }

    pub const fn default_binary(self) -> &'static str {
        match self {
            Self::Antigravity => "agy",
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
            Self::OpenCode => "opencode",
            Self::GeminiCli => "gemini",
            Self::CursorAgent => "cursor-agent",
            Self::Copilot => "copilot",
            Self::Goose => "goose",
            Self::Pi => "pi",
            Self::Kimi => "kimi",
            Self::Cline => "cline",
            Self::MistralVibe => "vibe-acp",
            Self::Augment => "auggie",
        }
    }

    /// Alternate binary names checked during PATH discovery when `default_binary()` is absent.
    pub const fn fallback_binaries(self) -> &'static [&'static str] {
        match self {
            Self::MistralVibe => &["vibe"],
            _ => &[],
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Antigravity => "Antigravity",
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::OpenCode => "OpenCode",
            Self::GeminiCli => "Gemini CLI",
            Self::CursorAgent => "Cursor Agent",
            Self::Copilot => "GitHub Copilot",
            Self::Goose => "Goose",
            Self::Pi => "Pi",
            Self::Kimi => "Kimi CLI",
            Self::Cline => "Cline",
            Self::MistralVibe => "Mistral Vibe",
            Self::Augment => "Augment",
        }
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Antigravity => "AGY",
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude",
            Self::OpenCode => "OpenCode",
            Self::GeminiCli => "Gemini",
            Self::CursorAgent => "Cursor",
            Self::Copilot => "Copilot",
            Self::Goose => "Goose",
            Self::Pi => "Pi",
            Self::Kimi => "Kimi",
            Self::Cline => "Cline",
            Self::MistralVibe => "Vibe",
            Self::Augment => "Augment",
        }
    }

    pub const fn env_override_key(self) -> &'static str {
        match self {
            Self::Antigravity => "ORBIT_AGY_BIN",
            Self::Codex => "ORBIT_CODEX_BIN",
            Self::ClaudeCode => "ORBIT_CLAUDE_BIN",
            Self::OpenCode => "ORBIT_OPENCODE_BIN",
            Self::GeminiCli => "ORBIT_GEMINI_BIN",
            Self::CursorAgent => "ORBIT_CURSOR_AGENT_BIN",
            Self::Copilot => "ORBIT_COPILOT_BIN",
            Self::Goose => "ORBIT_GOOSE_BIN",
            Self::Pi => "ORBIT_PI_BIN",
            Self::Kimi => "ORBIT_KIMI_BIN",
            Self::Cline => "ORBIT_CLINE_BIN",
            Self::MistralVibe => "ORBIT_VIBE_BIN",
            Self::Augment => "ORBIT_AUGGIE_BIN",
        }
    }

    pub const fn protocol_transport(self) -> &'static str {
        match self {
            Self::Antigravity => "acp-stdio + transcript",
            Self::Codex => "app-server json-rpc",
            Self::ClaudeCode => "stream-json control",
            Self::Pi => "pi-rpc jsonl",
            Self::OpenCode
            | Self::GeminiCli
            | Self::CursorAgent
            | Self::Copilot
            | Self::Goose
            | Self::Kimi
            | Self::Cline
            | Self::MistralVibe
            | Self::Augment => "acp-stdio",
        }
    }

    /// Returns `true` for the 9 first-class ACP stdio providers powered by `acp_providers`.
    pub const fn is_acp_stdio_provider(self) -> bool {
        matches!(
            self,
            Self::OpenCode
                | Self::GeminiCli
                | Self::CursorAgent
                | Self::Copilot
                | Self::Goose
                | Self::Kimi
                | Self::Cline
                | Self::MistralVibe
                | Self::Augment
        )
    }
}

/// Normalized permission/execution mode across all three providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Default,
    Plan,
    AcceptEdits,
    FullAccess,
    Custom(String),
}

/// Normalized reasoning effort / thinking budget across Codex, Claude Code, and Antigravity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    High,
    #[serde(alias = "extra_high")]
    Xhigh,
    Max,
    Ultra,
}

impl ReasoningEffort {
    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" => Some(Self::Off),
            "low" | "minimal" => Some(Self::Low),
            "medium" | "med" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" | "extra_high" | "extra-high" => Some(Self::Xhigh),
            "max" => Some(Self::Max),
            "ultra" | "ultracode" => Some(Self::Ultra),
            _ => None,
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Xhigh => "Extra High",
            Self::Max => "Max",
            Self::Ultra => "Ultra",
        }
    }
}

/// Supported reasoning/thinking option for a specific provider model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingOptionSpec {
    pub effort: ReasoningEffort,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

/// Discovered or manifest model entry for a provider (Paseo-style catalog model).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModelSpec {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub default_effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub thinking_options: Vec<ThinkingOptionSpec>,
    #[serde(default)]
    pub supports_fast_mode: bool,
}

/// Dynamic model & thinking catalog for a single provider on a specific Orbit host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModelCatalog {
    pub provider: ProviderKind,
    pub models: Vec<ProviderModelSpec>,
    #[serde(default)]
    pub configured_default_model: Option<String>,
    #[serde(default)]
    pub configured_default_effort: Option<ReasoningEffort>,
}

/// Strict algebraic lifecycle state machine for a managed agent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AgentStatus {
    Initializing,
    Idle,
    Running {
        turn_id: String,
    },
    /// Post-turn autonomous wakeup (e.g. background subagent, timer, or async task completion).
    AutonomousWakeup {
        turn_id: String,
        source: String,
    },
    /// Blocked on interactive tool approval or clarifying question from the user.
    AwaitingPermission {
        turn_id: String,
        request_id: String,
    },
    Error {
        message: String,
        recoverable: bool,
    },
    Closed,
}

impl AgentStatus {
    /// Returns `true` when the foreground turn has completed and the session is only
    /// keeping track of asynchronous background tasks (ready for new user prompts immediately).
    pub fn is_background_waiting(&self) -> bool {
        matches!(
            self,
            AgentStatus::AutonomousWakeup { source, .. }
                if source.trim().starts_with("waiting:")
        )
    }

    /// Returns `true` when a foreground turn or active autonomous wakeup is actively executing.
    pub fn is_foreground_busy(&self) -> bool {
        match self {
            AgentStatus::Initializing | AgentStatus::Running { .. } => true,
            AgentStatus::AutonomousWakeup { source, .. } => !source.trim().starts_with("waiting:"),
            AgentStatus::Idle
            | AgentStatus::AwaitingPermission { .. }
            | AgentStatus::Error { .. }
            | AgentStatus::Closed => false,
        }
    }
}

/// Host identity and metadata for Multi-Host fleet management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub host_id: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub public_key_b64: String,
    pub protocol_version: u32,
    pub daemon_version: String,
    pub available_providers: Vec<ProviderKind>,
}

/// Workspace record (either a direct local checkout, isolated git worktree, or multi-agent Mission Workspace).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub workspace_id: String,
    pub title: String,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub is_worktree: bool,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub setup_status: WorkspaceSetupStatus,
    /// Optional concise one-line mission goal for a collaborative Mission Workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_goal: Option<String>,
    /// Logical project root path (allows peers in separate worktrees or nested repos to share one Mission Workspace).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    /// Monotonic revision counter for the workspace's Shared Blackboard (`BOARD.md`).
    #[serde(default)]
    pub board_revision: u64,
    /// Whether the user explicitly renamed this workspace (prevents auto-title overwrites).
    #[serde(default)]
    pub title_locked: bool,
    /// Whether this workspace has absorbed auto-clustered sessions.
    #[serde(default)]
    pub auto_clustered: bool,
    /// `true` for explicit/curated Mission Workspaces; `false` for unclassified inbox containers (`待归类`).
    #[serde(default)]
    pub is_curated: bool,
}

/// Auto-discovered local project / Git repository choice for zero-typing UI selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredProject {
    pub path: String,
    pub name: String,
    pub git_branch: Option<String>,
    pub is_git_repo: bool,
    pub last_modified_unix: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested_git_repos: Vec<NestedGitRepo>,
}

/// Directory entry for interactive remote/local folder browsing (`+ 添加工作目录`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_git_repo: bool,
}

/// Selectable LLM model option reported by an installed Provider CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub is_default: bool,
}

/// Git file entry status for mobile working-tree review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileEntry {
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub untracked: bool,
}

/// Git checkout snapshot for a workspace directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatusSnapshot {
    pub cwd: String,
    pub is_git_repo: bool,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitFileEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nested_git_repos: Vec<NestedGitRepo>,
}

/// Synchronized follow-up prompt queued on the host daemon for multi-device visibility & offline drain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedPromptView {
    pub queue_id: String,
    pub display_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub badges: Vec<PromptBadge>,
    pub created_at: DateTime<Utc>,
}

/// Summary projection of an agent session for fast directory listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub agent_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub title: String,
    pub provider: ProviderKind,
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub fast_mode: bool,
    pub mode: SessionMode,
    pub status: AgentStatus,
    pub parent_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_host_id: Option<String>,
    pub latest_seq: u64,
    /// Monotonic counter incremented on every server-side timeline rewind so offline/unsubscribed
    /// clients detect history resets even after new post-rewind events surpass prior `latest_seq`.
    #[serde(default)]
    pub timeline_epoch: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub auto_archive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub attention: AttentionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queued_prompts: Vec<QueuedPromptView>,
    /// Daemon-authoritative highest timeline `seq` read on ANY connected device (strictly monotonic).
    #[serde(default)]
    pub read_seq: u64,
    /// Daemon-synced pinned flag for cross-device sidebar parity.
    #[serde(default)]
    pub is_pinned: bool,
    /// Monotonic ordering weight for pinned conversations.
    #[serde(default)]
    pub pinned_order: i64,
    /// Short collaborative handle unique within the agent's Mission Workspace (e.g. `lead`, `reviewer`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    /// Optional auto-clustering recommendation or reversible auto-group banner metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_suggestion: Option<WorkspaceClusterSuggestion>,
    /// Concise 1-line AI-generated (or user-edited) summary of what this conversation is currently doing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_summary: Option<String>,
    /// Optional source agent ID when this conversation was branched/forked from another thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from_agent_id: Option<String>,
    /// Optional source timeline sequence number at which this conversation forked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_at_seq: Option<u64>,
}

/// Destination workspace strategy when forking/branching an existing conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ForkTarget {
    #[default]
    SameDirectory,
    NewWorktree {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_ref: Option<String>,
    },
}

/// Active detached PTY terminal summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalSummary {
    pub terminal_id: String,
    pub cwd: String,
    pub title: String,
    pub cols: u16,
    pub rows: u16,
    pub created_at: DateTime<Utc>,
}

/// Normalized tool call category for mobile-friendly rendering (cards, diffs, shell blocks).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolCallDetail {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Running,
    Completed,
    Failed,
    Denied,
}

/// Structured metadata identifying a genuine asynchronous background task
/// (e.g., Antigravity `run_command` promoted to a background task, or Claude Code
/// `Bash(run_in_background: true)` / `local_bash` task).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTaskInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
}

/// Interactive multiple-choice question prompt (`ask_question` / elicitation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionItem {
    pub id: String,
    pub header: Option<String>,
    pub question: String,
    pub options: Vec<String>,
    pub is_multi_select: bool,
}

/// Quota window snapshot (e.g. 5-hour rolling window, weekly quota, or credits).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub label: String,
    pub used_percent: f32,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderQuotaSnapshot {
    pub provider: ProviderKind,
    pub account_label: Option<String>,
    pub windows: Vec<QuotaWindow>,
    pub credits_remaining: Option<f64>,
    pub error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// Compact semantic chip attached to a user turn (e.g. `/plan`, `@file`, `Working Tree Diff`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptBadge {
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Monotonically sequenced timeline event stored in SQLite WAL and streamed to clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub seq: u64,
    pub epoch: u32,
    pub agent_id: String,
    pub turn_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub payload: TimelinePayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimelinePayload {
    UserPrompt {
        message_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        badges: Vec<PromptBadge>,
    },
    AssistantThinking {
        item_id: String,
        text: String,
    },
    AssistantMessage {
        item_id: String,
        text: String,
        streaming: bool,
    },
    ToolCall {
        call_id: String,
        status: ToolCallStatus,
        detail: ToolCallDetail,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        background_task: Option<BackgroundTaskInfo>,
    },
    PermissionRequest {
        request_id: String,
        title: String,
        detail: ToolCallDetail,
        allowed_decisions: Vec<String>,
    },
    PermissionResolved {
        request_id: String,
        decision: String,
    },
    QuestionRequest {
        request_id: String,
        questions: Vec<QuestionItem>,
    },
    QuestionResolved {
        request_id: String,
        answers: Vec<Vec<String>>,
    },
    Notice {
        level: String,
        message: String,
    },
    AgentMessage {
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
    },
    StatusChanged {
        status: AgentStatus,
    },
}

/// Client-to-Daemon and Daemon-to-Client RPC envelope over WebSocket / E2EE Relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum WireMessage {
    Hello {
        client_id: String,
        client_name: String,
        protocol_version: u32,
        supports_zstd: bool,
    },
    Welcome {
        host: HostInfo,
    },
    Ping {
        ts_ms: u64,
    },
    Pong {
        ts_ms: u64,
    },

    // Workspaces, Directory Browser & Git / Worktrees
    DiscoverProjects {
        request_id: Uuid,
    },
    DiscoveredProjectsSnapshot {
        request_id: Uuid,
        projects: Vec<DiscoveredProject>,
    },
    ListDirectory {
        request_id: Uuid,
        path: Option<String>,
    },
    DirectoryEntriesResponse {
        request_id: Uuid,
        current_path: String,
        parent_path: Option<String>,
        entries: Vec<DirectoryEntry>,
    },
    ListModels {
        request_id: Uuid,
    },
    ModelsSnapshot {
        request_id: Uuid,
        models: HashMap<ProviderKind, Vec<ModelOption>>,
    },
    ListWorkspaces {
        request_id: Uuid,
        #[serde(default)]
        include_archived: bool,
        #[serde(default)]
        include_background: bool,
    },
    WorkspacesSnapshot {
        request_id: Uuid,
        workspaces: Vec<WorkspaceSummary>,
    },
    CreateWorkspace {
        request_id: Uuid,
        source_cwd: String,
        title: Option<String>,
        /// If `Some(branch_name)`, creates an isolated git worktree under `$ORBIT_HOME/worktrees/`.
        worktree_branch: Option<String>,
        base_ref: Option<String>,
        #[serde(default)]
        isolation: WorkspaceIsolation,
        #[serde(default)]
        worktree_mode: WorktreeCreateMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pr_number: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_workspace_id: Option<String>,
        #[serde(default)]
        background: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idempotency_key: Option<String>,
        /// Explicit curated flag (`Some(true)` for user-created Mission Workspaces).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_curated: Option<bool>,
        /// Optional one-line mission goal for a curated Mission Workspace.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mission_goal: Option<String>,
        /// Optional logical project root (defaults to `source_cwd`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_root: Option<String>,
    },
    WorkspaceCreated {
        request_id: Uuid,
        workspace: WorkspaceSummary,
    },
    WorkspaceUpdated {
        workspace: WorkspaceSummary,
    },
    WorkspaceSetupProgress {
        progress: WorkspaceSetupProgress,
    },
    RenameWorkspace {
        request_id: Uuid,
        workspace_id: String,
        title: String,
    },
    ArchiveWorkspace {
        request_id: Uuid,
        workspace_id: String,
        /// When `true`, disbands the workspace and moves its active conversations back to `待归类` instead of archiving them.
        #[serde(default)]
        keep_agents_unclassified: bool,
    },
    UnarchiveWorkspace {
        request_id: Uuid,
        workspace_id: String,
    },
    ListWorkspaceScripts {
        request_id: Uuid,
        workspace_id: String,
    },
    WorkspaceScriptsSnapshot {
        request_id: Uuid,
        workspace_id: String,
        scripts: Vec<WorkspaceScriptInfo>,
    },
    StartWorkspaceScript {
        request_id: Uuid,
        workspace_id: String,
        script_name: String,
    },
    StopWorkspaceScript {
        request_id: Uuid,
        workspace_id: String,
        script_name: String,
    },
    WorkspaceScriptUpdated {
        script: WorkspaceScriptInfo,
    },
    GetGitStatus {
        request_id: Uuid,
        cwd: String,
    },
    GitStatusResponse {
        request_id: Uuid,
        status: GitStatusSnapshot,
    },
    /// Inspect whether `cwd` is a Git repository and discover nested Git repos inside it / `extra_dirs`.
    InspectGitRepos {
        request_id: Uuid,
        cwd: String,
        #[serde(default)]
        extra_dirs: Vec<String>,
    },
    GitReposResponse {
        request_id: Uuid,
        cwd: String,
        is_git_repo: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        git_branch: Option<String>,
        #[serde(default)]
        repos: Vec<NestedGitRepo>,
    },
    GetGitDiff {
        request_id: Uuid,
        cwd: String,
        path: Option<String>,
        staged: bool,
    },
    GitDiffResponse {
        request_id: Uuid,
        cwd: String,
        path: Option<String>,
        unified_diff: String,
    },
    ListWorkspaceFiles {
        request_id: Uuid,
        cwd: String,
    },
    WorkspaceFilesResponse {
        request_id: Uuid,
        cwd: String,
        files: Vec<String>,
    },
    ReadWorkspaceFile {
        request_id: Uuid,
        cwd: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_bytes: Option<u64>,
    },
    WorkspaceFileContentResponse {
        request_id: Uuid,
        cwd: String,
        path: String,
        resolved_path: String,
        /// `"text"` | `"image"` | `"binary"`
        content_kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        content: String,
        size_bytes: u64,
        truncated: bool,
    },

    // Agents & Interactive Orchestration
    ListAgents {
        request_id: Uuid,
        workspace_id: Option<String>,
        #[serde(default)]
        include_archived: bool,
        #[serde(default)]
        include_background: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_agent_id: Option<String>,
    },
    AgentsSnapshot {
        request_id: Uuid,
        agents: Vec<AgentSummary>,
    },
    CreateAgent {
        request_id: Uuid,
        cwd: String,
        #[serde(default)]
        extra_dirs: Vec<String>,
        title: Option<String>,
        provider: ProviderKind,
        model: Option<String>,
        #[serde(default)]
        effort: Option<ReasoningEffort>,
        mode: SessionMode,
        initial_prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial_display_prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        initial_badges: Vec<PromptBadge>,
        #[serde(default)]
        worktree: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_agent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_host_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile_id: Option<String>,
        #[serde(default)]
        background: bool,
        #[serde(default)]
        auto_archive: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notify_on_finish: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idempotency_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_ref: Option<String>,
        #[serde(default)]
        worktree_mode: WorktreeCreateMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        /// Optional short collaborative handle within the workspace (e.g. `lead`, `reviewer`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        handle: Option<String>,
        /// Override automatic clustering into an existing Mission Workspace (defaults to host config).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auto_cluster: Option<bool>,
        /// Enable Fast output mode (lower latency, higher quota usage) when supported by the model.
        /// `None` inherits the profile default; `Some(false)` explicitly disables fast mode.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fast_mode: Option<bool>,
    },
    AgentCreated {
        request_id: Uuid,
        agent: AgentSummary,
    },
    /// Fork/branch an existing conversation into a new session (same directory or fresh git worktree).
    ForkAgent {
        request_id: Uuid,
        source_agent_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        up_to_seq: Option<u64>,
        #[serde(default)]
        target: ForkTarget,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial_prompt: Option<String>,
    },
    AgentForked {
        request_id: Uuid,
        agent: AgentSummary,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace: Option<WorkspaceSummary>,
    },
    AgentUpdated {
        agent: AgentSummary,
    },
    AgentDeleted {
        agent_id: String,
    },
    SendPrompt {
        request_id: Uuid,
        agent_id: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        badges: Vec<PromptBadge>,
        #[serde(default)]
        active_turn_behavior: ActiveTurnBehavior,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caller_agent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notify_on_finish: Option<bool>,
    },
    EnqueuePrompt {
        request_id: Uuid,
        agent_id: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        badges: Vec<PromptBadge>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queue_id: Option<String>,
    },
    RemoveQueuedPrompt {
        request_id: Uuid,
        agent_id: String,
        queue_id: String,
    },
    FlushQueuedPromptNow {
        request_id: Uuid,
        agent_id: String,
        queue_id: String,
    },
    ReorderQueuedPrompts {
        request_id: Uuid,
        agent_id: String,
        queue_ids: Vec<String>,
    },
    UpgradeDaemon {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_version: Option<String>,
    },
    InterruptTurn {
        request_id: Uuid,
        agent_id: String,
    },
    SetSessionMode {
        request_id: Uuid,
        agent_id: String,
        mode: SessionMode,
    },
    SetAgentConfig {
        request_id: Uuid,
        agent_id: String,
        model: Option<String>,
        effort: Option<ReasoningEffort>,
    },
    SetAgentModel {
        request_id: Uuid,
        agent_id: String,
        model: Option<String>,
    },
    SwitchAgentProvider {
        request_id: Uuid,
        agent_id: String,
        provider: ProviderKind,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<ReasoningEffort>,
    },
    ArchiveAgent {
        request_id: Uuid,
        agent_id: String,
    },
    UnarchiveAgent {
        request_id: Uuid,
        agent_id: String,
    },
    DeleteAgent {
        request_id: Uuid,
        agent_id: String,
    },
    DetachAgent {
        request_id: Uuid,
        agent_id: String,
    },
    MarkAgentRead {
        request_id: Uuid,
        agent_id: String,
        /// Optional explicit highest seq read by the client; defaults to daemon's current `latest_seq`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seen_seq: Option<u64>,
    },
    MarkAgentUnread {
        request_id: Uuid,
        agent_id: String,
    },
    SetAgentPinned {
        request_id: Uuid,
        agent_id: String,
        pinned: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pinned_order: Option<i64>,
    },
    UpdateAgent {
        request_id: Uuid,
        agent_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<ReasoningEffort>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<SessionMode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        labels: Option<HashMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        handle: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fast_mode: Option<bool>,
    },
    ListPendingPermissions {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
    },
    PendingPermissionsSnapshot {
        request_id: Uuid,
        permissions: Vec<PendingPermissionSummary>,
    },
    /// Rewind an agent session to strictly before `to_before_seq` (removing that user prompt and all
    /// subsequent events), optionally restoring workspace files back to the pre-turn checkpoint.
    RewindAgent {
        request_id: Uuid,
        agent_id: String,
        to_before_seq: u64,
        #[serde(default)]
        restore_files: bool,
    },
    /// Authoritative post-rewind timeline replacement broadcast to all subscribed clients.
    AgentRewound {
        #[serde(default)]
        request_id: Option<Uuid>,
        agent: AgentSummary,
        to_before_seq: u64,
        events: Vec<TimelineEvent>,
        #[serde(default)]
        restored_files: u32,
        #[serde(default)]
        restored_prompt: Option<String>,
        #[serde(default)]
        notice: Option<String>,
    },
    ResolvePermission {
        request_id: Uuid,
        agent_id: String,
        permission_request_id: String,
        decision: String,
    },
    AnswerQuestion {
        request_id: Uuid,
        agent_id: String,
        question_request_id: String,
        answers: Vec<Vec<String>>,
    },
    Ack {
        request_id: Uuid,
    },

    // Sequenced Timeline Sync
    SubscribeTimeline {
        request_id: Uuid,
        agent_id: String,
        /// Client passes its local SQLite cursor for instant catch-up without full reload.
        since_seq: u64,
    },
    TimelineCatchup {
        request_id: Uuid,
        agent_id: String,
        events: Vec<TimelineEvent>,
        has_more: bool,
    },
    TimelineLive {
        event: TimelineEvent,
    },

    // Detached PTY Terminals
    ListTerminals {
        request_id: Uuid,
    },
    TerminalsSnapshot {
        request_id: Uuid,
        terminals: Vec<TerminalSummary>,
    },
    CreateTerminal {
        request_id: Uuid,
        cwd: String,
        title: Option<String>,
        cols: u16,
        rows: u16,
    },
    AttachTerminal {
        request_id: Uuid,
        terminal_id: String,
        slot: u8,
    },
    TerminalAttached {
        request_id: Uuid,
        terminal: TerminalSummary,
        slot: u8,
    },
    KillTerminal {
        request_id: Uuid,
        terminal_id: String,
    },
    CaptureTerminal {
        request_id: Uuid,
        terminal_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lines: Option<u32>,
    },
    TerminalCaptured {
        request_id: Uuid,
        terminal_id: String,
        text: String,
    },
    SendTerminalKeys {
        request_id: Uuid,
        terminal_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default)]
        keys: Vec<String>,
    },

    // Agent Profiles
    ListProfiles {
        request_id: Uuid,
    },
    ProfilesSnapshot {
        request_id: Uuid,
        profiles: Vec<AgentProfile>,
    },
    UpsertProfile {
        request_id: Uuid,
        profile: AgentProfile,
    },
    ProfileUpserted {
        request_id: Uuid,
        profile: AgentProfile,
    },
    DeleteProfile {
        request_id: Uuid,
        profile_id: String,
    },

    // Schedules & Heartbeats
    ListSchedules {
        request_id: Uuid,
        #[serde(default)]
        include_completed: bool,
    },
    SchedulesSnapshot {
        request_id: Uuid,
        schedules: Vec<ScheduleSpec>,
    },
    CreateSchedule {
        request_id: Uuid,
        name: String,
        cron_expr: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timezone: Option<String>,
        prompt: String,
        target: ScheduleTarget,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_runs: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created_by_agent: Option<String>,
    },
    ScheduleCreated {
        request_id: Uuid,
        schedule: ScheduleSpec,
    },
    ScheduleUpdated {
        schedule: ScheduleSpec,
    },
    ScheduleDeleted {
        schedule_id: String,
    },
    UpdateSchedule {
        request_id: Uuid,
        schedule_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cron_expr: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_runs: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        archive_on_finish: Option<bool>,
    },
    PauseSchedule {
        request_id: Uuid,
        schedule_id: String,
    },
    ResumeSchedule {
        request_id: Uuid,
        schedule_id: String,
    },
    RunScheduleOnce {
        request_id: Uuid,
        schedule_id: String,
    },
    DeleteSchedule {
        request_id: Uuid,
        schedule_id: String,
    },
    ListScheduleRuns {
        request_id: Uuid,
        schedule_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    ScheduleRunsSnapshot {
        request_id: Uuid,
        schedule_id: String,
        runs: Vec<ScheduleRunRecord>,
    },

    // Daemon-to-Daemon Connected Hosts (Cross-Host Orchestration)
    ListConnectedHosts {
        request_id: Uuid,
    },
    ConnectedHostsSnapshot {
        request_id: Uuid,
        hosts: Vec<ConnectedHostSummary>,
    },
    AddConnectedHost {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        pairing_uri: String,
    },
    ConnectedHostAdded {
        request_id: Uuid,
        host: ConnectedHostSummary,
    },
    ConnectedHostUpdated {
        host: ConnectedHostSummary,
    },
    ConnectedHostRemoved {
        registration_id: String,
    },
    UpdateConnectedHost {
        request_id: Uuid,
        registration_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        enabled: Option<bool>,
    },
    RemoveConnectedHost {
        request_id: Uuid,
        registration_id: String,
    },

    // Host Orchestration Config & Bundled Skills
    GetOrchestrationConfig {
        request_id: Uuid,
    },
    OrchestrationConfigSnapshot {
        request_id: Uuid,
        config: OrchestrationConfig,
    },
    UpdateOrchestrationConfig {
        request_id: Uuid,
        config: OrchestrationConfig,
    },
    ListOrchestrationSkills {
        request_id: Uuid,
    },
    OrchestrationSkillsSnapshot {
        request_id: Uuid,
        skills: Vec<OrchestrationSkillInfo>,
    },
    SyncOrchestrationSkills {
        request_id: Uuid,
        skill_ids: Vec<String>,
    },

    // System Keep-Awake (OS Sleep Prevention)
    GetKeepAwakeStatus {
        request_id: Uuid,
    },
    KeepAwakeStatusSnapshot {
        request_id: Uuid,
        status: KeepAwakeStatus,
    },
    KeepAwakeStatusUpdated {
        status: KeepAwakeStatus,
    },

    // Provider-Native Subagent Observability
    ListProviderSubagents {
        request_id: Uuid,
        parent_agent_id: String,
    },
    ProviderSubagentsSnapshot {
        request_id: Uuid,
        parent_agent_id: String,
        subagents: Vec<ProviderSubagentDescriptor>,
    },
    ProviderSubagentUpdated {
        subagent: ProviderSubagentDescriptor,
    },
    SubscribeProviderSubagent {
        request_id: Uuid,
        parent_agent_id: String,
        subagent_id: String,
        since_seq: u64,
    },
    ProviderSubagentTimelineCatchup {
        request_id: Uuid,
        parent_agent_id: String,
        subagent_id: String,
        events: Vec<ProviderSubagentEvent>,
        has_more: bool,
    },
    ProviderSubagentTimelineLive {
        event: ProviderSubagentEvent,
    },

    // Quotas & Provider Model Catalogs
    RefreshQuotas {
        request_id: Uuid,
    },
    QuotaReport {
        request_id: Option<Uuid>,
        snapshots: Vec<ProviderQuotaSnapshot>,
    },
    ListProviderModels {
        request_id: Uuid,
        #[serde(default)]
        force_refresh: bool,
    },
    ProviderModelsSnapshot {
        request_id: Uuid,
        catalogs: Vec<ProviderModelCatalog>,
    },

    // Remote Attachment Staging (Chunked to stay far below 16 MiB Zstd cap)
    StageAttachmentChunk {
        request_id: Uuid,
        upload_id: String,
        filename: String,
        chunk_index: u32,
        total_chunks: u32,
        data_b64: String,
    },
    AttachmentStaged {
        request_id: Uuid,
        upload_id: String,
        chunk_index: u32,
        completed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size_bytes: Option<u64>,
    },

    // Ephemeral Side Question (/btw — zero-tool, read-only, zero main-thread history pollution)
    AskSideQuestion {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        cwd: String,
        provider: ProviderKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        question: String,
        #[serde(default)]
        context_summary: String,
    },
    SideQuestionDelta {
        request_id: Uuid,
        text_delta: String,
    },
    SideQuestionDone {
        request_id: Uuid,
        full_text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    // Host-Authoritative Multi-Device Sync (Projects, Project Order, Sort Mode)
    GetHostSyncState {
        request_id: Uuid,
    },
    HostSyncStateSnapshot {
        request_id: Uuid,
        state: HostSyncState,
    },
    HostSyncStateUpdated {
        state: HostSyncState,
    },
    UpsertHostProject {
        request_id: Uuid,
        project: HostProjectEntry,
    },
    DeleteHostProject {
        request_id: Uuid,
        project_id: String,
    },
    SetHostProjectOrder {
        request_id: Uuid,
        project_order: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project_sort_mode: Option<String>,
    },

    // Mission Workspace Collaboration (Shared Blackboard, Activity/Conflict Feed, Clustering & Moves)
    GetWorkspaceBoard {
        request_id: Uuid,
        workspace_id: String,
    },
    WorkspaceBoardSnapshot {
        request_id: Uuid,
        board: WorkspaceBoardSnapshot,
    },
    UpdateWorkspaceBoard {
        request_id: Uuid,
        workspace_id: String,
        /// Full Markdown replacement when `section` is `None`, or section body when `section` is `Some`.
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        section: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_by: Option<String>,
    },
    WorkspaceBoardUpdated {
        board: WorkspaceBoardSnapshot,
    },
    ListWorkspaceEvents {
        request_id: Uuid,
        workspace_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    WorkspaceEventsSnapshot {
        request_id: Uuid,
        workspace_id: String,
        events: Vec<WorkspaceEventRecord>,
    },
    WorkspaceEventRecorded {
        event: WorkspaceEventRecord,
    },
    UpdateWorkspaceGoal {
        request_id: Uuid,
        workspace_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mission_goal: Option<String>,
    },
    MoveAgentToWorkspace {
        request_id: Uuid,
        agent_id: String,
        /// Target workspace ID; `None` splits the agent into a fresh curated Mission Workspace (or `待归类` when `make_unclassified == true`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_workspace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_workspace_title: Option<String>,
        /// When `true`, removes the agent from its curated Mission Workspace and places it in `待归类`.
        #[serde(default)]
        make_unclassified: bool,
    },
    MergeWorkspaces {
        request_id: Uuid,
        source_workspace_id: String,
        target_workspace_id: String,
    },
    DismissClusterSuggestion {
        request_id: Uuid,
        agent_id: String,
    },
    /// Trigger AI-assisted classification & semantic summarization for unclassified sessions in a project.
    OrganizeProjectWorkspaces {
        request_id: Uuid,
        project_root: String,
        #[serde(default)]
        agent_ids: Vec<String>,
        #[serde(default = "wire_default_true")]
        auto_apply: bool,
    },
    /// Trigger AI-assisted semantic tab handle & 1-line live summary update for a single conversation.
    SummarizeAgent {
        request_id: Uuid,
        agent_id: String,
    },
    /// Broadcast progress/completion update for AI workspace organization.
    WorkspaceOrganizeProgress {
        event: WorkspaceOrganizeEvent,
    },

    RpcError {
        request_id: Option<Uuid>,
        code: String,
        message: String,
    },
}

/// Binary PTY terminal stream frame (zero-JSON overhead for 120Hz mobile terminal rendering).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalBinaryFrame {
    Output { slot: u8, data: Vec<u8> },
    Input { slot: u8, data: Vec<u8> },
    Resize { slot: u8, cols: u16, rows: u16 },
    Snapshot { slot: u8, data: Vec<u8> },
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("zstd compression error: {0}")]
    Compression(#[from] std::io::Error),
    #[error("malformed binary terminal frame")]
    InvalidBinaryFrame,
}

/// Anti-bomb upper bound on a single decompressed Zstd `WireMessage` payload (32 MiB).
pub const MAX_ZSTD_WIRE_DECODE_BYTES: usize = 32 * 1024 * 1024;
/// Target uncompressed JSON byte budget per paginated timeline batch (`TimelineCatchup` / `AgentRewound`).
pub const MAX_WIRE_TIMELINE_PAGE_BYTES: usize = 4 * 1024 * 1024;

impl WireMessage {
    /// Encode message as Zstd-compressed JSON bytes for cross-border / mobile bandwidth efficiency.
    pub fn encode_zstd(&self, level: i32) -> Result<Vec<u8>, ProtocolError> {
        let json = serde_json::to_vec(self)?;
        Ok(zstd::encode_all(json.as_slice(), level)?)
    }

    /// Decode a Zstd-compressed WireMessage (with a 32 MiB anti-bomb cap).
    pub fn decode_zstd(compressed: &[u8]) -> Result<Self, ProtocolError> {
        use std::io::Read;
        let decoder = zstd::Decoder::new(compressed)?;
        let mut decompressed = Vec::new();
        decoder
            .take((MAX_ZSTD_WIRE_DECODE_BYTES as u64) + 1)
            .read_to_end(&mut decompressed)?;
        if decompressed.len() > MAX_ZSTD_WIRE_DECODE_BYTES {
            return Err(ProtocolError::Compression(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "zstd wire payload exceeds {} MiB limit",
                    MAX_ZSTD_WIRE_DECODE_BYTES / (1024 * 1024)
                ),
            )));
        }
        Ok(serde_json::from_slice(&decompressed)?)
    }
}

impl TerminalBinaryFrame {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Output { slot, data } => {
                let mut buf = Vec::with_capacity(2 + data.len());
                buf.push(0x01);
                buf.push(*slot);
                buf.extend_from_slice(data);
                buf
            }
            Self::Input { slot, data } => {
                let mut buf = Vec::with_capacity(2 + data.len());
                buf.push(0x02);
                buf.push(*slot);
                buf.extend_from_slice(data);
                buf
            }
            Self::Resize { slot, cols, rows } => {
                let mut buf = Vec::with_capacity(6);
                buf.push(0x03);
                buf.push(*slot);
                buf.extend_from_slice(&cols.to_be_bytes());
                buf.extend_from_slice(&rows.to_be_bytes());
                buf
            }
            Self::Snapshot { slot, data } => {
                let mut buf = Vec::with_capacity(2 + data.len());
                buf.push(0x04);
                buf.push(*slot);
                buf.extend_from_slice(data);
                buf
            }
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() < 2 {
            return Err(ProtocolError::InvalidBinaryFrame);
        }
        let opcode = bytes[0];
        let slot = bytes[1];
        match opcode {
            0x01 => Ok(Self::Output {
                slot,
                data: bytes[2..].to_vec(),
            }),
            0x02 => Ok(Self::Input {
                slot,
                data: bytes[2..].to_vec(),
            }),
            0x03 => {
                if bytes.len() != 6 {
                    return Err(ProtocolError::InvalidBinaryFrame);
                }
                let cols = u16::from_be_bytes([bytes[2], bytes[3]]);
                let rows = u16::from_be_bytes([bytes[4], bytes[5]]);
                Ok(Self::Resize { slot, cols, rows })
            }
            0x04 => Ok(Self::Snapshot {
                slot,
                data: bytes[2..].to_vec(),
            }),
            _ => Err(ProtocolError::InvalidBinaryFrame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zstd_wire_roundtrip() {
        let msg = WireMessage::CreateAgent {
            request_id: Uuid::new_v4(),
            cwd: "/workspace/orbit".into(),
            extra_dirs: vec!["/workspace/shared".into()],
            title: Some("Build Orbit".into()),
            provider: ProviderKind::Antigravity,
            model: Some("gemini-pro".into()),
            effort: Some(ReasoningEffort::High),
            mode: SessionMode::AcceptEdits,
            initial_prompt: "Build the Rust agent orchestrator!".repeat(20),
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree: false,
            workspace_id: None,
            parent_agent_id: None,
            parent_host_id: None,
            profile_id: None,
            background: false,
            auto_archive: false,
            notify_on_finish: None,
            idempotency_key: None,
            base_ref: None,
            worktree_mode: WorktreeCreateMode::BranchOff,
            branch: None,
            handle: Some("lead".into()),
            auto_cluster: None,
            fast_mode: Some(true),
        };
        let raw_json = serde_json::to_vec(&msg).expect("serialize");
        let compressed = msg.encode_zstd(3).expect("compress");
        assert!(compressed.len() < raw_json.len());
        let decoded = WireMessage::decode_zstd(&compressed).expect("decompress");
        match decoded {
            WireMessage::CreateAgent {
                provider,
                fast_mode,
                ..
            } => {
                assert_eq!(provider, ProviderKind::Antigravity);
                assert_eq!(fast_mode, Some(true));
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[test]
    fn backward_compatible_v2_deserialization() {
        let legacy_create = serde_json::json!({
            "op": "create_agent",
            "request_id": Uuid::new_v4(),
            "cwd": "/tmp/demo",
            "title": null,
            "provider": "claude_code",
            "model": null,
            "mode": "accept_edits",
            "initial_prompt": "hello"
        });
        let msg: WireMessage =
            serde_json::from_value(legacy_create).expect("deserialize legacy v2 CreateAgent");
        match msg {
            WireMessage::CreateAgent {
                workspace_id,
                parent_agent_id,
                background,
                auto_archive,
                ..
            } => {
                assert!(workspace_id.is_none());
                assert!(parent_agent_id.is_none());
                assert!(!background);
                assert!(!auto_archive);
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let legacy_send = serde_json::json!({
            "op": "send_prompt",
            "request_id": Uuid::new_v4(),
            "agent_id": "ag_1",
            "prompt": "follow up"
        });
        let msg2: WireMessage =
            serde_json::from_value(legacy_send).expect("deserialize legacy v2 SendPrompt");
        match msg2 {
            WireMessage::SendPrompt {
                active_turn_behavior,
                message_id,
                ..
            } => {
                assert_eq!(active_turn_behavior, ActiveTurnBehavior::Steer);
                assert!(message_id.is_none());
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let legacy_agent = serde_json::json!({
            "agent_id": "ag_legacy",
            "workspace_id": "ws_1",
            "cwd": "/tmp/demo",
            "title": "Legacy",
            "provider": "codex",
            "model": null,
            "mode": "default",
            "status": { "state": "idle" },
            "parent_agent_id": null,
            "latest_seq": 10,
            "created_at": Utc::now(),
            "updated_at": Utc::now()
        });
        let ag: AgentSummary =
            serde_json::from_value(legacy_agent).expect("deserialize legacy v2 AgentSummary");
        assert!(ag.archived_at.is_none());
        assert_eq!(ag.attention, AttentionState::None);
        assert!(ag.labels.is_empty());

        let cfg = OrchestrationConfig::default();
        assert!(cfg.mcp_inject_enabled);
        assert!(cfg.idle_cleanup_enabled);
        assert!(cfg.keep_awake_while_running);
        assert_eq!(cfg.idle_timeout_secs, 3600);

        let legacy_cfg: OrchestrationConfig =
            serde_json::from_str("{}").expect("deserialize legacy empty OrchestrationConfig");
        assert!(legacy_cfg.keep_awake_while_running);
    }

    #[test]
    fn binary_terminal_frame_roundtrip() {
        let frame = TerminalBinaryFrame::Resize {
            slot: 2,
            cols: 120,
            rows: 40,
        };
        let encoded = frame.encode();
        let decoded = TerminalBinaryFrame::decode(&encoded).expect("decode");
        assert_eq!(frame, decoded);
    }

    #[test]
    fn attachment_chunk_and_staged_zstd_roundtrip() {
        let req_id = Uuid::new_v4();
        let chunk = WireMessage::StageAttachmentChunk {
            request_id: req_id,
            upload_id: "up_123".into(),
            filename: "shot.png".into(),
            chunk_index: 0,
            total_chunks: 2,
            data_b64: "iVBORw0KGgo=".into(),
        };
        let z = chunk.encode_zstd(3).expect("compress chunk");
        let dec = WireMessage::decode_zstd(&z).expect("decode chunk");
        match dec {
            WireMessage::StageAttachmentChunk {
                request_id,
                upload_id,
                filename,
                chunk_index,
                total_chunks,
                data_b64,
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(upload_id, "up_123");
                assert_eq!(filename, "shot.png");
                assert_eq!(chunk_index, 0);
                assert_eq!(total_chunks, 2);
                assert_eq!(data_b64, "iVBORw0KGgo=");
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let staged = WireMessage::AttachmentStaged {
            request_id: req_id,
            upload_id: "up_123".into(),
            chunk_index: 1,
            completed: true,
            path: Some("/home/user/.orbit/attachments/123_shot.png".into()),
            filename: Some("shot.png".into()),
            size_bytes: Some(2048),
        };
        let z2 = staged.encode_zstd(3).expect("compress staged");
        let dec2 = WireMessage::decode_zstd(&z2).expect("decode staged");
        match dec2 {
            WireMessage::AttachmentStaged {
                completed,
                path,
                size_bytes,
                ..
            } => {
                assert!(completed);
                assert_eq!(
                    path.as_deref(),
                    Some("/home/user/.orbit/attachments/123_shot.png")
                );
                assert_eq!(size_bytes, Some(2048));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn workspace_file_and_side_question_wire_roundtrip() {
        let req_id = Uuid::new_v4();
        let read_req = WireMessage::ReadWorkspaceFile {
            request_id: req_id,
            cwd: "/workspace/orbit".into(),
            path: "README.md".into(),
            max_bytes: Some(1_048_576),
        };
        let encoded = read_req.encode_zstd(3).expect("encode read request");
        match WireMessage::decode_zstd(&encoded).expect("decode read request") {
            WireMessage::ReadWorkspaceFile {
                request_id,
                cwd,
                path,
                max_bytes,
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(cwd, "/workspace/orbit");
                assert_eq!(path, "README.md");
                assert_eq!(max_bytes, Some(1_048_576));
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let side_req = WireMessage::AskSideQuestion {
            request_id: req_id,
            agent_id: Some("ag_1".into()),
            cwd: "/workspace/orbit".into(),
            provider: ProviderKind::ClaudeCode,
            model: Some("sonnet".into()),
            question: "Why did we use RwLock here?".into(),
            context_summary: "Active turn editing orbit-client-core.".into(),
        };
        let encoded2 = side_req.encode_zstd(3).expect("encode side question");
        match WireMessage::decode_zstd(&encoded2).expect("decode side question") {
            WireMessage::AskSideQuestion {
                request_id,
                provider,
                question,
                ..
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(provider, ProviderKind::ClaudeCode);
                assert_eq!(question, "Why did we use RwLock here?");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn rewind_agent_wire_roundtrip() {
        let req_id = Uuid::new_v4();
        let req = WireMessage::RewindAgent {
            request_id: req_id,
            agent_id: "ag_rewind".into(),
            to_before_seq: 42,
            restore_files: true,
        };
        let encoded = req.encode_zstd(3).expect("encode rewind request");
        match WireMessage::decode_zstd(&encoded).expect("decode rewind request") {
            WireMessage::RewindAgent {
                request_id,
                agent_id,
                to_before_seq,
                restore_files,
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(agent_id, "ag_rewind");
                assert_eq!(to_before_seq, 42);
                assert!(restore_files);
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let now = Utc::now();
        let reply = WireMessage::AgentRewound {
            request_id: Some(req_id),
            agent: AgentSummary {
                agent_id: "ag_rewind".into(),
                workspace_id: "ws_1".into(),
                cwd: "/workspace/orbit".into(),
                title: "Refactor".into(),
                provider: ProviderKind::ClaudeCode,
                model: Some("sonnet".into()),
                effort: None,
                fast_mode: false,
                mode: SessionMode::Default,
                status: AgentStatus::Idle,
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
                queued_prompts: Vec::new(),
                read_seq: 12,
                is_pinned: false,
                pinned_order: 0,
                handle: Some("lead".into()),
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: None,
                forked_at_seq: None,
            },
            to_before_seq: 42,
            events: Vec::new(),
            restored_files: 3,
            restored_prompt: Some("Update layout".into()),
            notice: None,
        };
        let encoded_reply = reply.encode_zstd(3).expect("encode rewind reply");
        match WireMessage::decode_zstd(&encoded_reply).expect("decode rewind reply") {
            WireMessage::AgentRewound {
                request_id,
                to_before_seq,
                restored_files,
                restored_prompt,
                ..
            } => {
                assert_eq!(request_id, Some(req_id));
                assert_eq!(to_before_seq, 42);
                assert_eq!(restored_files, 3);
                assert_eq!(restored_prompt.as_deref(), Some("Update layout"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn provider_kind_all_13_metadata_slugs_and_serde_roundtrip() {
        assert_eq!(ProviderKind::all().len(), 13);
        let mut seen_slugs = std::collections::HashSet::new();
        let mut seen_env_keys = std::collections::HashSet::new();
        for &kind in ProviderKind::all() {
            assert!(
                seen_slugs.insert(kind.slug()),
                "duplicate slug for {kind:?}"
            );
            assert!(
                seen_env_keys.insert(kind.env_override_key()),
                "duplicate env key for {kind:?}"
            );
            assert!(!kind.default_binary().is_empty());
            assert!(!kind.display_name().is_empty());
            assert!(!kind.short_label().is_empty());
            assert!(!kind.protocol_transport().is_empty());
            assert_eq!(ProviderKind::from_slug(kind.slug()), Some(kind));

            let serialized = serde_json::to_string(&kind).expect("serialize provider");
            assert_eq!(serialized, format!("\"{}\"", kind.slug()));
            let deserialized: ProviderKind =
                serde_json::from_str(&serialized).expect("deserialize provider");
            assert_eq!(deserialized, kind);
        }

        // Alias parsing & serde deserialization
        let aliases = [
            ("agy", ProviderKind::Antigravity),
            ("claude", ProviderKind::ClaudeCode),
            ("open_code", ProviderKind::OpenCode),
            ("opencode", ProviderKind::OpenCode),
            ("gemini", ProviderKind::GeminiCli),
            ("cursor", ProviderKind::CursorAgent),
            ("github_copilot", ProviderKind::Copilot),
            ("kimi_cli", ProviderKind::Kimi),
            ("vibe", ProviderKind::MistralVibe),
            ("vibe_acp", ProviderKind::MistralVibe),
            ("auggie", ProviderKind::Augment),
        ];
        for (alias, expected) in aliases {
            assert_eq!(ProviderKind::from_slug(alias), Some(expected));
            let wire = format!("\"{alias}\"");
            let parsed: ProviderKind =
                serde_json::from_str(&wire).expect("deserialize provider alias");
            assert_eq!(parsed, expected);
        }
        assert_eq!(ProviderKind::MistralVibe.fallback_binaries(), &["vibe"]);
    }

    #[test]
    fn fork_agent_wire_roundtrip_and_legacy_compat() {
        let req_id = Uuid::new_v4();
        let req = WireMessage::ForkAgent {
            request_id: req_id,
            source_agent_id: "ag_source".into(),
            up_to_seq: Some(15),
            target: ForkTarget::NewWorktree {
                branch: Some("orbit/fork-exp".into()),
                base_ref: Some("main".into()),
            },
            title: Some("Experiment".into()),
            initial_prompt: Some("Try approach B".into()),
        };
        let encoded = req.encode_zstd(3).expect("encode fork request");
        match WireMessage::decode_zstd(&encoded).expect("decode fork request") {
            WireMessage::ForkAgent {
                request_id,
                source_agent_id,
                up_to_seq,
                target,
                title,
                initial_prompt,
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(source_agent_id, "ag_source");
                assert_eq!(up_to_seq, Some(15));
                assert_eq!(
                    target,
                    ForkTarget::NewWorktree {
                        branch: Some("orbit/fork-exp".into()),
                        base_ref: Some("main".into()),
                    }
                );
                assert_eq!(title.as_deref(), Some("Experiment"));
                assert_eq!(initial_prompt.as_deref(), Some("Try approach B"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        // Minimal legacy-style JSON without optional fields defaults to SameDirectory
        let minimal_json = serde_json::json!({
            "op": "fork_agent",
            "request_id": req_id,
            "source_agent_id": "ag_source"
        });
        match serde_json::from_value::<WireMessage>(minimal_json).expect("deserialize minimal fork")
        {
            WireMessage::ForkAgent {
                up_to_seq,
                target,
                title,
                initial_prompt,
                ..
            } => {
                assert!(up_to_seq.is_none());
                assert_eq!(target, ForkTarget::SameDirectory);
                assert!(title.is_none());
                assert!(initial_prompt.is_none());
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let now = Utc::now();
        let reply = WireMessage::AgentForked {
            request_id: req_id,
            agent: AgentSummary {
                agent_id: "ag_forked".into(),
                workspace_id: "ws_wt".into(),
                cwd: "/tmp/wt".into(),
                title: "Experiment".into(),
                provider: ProviderKind::ClaudeCode,
                model: Some("sonnet".into()),
                effort: Some(ReasoningEffort::High),
                fast_mode: false,
                mode: SessionMode::AcceptEdits,
                status: AgentStatus::Idle,
                parent_agent_id: None,
                parent_host_id: None,
                latest_seq: 15,
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
                read_seq: 15,
                is_pinned: false,
                pinned_order: 0,
                handle: Some("fork".into()),
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: Some("ag_source".into()),
                forked_at_seq: Some(15),
            },
            workspace: Some(WorkspaceSummary {
                workspace_id: "ws_wt".into(),
                title: "Experiment".into(),
                cwd: "/tmp/wt".into(),
                git_branch: Some("orbit/fork-exp".into()),
                is_worktree: true,
                updated_at: now,
                source_cwd: Some("/tmp/repo".into()),
                base_ref: Some("main".into()),
                parent_workspace_id: Some("ws_1".into()),
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: Some("/tmp/repo".into()),
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            }),
        };
        let encoded_reply = reply.encode_zstd(3).expect("encode fork reply");
        match WireMessage::decode_zstd(&encoded_reply).expect("decode fork reply") {
            WireMessage::AgentForked {
                request_id,
                agent,
                workspace,
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(agent.agent_id, "ag_forked");
                assert_eq!(agent.forked_from_agent_id.as_deref(), Some("ag_source"));
                assert_eq!(agent.forked_at_seq, Some(15));
                assert_eq!(
                    workspace.as_ref().map(|w| w.workspace_id.as_str()),
                    Some("ws_wt")
                );
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        // Legacy AgentSummary JSON without fork provenance fields deserializes cleanly
        let legacy_agent = serde_json::json!({
            "agent_id": "ag_old",
            "workspace_id": "ws_1",
            "cwd": "/tmp/demo",
            "title": "Old",
            "provider": "codex",
            "model": null,
            "mode": "default",
            "status": { "state": "idle" },
            "parent_agent_id": null,
            "latest_seq": 3,
            "created_at": now,
            "updated_at": now
        });
        let parsed: AgentSummary = serde_json::from_value(legacy_agent).expect("legacy agent");
        assert!(parsed.forked_from_agent_id.is_none());
        assert!(parsed.forked_at_seq.is_none());
    }

    #[test]
    fn fast_mode_legacy_wire_deserialization_defaults_false() {
        let legacy_agent_json = r#"{
            "agent_id": "ag_legacy",
            "workspace_id": "ws_1",
            "cwd": "/repo",
            "title": "Legacy",
            "provider": "claude_code",
            "model": "claude-opus-4-6",
            "mode": "accept_edits",
            "status": { "state": "idle" },
            "parent_agent_id": null,
            "latest_seq": 1,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }"#;
        let agent: AgentSummary = serde_json::from_str(legacy_agent_json).expect("legacy agent");
        assert!(!agent.fast_mode);

        let legacy_create_json = format!(
            r#"{{
                "op": "create_agent",
                "request_id": "{}",
                "cwd": "/repo",
                "title": null,
                "provider": "codex",
                "model": "gpt-5.4",
                "mode": "accept_edits",
                "initial_prompt": "hello"
            }}"#,
            Uuid::new_v4()
        );
        match serde_json::from_str::<WireMessage>(&legacy_create_json).expect("legacy create") {
            WireMessage::CreateAgent { fast_mode, .. } => assert_eq!(fast_mode, None),
            other => panic!("unexpected variant: {other:?}"),
        }

        let explicit_false_create_json = format!(
            r#"{{
                "op": "create_agent",
                "request_id": "{}",
                "cwd": "/repo",
                "title": null,
                "provider": "codex",
                "model": "gpt-5.4",
                "mode": "accept_edits",
                "initial_prompt": "hello",
                "fast_mode": false
            }}"#,
            Uuid::new_v4()
        );
        match serde_json::from_str::<WireMessage>(&explicit_false_create_json)
            .expect("explicit false create")
        {
            WireMessage::CreateAgent { fast_mode, .. } => assert_eq!(fast_mode, Some(false)),
            other => panic!("unexpected variant: {other:?}"),
        }

        let legacy_update_json = format!(
            r#"{{
                "op": "update_agent",
                "request_id": "{}",
                "agent_id": "ag_legacy",
                "title": "Renamed"
            }}"#,
            Uuid::new_v4()
        );
        match serde_json::from_str::<WireMessage>(&legacy_update_json).expect("legacy update") {
            WireMessage::UpdateAgent { fast_mode, .. } => assert_eq!(fast_mode, None),
            other => panic!("unexpected variant: {other:?}"),
        }

        let update_fast = WireMessage::UpdateAgent {
            request_id: Uuid::new_v4(),
            agent_id: "ag_legacy".into(),
            title: None,
            model: None,
            effort: None,
            mode: None,
            labels: None,
            handle: None,
            task_summary: None,
            fast_mode: Some(true),
        };
        let encoded = update_fast.encode_zstd(3).expect("encode update fast");
        match WireMessage::decode_zstd(&encoded).expect("decode update fast") {
            WireMessage::UpdateAgent { fast_mode, .. } => assert_eq!(fast_mode, Some(true)),
            other => panic!("unexpected variant: {other:?}"),
        }

        let legacy_model_json = r#"{
            "id": "claude-opus-4-6",
            "label": "Opus 4.6"
        }"#;
        let spec: ProviderModelSpec =
            serde_json::from_str(legacy_model_json).expect("legacy model spec");
        assert!(!spec.supports_fast_mode);

        let legacy_profile_json = r#"{
            "profile_id": "p1",
            "name": "Reviewer",
            "provider": "codex",
            "mode": "plan",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }"#;
        let prof: AgentProfile = serde_json::from_str(legacy_profile_json).expect("legacy profile");
        assert!(!prof.fast_mode);
    }
}
