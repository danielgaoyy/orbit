//! Core wire types, state machines, timeline events, git/worktree models, orchestration models, and binary terminal framing.

pub mod acp;
pub mod artifacts;
pub mod browser;
pub mod git_collab;
pub mod goals;
pub mod guardian;
pub mod handoff;
pub mod hooks;
pub mod im_channels;
pub mod instructions;
pub mod lsp;
pub mod mcp_servers;
pub mod media;
pub mod orchestration;
pub mod push;
pub mod router;
pub mod rules;
pub mod skills;
pub mod timers;
pub mod triggers;
pub mod usage;
pub mod workflows;
pub use acp::*;
pub use artifacts::*;
pub use browser::*;
pub use git_collab::*;
pub use goals::*;
pub use guardian::*;
pub use handoff::*;
pub use hooks::*;
pub use im_channels::*;
pub use instructions::*;
pub use lsp::*;
pub use mcp_servers::*;
pub use media::*;
pub use orchestration::*;
pub use push::*;
pub use router::*;
pub use rules::*;
pub use skills::*;
pub use timers::*;
pub use triggers::*;
pub use usage::*;
pub use workflows::*;

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 4;

/// Canonical default GitHub repository (`owner/repo`) for Orbit releases, self-updates,
/// and remote installer bootstrapping.
///
/// Forks and organizations can override this in one place at runtime or compile time via
/// `ORBIT_GITHUB_REPO=owner/repo` (validated by [`configured_github_repo`]).
pub const DEFAULT_GITHUB_REPO: &str = "danielgaoyy/orbit";

/// Validate a GitHub `owner/repo` slug against strict safe ASCII characters
/// (`^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$`), rejecting `.` segments, `..` traversal,
/// extra slashes, whitespace, or shell/URL injection characters.
pub fn is_valid_github_repo(raw: &str) -> bool {
    let Some((owner, repo)) = raw.split_once('/') else {
        return false;
    };
    if owner.is_empty() || repo.is_empty() || owner.len() > 100 || repo.len() > 100 {
        return false;
    }
    if owner == "." || repo == "." || owner.contains("..") || repo.contains("..") {
        return false;
    }
    let is_safe_segment = |part: &str| {
        part.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    is_safe_segment(owner) && is_safe_segment(repo)
}

fn resolve_github_repo_from(runtime_env: Option<&str>, compile_env: Option<&str>) -> String {
    if let Some(val) = runtime_env
        && is_valid_github_repo(val)
    {
        return val.to_string();
    }
    if let Some(val) = compile_env
        && is_valid_github_repo(val)
    {
        return val.to_string();
    }
    DEFAULT_GITHUB_REPO.to_string()
}

/// Resolve the configured GitHub repository (`owner/repo`) for release downloads and updates.
///
/// Precedence:
/// 1. Runtime `ORBIT_GITHUB_REPO` environment variable (if strictly valid).
/// 2. Compile-time `option_env!("ORBIT_GITHUB_REPO")` (if strictly valid).
/// 3. [`DEFAULT_GITHUB_REPO`] (`"danielgaoyy/orbit"`).
pub fn configured_github_repo() -> String {
    let runtime = std::env::var("ORBIT_GITHUB_REPO").ok();
    resolve_github_repo_from(runtime.as_deref(), option_env!("ORBIT_GITHUB_REPO"))
}

fn wire_default_true() -> bool {
    true
}

fn wire_default_symbol_limit() -> u32 {
    30
}

fn wire_default_lsp_limit() -> u32 {
    50
}

fn wire_default_usage_days() -> u32 {
    7
}

fn wire_default_workflow_concurrency() -> u32 {
    3
}

fn wire_is_false(v: &bool) -> bool {
    !*v
}

/// First-class supported coding agent CLIs in Orbit (native drivers + Universal ACP).
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
    Acp,
}

impl ProviderKind {
    const NATIVE_PROVIDERS: [Self; 13] = [
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

    const ALL_PROVIDERS: [Self; 14] = [
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
        Self::Acp,
    ];

    /// All 13 first-class native coding agent providers in canonical display order.
    pub const fn native_all() -> &'static [Self] {
        &Self::NATIVE_PROVIDERS
    }

    /// All 14 coding agent providers (13 native + Universal ACP) in canonical display order.
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
            Self::Acp => "acp",
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
            "acp" => Some(Self::Acp),
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
            Self::Acp => "acp",
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
            Self::Acp => "ACP Agent",
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
            Self::Acp => "ACP",
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
            Self::Acp => "ORBIT_ACP_BIN",
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
            Self::Acp => "acp-generic-stdio",
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

/// Normalized permission/execution mode across all providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Default,
    #[serde(rename = "auto")]
    Auto,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
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
    /// `true` for explicit/curated Mission Workspaces; `false` for unfiled inbox containers.
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
    #[serde(default)]
    pub nested_git_repos: Vec<NestedGitRepo>,
}

/// Directory entry for interactive remote/local folder browsing (`+ Add project folder`).
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
    #[serde(default)]
    pub nested_git_repos: Vec<NestedGitRepo>,
}

/// Result payload returned after committing staged changes in a Git repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommitResult {
    pub commit_sha: String,
    pub short_sha: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// Result payload returned after pushing commits to a Git remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitPushResult {
    pub remote: String,
    pub branch: String,
    pub output: String,
}

/// Result payload returned after creating a Pull Request via GitHub CLI (`gh pr create`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCreatePrResult {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    pub title: String,
}

/// Typed outcome returned by `WireMessage::GitMutationResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GitMutationOutcome {
    FilesStaged {
        paths: Vec<String>,
        all: bool,
    },
    FilesUnstaged {
        paths: Vec<String>,
        all: bool,
    },
    HunkApplied {
        reverse: bool,
        cached: bool,
    },
    FilesDiscarded {
        paths: Vec<String>,
        include_untracked: bool,
    },
    Committed(GitCommitResult),
    Pushed(GitPushResult),
    PrCreated(GitCreatePrResult),
    StashMutated(GitStashMutationResult),
    RebaseAction(GitRebaseActionResult),
    ConflictResolved(GitResolveConflictResult),
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
    /// Optional ACP registry agent ID when `provider == ProviderKind::Acp` (e.g. `"opencode"`, `"gemini-cli"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_agent_id: Option<String>,
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

/// A lightweight jump target, independent of the page of timeline events being delivered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineOutlinePrompt {
    pub seq: u64,
    pub preview: String,
}

impl TimelineOutlinePrompt {
    /// Project the same bounded, user-visible preview for stored, live and replica prompts.
    pub fn from_user_prompt(seq: u64, raw: &str, badges: &[PromptBadge]) -> Option<Self> {
        let (text, directive) = display_prompt_parts(raw);
        if is_compact_prompt(text, directive, badges) {
            return None;
        }
        let preview = if text.is_empty() {
            let mut labels = badges
                .iter()
                .flat_map(|badge| badge.label.split_whitespace())
                .peekable();
            if labels.peek().is_some() {
                outline_preview(labels)
            } else {
                outline_preview(directive.unwrap_or("Prompt").split_whitespace())
            }
        } else {
            outline_preview(text.split_whitespace())
        };
        Some(Self { seq, preview })
    }
}

fn outline_preview<'a>(words: impl Iterator<Item = &'a str>) -> String {
    let mut preview: String = words
        .flat_map(|w| w.chars().chain(std::iter::once(' ')))
        .take(110)
        .collect();
    preview.truncate(preview.trim_end().len());
    preview
}

/// Whether a user turn is represented by a compaction milestone on the stage.
pub fn is_compaction_command(raw: &str, badges: &[PromptBadge]) -> bool {
    let (text, directive) = display_prompt_parts(raw);
    is_compact_prompt(text, directive, badges)
}

fn is_compact_prompt(text: &str, directive: Option<&str>, badges: &[PromptBadge]) -> bool {
    const COMMAND: &str = "/compact";
    text.get(..COMMAND.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(COMMAND))
        && text[COMMAND.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        || text.is_empty()
            && (directive == Some(COMMAND)
                || badges
                    .iter()
                    .any(|badge| badge.kind == "slash" && badge.label == COMMAND))
}

// Legacy records can contain the wire prompt. Only unwrap Orbit's leading envelopes,
// preserving examples of these tags in the user's actual request.
fn display_prompt_parts(raw: &str) -> (&str, Option<&str>) {
    let mut rest = raw.trim();
    let mut directive = None;
    while let Some(tag) = rest.strip_prefix('<') {
        let name = tag
            .split(['>', ' ', '\n', '\t', '\r'])
            .next()
            .unwrap_or_default();
        let close = match name {
            "orbit-context" => "</orbit-context>",
            "orbit-workspace" => "</orbit-workspace>",
            "orbit-workspace-delta" => "</orbit-workspace-delta>",
            "orbit-fleet-state" => "</orbit-fleet-state>",
            "orbit-fleet-delta" => "</orbit-fleet-delta>",
            "orbit-system" => "</orbit-system>",
            "WORKSPACE_RULES" => "</WORKSPACE_RULES>",
            "WORKFLOW_INSTRUCTION" => "</WORKFLOW_INSTRUCTION>",
            "ATTACHED_CONTEXT" => "</ATTACHED_CONTEXT>",
            _ => break,
        };
        let Some((block, tail)) = tag.split_once(close) else {
            break;
        };
        if name == "WORKFLOW_INSTRUCTION" {
            directive = block.split_once('>').and_then(|(opening, _)| {
                opening
                    .split_once("command=\"")?
                    .1
                    .split_once('"')
                    .map(|(command, _)| command)
            });
        }
        rest = tail.trim_start();
    }
    let text = rest
        .strip_prefix("<USER_REQUEST>")
        .and_then(|body| body.strip_suffix("</USER_REQUEST>"))
        .unwrap_or(rest)
        .trim();
    (text, directive)
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
#[allow(clippy::large_enum_variant)]
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guardian_assessment: Option<GuardianAssessment>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delivery_status: Option<AgentMessageDeliveryStatus>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delivery_turn_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocking_turn_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queued_at_ms: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dispatched_at_ms: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        settled_at_ms: Option<i64>,
    },
    /// Monotonic lifecycle update for an already-emitted [`TimelinePayload::AgentMessage`],
    /// preserving chronological `seq` order while letting clients update delivery status in place.
    AgentMessageDeliveryUpdated {
        message_id: String,
        status: AgentMessageDeliveryStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delivery_turn_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocking_turn_id: Option<String>,
        timestamp_ms: i64,
    },
    StatusChanged {
        status: AgentStatus,
    },
}

/// Optimistic concurrency conflict returned when saving a workspace file whose on-disk
/// SHA-256 `etag` changed since it was loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFileConflict {
    pub current_etag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_content: Option<String>,
    pub current_size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_mtime_unix_ms: Option<i64>,
}

/// Indexed workspace code symbol entry for `@symbol` fuzzy citation (pure-Rust fast index + optional LSP enrichment).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSymbolEntry {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    pub language: String,
}

/// Full-text search match returned by a Daemon's authoritative SQLite FTS5 index
/// (`timeline_events_fts`), covering full conversation history independent of client pagination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteMessageSearchHit {
    pub agent_id: String,
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub role: String,
    pub snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub bm25_score: f64,
    #[serde(default)]
    pub agent_title: String,
    #[serde(default)]
    pub cwd: String,
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
        /// When `true`, disbands the workspace and moves its active conversations back to unfiled inbox instead of archiving them.
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
    GitStageFiles {
        request_id: Uuid,
        cwd: String,
        #[serde(default)]
        paths: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    GitUnstageFiles {
        request_id: Uuid,
        cwd: String,
        #[serde(default)]
        paths: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    GitApplyHunk {
        request_id: Uuid,
        cwd: String,
        patch: String,
        #[serde(default)]
        reverse: bool,
        #[serde(default)]
        cached: bool,
    },
    GitDiscardFiles {
        request_id: Uuid,
        cwd: String,
        paths: Vec<String>,
        #[serde(default)]
        include_untracked: bool,
    },
    GitCommit {
        request_id: Uuid,
        cwd: String,
        message: String,
        #[serde(default)]
        amend: bool,
    },
    GitPush {
        request_id: Uuid,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default)]
        set_upstream: bool,
    },
    GitCreatePr {
        request_id: Uuid,
        cwd: String,
        title: String,
        #[serde(default)]
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<String>,
        #[serde(default)]
        draft: bool,
    },
    GitMutationResult {
        request_id: Uuid,
        cwd: String,
        result: GitMutationOutcome,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<GitStatusSnapshot>,
    },
    GitStashList {
        request_id: Uuid,
        cwd: String,
    },
    GitStashPush {
        request_id: Uuid,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default)]
        include_untracked: bool,
    },
    GitStashApply {
        request_id: Uuid,
        cwd: String,
        stash_ref: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_oid: Option<String>,
    },
    GitStashPop {
        request_id: Uuid,
        cwd: String,
        stash_ref: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_oid: Option<String>,
    },
    GitStashDrop {
        request_id: Uuid,
        cwd: String,
        stash_ref: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_oid: Option<String>,
    },
    GitRebaseStatus {
        request_id: Uuid,
        cwd: String,
    },
    GitRebaseStart {
        request_id: Uuid,
        cwd: String,
        onto_ref: String,
    },
    GitRebaseContinue {
        request_id: Uuid,
        cwd: String,
    },
    GitRebaseAbort {
        request_id: Uuid,
        cwd: String,
    },
    GitRebaseSkip {
        request_id: Uuid,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_step: Option<u32>,
    },
    GitListConflicts {
        request_id: Uuid,
        cwd: String,
    },
    GitResolveConflict {
        request_id: Uuid,
        cwd: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block_index: Option<u32>,
        resolution: GitConflictResolution,
        #[serde(default = "wire_default_true")]
        auto_stage: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_sha256: Option<String>,
    },
    GitPrStatus {
        request_id: Uuid,
        cwd: String,
    },
    GitGenerateCommitMessage {
        request_id: Uuid,
        cwd: String,
    },
    GitCollabResponse {
        request_id: Uuid,
        cwd: String,
        payload: GitCollabQueryResult,
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
    SearchWorkspaceSymbols {
        request_id: Uuid,
        cwd: String,
        query: String,
        #[serde(default = "wire_default_symbol_limit")]
        limit: u32,
    },
    WorkspaceSymbolsResponse {
        request_id: Uuid,
        cwd: String,
        symbols: Vec<WorkspaceSymbolEntry>,
        #[serde(default)]
        truncated: bool,
    },
    /// Query the daemon's authoritative full-history SQLite FTS5 index (`timeline_events_fts`).
    /// When `agent_id` is `Some`, returns individual matching events within that session
    /// (for in-session `Cmd+F` across unloaded historical turns); when `None`, returns the
    /// highest-ranked match per conversation across all sessions on this host.
    SearchMessages {
        request_id: Uuid,
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    SearchMessagesResult {
        request_id: Uuid,
        hits: Vec<RemoteMessageSearchHit>,
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
        /// `"text"` | `"image"` | `"pdf"` | `"audio"` | `"video"` | `"binary"`
        content_kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        content: String,
        size_bytes: u64,
        truncated: bool,
        /// SHA-256 hex digest of exact disk bytes for optimistic concurrency.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        etag: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mtime_unix_ms: Option<i64>,
        #[serde(default)]
        writable: bool,
        /// `"lf"` | `"crlf"` | `"mixed"`
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_ending: Option<String>,
    },
    WriteWorkspaceFile {
        request_id: Uuid,
        cwd: String,
        path: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_etag: Option<String>,
        #[serde(default)]
        force: bool,
    },
    WorkspaceFileWritten {
        request_id: Uuid,
        cwd: String,
        path: String,
        etag: String,
        size_bytes: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mtime_unix_ms: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conflict: Option<WorkspaceFileConflict>,
    },
    CreateWorkspaceEntry {
        request_id: Uuid,
        cwd: String,
        path: String,
        #[serde(default)]
        is_dir: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial_content: Option<String>,
    },
    RenameWorkspaceEntry {
        request_id: Uuid,
        cwd: String,
        old_path: String,
        new_path: String,
    },
    DeleteWorkspaceEntry {
        request_id: Uuid,
        cwd: String,
        path: String,
    },
    WorkspaceEntryMutated {
        request_id: Uuid,
        cwd: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_path: Option<String>,
        /// `"created_file"` | `"created_dir"` | `"renamed"` | `"deleted"`
        action: String,
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
        /// Optional ACP registry agent ID when `provider == ProviderKind::Acp`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        acp_agent_id: Option<String>,
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
        #[serde(default, skip_serializing_if = "wire_is_false")]
        preserve_queue: bool,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        acp_agent_id: Option<String>,
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
        /// When `since_seq == 0` and set, return only the latest `tail_limit` events (tail-first).
        /// An explicit open with a saved cursor can set this to request the outline alongside incremental events.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tail_limit: Option<u32>,
    },
    TimelineCatchup {
        request_id: Uuid,
        agent_id: String,
        events: Vec<TimelineEvent>,
        has_more: bool,
        #[serde(default)]
        has_older_history: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        outline_prompts: Vec<TimelineOutlinePrompt>,
    },
    FetchTimelineBefore {
        request_id: Uuid,
        agent_id: String,
        before_seq: u64,
        #[serde(default)]
        limit: u32,
    },
    TimelineHistoryPage {
        request_id: Uuid,
        agent_id: String,
        events: Vec<TimelineEvent>,
        has_more_older: bool,
    },
    TimelineLive {
        event: TimelineEvent,
    },
    ConfigureHostRelay {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay_url: Option<String>,
    },
    HostRelayConfigured {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        relay_url: Option<String>,
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
        timezone: Option<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ssh_alias: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_daemon_port: Option<u16>,
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

    // Personal Router ("Chief-of-Staff / 分工师") & Multi-Host Fleet Router Discovery
    GetPersonalRouterStatus {
        request_id: Uuid,
    },
    PersonalRouterStatusSnapshot {
        request_id: Uuid,
        status: RouterStatusSnapshot,
    },
    ConfigurePersonalRouter {
        request_id: Uuid,
        config: PersonalRouterConfig,
        /// Optional secret for outbound external Hermes webhook (never returned in snapshots).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        external_webhook_secret: Option<String>,
        /// When `true` and `config.enabled == true`, immediately create/resume the canonical Router agent.
        #[serde(default)]
        activate: bool,
    },
    ProbeRouterCapabilities {
        request_id: Uuid,
    },
    RouterCapabilitiesResponse {
        request_id: Uuid,
        capabilities: RouterHostCapabilities,
    },
    DiscoverFleetRouterHosts {
        request_id: Uuid,
    },
    FleetRouterHostsSnapshot {
        request_id: Uuid,
        hosts: Vec<RouterHostCapabilities>,
    },
    ExportHermesBridgeConfig {
        request_id: Uuid,
    },
    HermesBridgeConfigResponse {
        request_id: Uuid,
        bundle: HermesBridgeBundle,
    },
    InspectProjectContext {
        request_id: Uuid,
        cwd: String,
    },
    ProjectContextResponse {
        request_id: Uuid,
        context: ProjectContextPacket,
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

    // Skills Ecosystem & Managed Skills CRUD
    ListManagedSkills {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    ManagedSkillsSnapshot {
        request_id: Uuid,
        skills: Vec<ManagedSkillRecord>,
    },
    ReadManagedSkill {
        request_id: Uuid,
        skill_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    ManagedSkillContentResponse {
        request_id: Uuid,
        skill_id: String,
        content: String,
        etag: String,
    },
    UpsertManagedSkill {
        request_id: Uuid,
        scope: SkillWriteScope,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        slug: String,
        name: String,
        description: String,
        content: String,
        enabled: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_etag: Option<String>,
    },
    ToggleManagedSkill {
        request_id: Uuid,
        skill_id: String,
        enabled: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    DeleteManagedSkill {
        request_id: Uuid,
        skill_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    ImportManagedSkill {
        request_id: Uuid,
        scope: SkillWriteScope,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        source_markdown_or_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slug_override: Option<String>,
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
        /// Target workspace ID; `None` splits the agent into a fresh curated Mission Workspace (or unfiled inbox when `make_unclassified == true`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_workspace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_workspace_title: Option<String>,
        /// When `true`, removes the agent from its curated Mission Workspace and places it in the unfiled inbox.
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

    // Universal ACP Agent Registry & Live Probing
    ListAcpAgents {
        request_id: Uuid,
    },
    AcpAgentsSnapshot {
        request_id: Uuid,
        agents: Vec<AcpAgentAvailability>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        warning: Option<String>,
    },
    UpsertCustomAcpAgent {
        request_id: Uuid,
        spec: AcpAgentSpec,
    },
    DeleteCustomAcpAgent {
        request_id: Uuid,
        agent_id: String,
    },
    TestAcpAgent {
        request_id: Uuid,
        agent_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    AcpAgentProbeResult {
        request_id: Uuid,
        result: AcpProbeResult,
    },
    RefreshAcpRegistry {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        registry_url: Option<String>,
    },

    // Bidirectional Git Worktree <-> Local Handoff (Preview & Atomic Execution)
    PreviewWorktreeHandoff {
        request_id: Uuid,
        workspace_id: String,
        direction: WorktreeHandoffDirection,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_workspace_id: Option<String>,
        #[serde(default = "wire_default_true")]
        include_committed: bool,
    },
    WorktreeHandoffPreviewResult {
        request_id: Uuid,
        preview: WorktreeHandoffPreview,
    },
    ExecuteWorktreeHandoff {
        request_id: Uuid,
        workspace_id: String,
        direction: WorktreeHandoffDirection,
        mode: WorktreeHandoffMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_workspace_id: Option<String>,
        #[serde(default = "wire_default_true")]
        include_committed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        #[serde(default)]
        allow_three_way_conflicts: bool,
    },
    WorktreeHandoffExecuted {
        request_id: Uuid,
        outcome: WorktreeHandoffOutcome,
    },

    // First-Class Artifacts & SEP-1865 MCP Apps
    ListArtifacts {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
    },
    ArtifactsSnapshot {
        request_id: Uuid,
        artifacts: Vec<ArtifactSummary>,
    },
    GetArtifact {
        request_id: Uuid,
        artifact_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<u32>,
    },
    ArtifactContent {
        request_id: Uuid,
        artifact: ArtifactDetail,
    },
    UpsertArtifact {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artifact_id: Option<String>,
        workspace_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        title: String,
        kind: ArtifactKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resource_uri: Option<String>,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary_note: Option<String>,
    },
    ArtifactUpdated {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<Uuid>,
        artifact: ArtifactSummary,
    },
    DeleteArtifact {
        request_id: Uuid,
        artifact_id: String,
    },
    ArtifactDeleted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<Uuid>,
        artifact_id: String,
    },

    // Project Instruction Hierarchy Discovery & Atomic Editing
    ListInstructionFiles {
        request_id: Uuid,
        cwd: String,
    },
    InstructionFilesSnapshot {
        request_id: Uuid,
        cwd: String,
        files: Vec<InstructionFileEntry>,
        #[serde(default)]
        total_tokens: u32,
        #[serde(default)]
        merged_preview: String,
    },
    ReadInstructionFile {
        request_id: Uuid,
        cwd: String,
        path: String,
    },
    InstructionFileContentResponse {
        request_id: Uuid,
        cwd: String,
        path: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        etag: Option<String>,
        exists: bool,
    },
    SaveInstructionFile {
        request_id: Uuid,
        cwd: String,
        path: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_etag: Option<String>,
    },
    InstructionFileSavedResponse {
        request_id: Uuid,
        cwd: String,
        path: String,
        etag: String,
        size_bytes: u64,
        approx_tokens: u32,
    },

    // External MCP Server Manager (stdio + Streamable HTTP/SSE + live probe)
    ListExternalMcpServers {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    ExternalMcpServersSnapshot {
        request_id: Uuid,
        servers: Vec<ExternalMcpServerRecord>,
    },
    UpsertExternalMcpServer {
        request_id: Uuid,
        scope: McpWriteScope,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        name: String,
        transport: McpTransportKind,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        keep_existing_secrets: bool,
        #[serde(default = "wire_default_true")]
        enabled: bool,
    },
    ToggleExternalMcpServer {
        request_id: Uuid,
        server_id: String,
        enabled: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    DeleteExternalMcpServer {
        request_id: Uuid,
        server_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    ProbeExternalMcpServer {
        request_id: Uuid,
        server_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    ExternalMcpProbeResponse {
        request_id: Uuid,
        server_id: String,
        result: McpProbeResult,
    },

    // Path-Scoped Glob Rules (.orbit/rules/*.md, .claude/rules/*.md, .cursor/rules/*.mdc)
    ListPathRules {
        request_id: Uuid,
        cwd: String,
    },
    PathRulesSnapshot {
        request_id: Uuid,
        cwd: String,
        rules: Vec<PathScopedRule>,
        #[serde(default)]
        total_always_tokens: u32,
        #[serde(default)]
        token_budget_warning: bool,
    },
    UpsertPathRule {
        request_id: Uuid,
        cwd: String,
        scope: RuleScope,
        name: String,
        #[serde(default)]
        description: String,
        trigger_mode: RuleTriggerMode,
        #[serde(default)]
        globs: Vec<String>,
        #[serde(default = "wire_default_true")]
        enabled: bool,
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_etag: Option<String>,
    },
    DeletePathRule {
        request_id: Uuid,
        cwd: String,
        path: String,
    },
    MatchPathRules {
        request_id: Uuid,
        cwd: String,
        #[serde(default)]
        active_paths: Vec<String>,
        #[serde(default)]
        manual_rule_names: Vec<String>,
    },
    MatchedPathRulesResponse {
        request_id: Uuid,
        cwd: String,
        #[serde(default)]
        matched: Vec<MatchedRuleSummary>,
        #[serde(default)]
        injected_markdown: String,
        #[serde(default)]
        total_tokens: u32,
    },

    // Pure-Rust LSP Language Server Integration
    ListLspServers {
        request_id: Uuid,
        cwd: String,
    },
    LspServersSnapshot {
        request_id: Uuid,
        cwd: String,
        servers: Vec<LspServerStatus>,
    },
    LspQuerySymbols {
        request_id: Uuid,
        cwd: String,
        query: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default = "wire_default_lsp_limit")]
        limit: u32,
    },
    LspSymbolsResponse {
        request_id: Uuid,
        cwd: String,
        symbols: Vec<LspSymbolInfo>,
    },
    LspGetDiagnostics {
        request_id: Uuid,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    LspDiagnosticsResponse {
        request_id: Uuid,
        cwd: String,
        diagnostics: Vec<LspDiagnostic>,
    },
    LspGotoDefinition {
        request_id: Uuid,
        cwd: String,
        path: String,
        line: u32,
        col: u32,
    },
    LspLocationsResponse {
        request_id: Uuid,
        cwd: String,
        locations: Vec<LspLocation>,
    },
    LspHover {
        request_id: Uuid,
        cwd: String,
        path: String,
        line: u32,
        col: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    LspHoverResponse {
        request_id: Uuid,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hover: Option<LspHoverInfo>,
    },
    LspReferences {
        request_id: Uuid,
        cwd: String,
        path: String,
        line: u32,
        col: u32,
        #[serde(default = "wire_default_true")]
        include_declaration: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    LspReferencesResponse {
        request_id: Uuid,
        cwd: String,
        references: Vec<LspReferenceLocation>,
    },
    LspCompletion {
        request_id: Uuid,
        cwd: String,
        path: String,
        line: u32,
        col: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trigger_character: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    LspCompletionResponse {
        request_id: Uuid,
        cwd: String,
        items: Vec<LspCompletionItem>,
    },
    LspCodeActionRequest {
        request_id: Uuid,
        cwd: String,
        path: String,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
        #[serde(default)]
        diagnostics: Vec<LspDiagnostic>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    LspCodeActionResponse {
        request_id: Uuid,
        cwd: String,
        actions: Vec<LspCodeAction>,
    },
    LspRename {
        request_id: Uuid,
        cwd: String,
        path: String,
        line: u32,
        col: u32,
        new_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    LspApplyWorkspaceEdit {
        request_id: Uuid,
        cwd: String,
        edit: LspWorkspaceEdit,
    },
    LspWorkspaceEditResponse {
        request_id: Uuid,
        cwd: String,
        result: LspWorkspaceEditResult,
    },
    RestartLspServer {
        request_id: Uuid,
        cwd: String,
        server_id: String,
    },

    // Usage Insights, Cost Ledger & Context Occupancy Telemetry
    GetUsageInsights {
        request_id: Uuid,
        #[serde(default = "wire_default_usage_days")]
        days: u32,
    },
    UsageInsightsResponse {
        request_id: Uuid,
        snapshot: UsageInsightsSnapshot,
    },
    GetContextOccupancy {
        request_id: Uuid,
        agent_id: String,
    },
    ContextOccupancyResponse {
        request_id: Uuid,
        breakdown: ContextOccupancyBreakdown,
    },

    // Deterministic Lifecycle Hooks (`$ORBIT_HOME/hooks.json` & `<cwd>/.orbit/hooks.json`)
    ListLifecycleHooks {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    LifecycleHooksSnapshot {
        request_id: Uuid,
        hooks: Vec<LifecycleHookRule>,
        #[serde(default)]
        recent_executions: Vec<HookExecutionRecord>,
        #[serde(default)]
        project_hooks_trusted: bool,
    },
    UpsertLifecycleHook {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        hook: LifecycleHookRule,
    },
    DeleteLifecycleHook {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        hook_id: String,
        scope: HookScope,
    },
    SetProjectHooksTrust {
        request_id: Uuid,
        cwd: String,
        trusted: bool,
    },
    TestLifecycleHook {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
        hook_id: String,
        #[serde(default)]
        sample_payload: serde_json::Value,
    },
    HookTestResultResponse {
        request_id: Uuid,
        record: HookExecutionRecord,
    },

    // Early-Wakeup One-Shot Conditional Timers
    CreateConditionalTimer {
        request_id: Uuid,
        agent_id: String,
        duration_seconds: u64,
        prompt: String,
        #[serde(default)]
        condition: TimerCondition,
    },
    ConditionalTimerCreated {
        request_id: Uuid,
        timer: ConditionalTimerRecord,
    },
    ListConditionalTimers {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        #[serde(default)]
        include_resolved: bool,
    },
    ConditionalTimersSnapshot {
        request_id: Uuid,
        timers: Vec<ConditionalTimerRecord>,
    },
    CancelConditionalTimer {
        request_id: Uuid,
        timer_id: String,
    },
    FireConditionalTimerNow {
        request_id: Uuid,
        timer_id: String,
    },
    ConditionalTimerUpdated {
        request_id: Uuid,
        timer: ConditionalTimerRecord,
    },
    ConditionalTimerEvent {
        timer: ConditionalTimerRecord,
    },

    // Guardian AI Risk-Assessment Auto-Approval Policy & Audit Log
    GetGuardianConfig {
        request_id: Uuid,
    },
    UpdateGuardianConfig {
        request_id: Uuid,
        config: GuardianPolicyConfig,
    },
    GuardianConfigSnapshot {
        request_id: Uuid,
        config: GuardianPolicyConfig,
        #[serde(default)]
        recent_audits: Vec<GuardianAuditEntry>,
    },

    // --- Unattended Autonomous Goal Mode & Streaming Monitor Engine ---
    StartGoalRun {
        request_id: Uuid,
        agent_id: String,
        objective: String,
        #[serde(default)]
        criteria: Vec<String>,
        #[serde(default)]
        verify_command: Option<String>,
        #[serde(default)]
        max_iterations: Option<u32>,
        #[serde(default)]
        max_duration_secs: Option<u64>,
    },
    ListGoalRuns {
        request_id: Uuid,
        #[serde(default)]
        agent_id: Option<String>,
    },
    ControlGoalRun {
        request_id: Uuid,
        goal_id: String,
        action: GoalControlAction,
    },
    GoalRunUpdated {
        request_id: Uuid,
        goal: GoalRunRecord,
    },
    GoalRunsSnapshot {
        request_id: Uuid,
        goals: Vec<GoalRunRecord>,
    },
    GoalRunEvent {
        goal: GoalRunRecord,
    },
    StartMonitorWatch {
        request_id: Uuid,
        agent_id: String,
        name: String,
        kind: MonitorKind,
        #[serde(default)]
        expires_in_secs: Option<u64>,
    },
    ListMonitorWatches {
        request_id: Uuid,
        #[serde(default)]
        agent_id: Option<String>,
    },
    StopMonitorWatch {
        request_id: Uuid,
        monitor_id: String,
    },
    MonitorWatchUpdated {
        request_id: Uuid,
        monitor: MonitorWatchRecord,
    },
    MonitorWatchesSnapshot {
        request_id: Uuid,
        monitors: Vec<MonitorWatchRecord>,
    },
    MonitorWatchEvent {
        monitor: MonitorWatchRecord,
    },

    // --- External Event Triggers (GitHub, Slack, Linear, Generic Webhook + GitHub PR Polling) ---
    ListEventTriggers {
        request_id: Uuid,
    },
    EventTriggersSnapshot {
        request_id: Uuid,
        triggers: Vec<EventTriggerRecord>,
        #[serde(default)]
        recent_deliveries: Vec<EventTriggerDelivery>,
        #[serde(default)]
        base_webhook_url: String,
    },
    UpsertEventTrigger {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trigger_id: Option<String>,
        name: String,
        source: TriggerSourceKind,
        #[serde(default)]
        event_filters: Vec<String>,
        prompt_template: String,
        target: TriggerDispatchTarget,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        secret: Option<String>,
        #[serde(default)]
        keep_existing_secret: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        github_poll: Option<GitHubPollConfig>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        github_token: Option<String>,
        #[serde(default)]
        keep_existing_token: bool,
        enabled: bool,
    },
    DeleteEventTrigger {
        request_id: Uuid,
        trigger_id: String,
    },
    TestEventTrigger {
        request_id: Uuid,
        trigger_id: String,
        sample_event_type: String,
        #[serde(default)]
        sample_payload: serde_json::Value,
    },
    EventTriggerTestResponse {
        request_id: Uuid,
        delivery: EventTriggerDelivery,
    },

    // --- Pure-Rust CDP Browser Automation & Guarded Computer Use ---
    ListBrowserSessions {
        request_id: Uuid,
        #[serde(default = "wire_default_true")]
        include_screenshots: bool,
    },
    BrowserSessionsSnapshot {
        request_id: Uuid,
        sessions: Vec<BrowserSessionSummary>,
        policy: BrowserPolicyConfig,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chrome_binary: Option<String>,
    },
    CreateBrowserSession {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial_url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headless: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        viewport_width: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        viewport_height: Option<u32>,
    },
    BrowserSessionUpdated {
        request_id: Uuid,
        session: BrowserSessionSummary,
    },
    ExecuteBrowserAction {
        request_id: Uuid,
        session_id: String,
        action: BrowserActionRequest,
    },
    BrowserActionResponse {
        request_id: Uuid,
        session: BrowserSessionSummary,
        #[serde(default)]
        elements: Vec<BrowserElementRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        screenshot_base64: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_excerpt: Option<String>,
        #[serde(default)]
        captcha_detected: bool,
    },
    CloseBrowserSession {
        request_id: Uuid,
        session_id: String,
    },
    UpdateBrowserPolicy {
        request_id: Uuid,
        policy: BrowserPolicyConfig,
    },

    // --- Batch CSV Fan-Out, Programmable DAG Workflows & Best-of-N Parallel Arena ---
    StartBatchFanout {
        request_id: Uuid,
        name: String,
        cwd: String,
        csv_content: String,
        prompt_template: String,
        provider: ProviderKind,
        #[serde(default)]
        model: Option<String>,
        mode: SessionMode,
        #[serde(default = "wire_default_workflow_concurrency")]
        max_concurrency: u32,
        #[serde(default)]
        isolated_worktree: bool,
    },
    StartDagWorkflow {
        request_id: Uuid,
        name: String,
        cwd: String,
        steps: Vec<DagStepSpec>,
        default_provider: ProviderKind,
        default_mode: SessionMode,
        #[serde(default = "wire_default_workflow_concurrency")]
        max_concurrency: u32,
    },
    StartBestOfN {
        request_id: Uuid,
        name: String,
        cwd: String,
        prompt: String,
        candidates: Vec<BestOfNCandidateSpec>,
        mode: SessionMode,
        #[serde(default)]
        verify_command: Option<String>,
    },
    ListWorkflowRuns {
        request_id: Uuid,
        #[serde(default)]
        include_completed: bool,
    },
    CancelWorkflowRun {
        request_id: Uuid,
        run_id: String,
    },
    SelectBestOfNWinner {
        request_id: Uuid,
        run_id: String,
        winner_step_id: String,
        #[serde(default)]
        apply_to_workspace: bool,
    },
    WorkflowRunUpdated {
        request_id: Uuid,
        run: WorkflowRunRecord,
    },
    WorkflowRunsSnapshot {
        request_id: Uuid,
        runs: Vec<WorkflowRunRecord>,
    },
    WorkflowRunEvent {
        run: WorkflowRunRecord,
    },

    // Bidirectional IM Channels (Lark / Feishu, Telegram, Slack, Discord + Remote Approvals)
    ListImChannels {
        request_id: Uuid,
    },
    ImChannelsSnapshot {
        request_id: Uuid,
        channels: Vec<ImChannelRecord>,
        bindings: Vec<ImThreadBinding>,
        base_webhook_url: String,
    },
    UpsertImChannel {
        request_id: Uuid,
        #[serde(default)]
        channel_id: Option<String>,
        name: String,
        platform: ImPlatform,
        enabled: bool,
        #[serde(default)]
        transport_mode: ImTransportMode,
        #[serde(default)]
        app_id: Option<String>,
        #[serde(default)]
        primary_secret: Option<String>,
        #[serde(default)]
        signing_secret: Option<String>,
        #[serde(default)]
        app_token: Option<String>,
        #[serde(default)]
        public_key: Option<String>,
        #[serde(default = "wire_default_true")]
        keep_existing_secrets: bool,
        #[serde(default)]
        allowed_chat_ids: Vec<String>,
        #[serde(default)]
        allowed_user_ids: Vec<String>,
        default_workspace_cwd: String,
        default_provider: ProviderKind,
        default_mode: SessionMode,
        #[serde(default = "wire_default_true")]
        forward_approvals: bool,
        #[serde(default)]
        routing_mode: ImRoutingMode,
        #[serde(default)]
        router_mirror_policy: RouterImMirrorPolicy,
    },
    DeleteImChannel {
        request_id: Uuid,
        channel_id: String,
    },
    TestImChannel {
        request_id: Uuid,
        channel_id: String,
        target_chat_id: String,
        message: String,
    },
    ImChannelTestResponse {
        request_id: Uuid,
        ok: bool,
        detail: String,
    },

    // Enhanced Desktop Notifications & Native Mobile Background Push (APNs / FCM / HarmonyOS Push Kit)
    RegisterPushDevice {
        request_id: Uuid,
        device_id: String,
        platform: PushPlatform,
        device_name: String,
        device_token: String,
        #[serde(default)]
        sandbox: bool,
    },
    UnregisterPushDevice {
        request_id: Uuid,
        device_id: String,
    },
    ListPushDevices {
        request_id: Uuid,
    },
    UpdatePushPreferences {
        request_id: Uuid,
        preferences: PushPreferences,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gateway_auth_token: Option<String>,
        #[serde(default = "wire_default_true")]
        keep_existing_gateway_token: bool,
    },
    PushDevicesSnapshot {
        request_id: Uuid,
        devices: Vec<PushDeviceRecord>,
        preferences: PushPreferences,
    },
    TestPushNotification {
        request_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
    },
    PushTestResponse {
        request_id: Uuid,
        attempted: u32,
        delivered: u32,
        detail: String,
    },

    /// Request an online bridge Daemon to open an E2EE tunnel to another registered Connected Host
    /// reachable from that bridge (including LAN-only or SSH-tunneled nodes).
    PeerBridgeOpen {
        request_id: Uuid,
        bridge_id: Uuid,
        target_host_id: String,
    },
    /// Bridge Daemon opened the upstream `/ws/e2ee` stream and is ready to forward ciphertext.
    PeerBridgeOpened {
        request_id: Uuid,
        bridge_id: Uuid,
    },
    /// Opaque Noise_NK E2EE ciphertext frame forwarded across a Peer Bridge.
    PeerBridgeData {
        bridge_id: Uuid,
        payload_b64: String,
    },
    /// Either side closed the Peer Bridge stream cleanly or due to an upstream error.
    PeerBridgeClosed {
        bridge_id: Uuid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// Notify other sessions on the same account that Cloud Sync Vault state changed.
    NotifyFleetVaultChanged {
        request_id: Uuid,
        vault_fingerprint: String,
        revision: u64,
    },
    /// Daemon broadcast pushed to other connected clients when another device updates the vault.
    FleetVaultChanged {
        vault_fingerprint: String,
        revision: u64,
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

/// Anti-bomb upper bound on a single decompressed Zstd `WireMessage` payload (40 MiB,
/// accommodating up to 25 MiB base64-encoded media responses plus JSON envelope).
pub const MAX_ZSTD_WIRE_DECODE_BYTES: usize = 40 * 1024 * 1024;
/// WebSocket frame/message size cap shared across daemon, relay, client, and relay-transport
/// endpoints (48 MiB, safely carrying 25 MiB base64/E2EE frames without tearing down sessions).
pub const MAX_WS_FRAME_BYTES: usize = 48 * 1024 * 1024;
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
    fn timeline_catchup_accepts_legacy_and_omits_empty_outlines() {
        let legacy = serde_json::json!({
            "op": "timeline_catchup", "request_id": Uuid::nil(),
            "agent_id": "agent", "events": [], "has_more": false
        });
        let wire: WireMessage = serde_json::from_value(legacy.clone()).expect("legacy catchup");
        assert!(
            matches!(&wire, WireMessage::TimelineCatchup { outline_prompts, .. } if outline_prompts.is_empty())
        );
        assert!(
            serde_json::to_value(wire)
                .expect("wire JSON")
                .get("outline_prompts")
                .is_none()
        );

        let mut modern = legacy;
        modern["outline_prompts"] = serde_json::json!([{ "seq": 7, "preview": "before the tail" }]);
        let wire: WireMessage = serde_json::from_value(modern.clone()).expect("modern catchup");
        assert_eq!(
            serde_json::to_value(wire).expect("wire JSON")["outline_prompts"],
            modern["outline_prompts"]
        );
    }

    #[test]
    fn outline_previews_unwrap_wire_sections_and_bound_unicode() {
        let body = "界🙂".repeat(100);
        let raw = format!(
            "<orbit-context>hidden</orbit-context><orbit-workspace id=\"w\">hidden</orbit-workspace>\n<WORKFLOW_INSTRUCTION command=\"/plan\">hidden</WORKFLOW_INSTRUCTION><ATTACHED_CONTEXT>hidden</ATTACHED_CONTEXT>\n<USER_REQUEST>  first\n\t{body}  </USER_REQUEST>"
        );
        let prompt = TimelineOutlinePrompt::from_user_prompt(7, &raw, &[]).expect("anchor");
        assert_eq!(prompt.preview, format!("first {}", "界🙂".repeat(52)));
        let badge = PromptBadge {
            kind: "file".into(),
            label: "file name".into(),
            path: None,
        };
        assert_eq!(
            TimelineOutlinePrompt::from_user_prompt(9, "", &[badge])
                .expect("badge")
                .preview,
            "file name"
        );
        for command in [
            "/COMPACT",
            "<orbit-workspace>hidden</orbit-workspace> /compact",
            "<WORKFLOW_INSTRUCTION command=\"/compact\">hidden</WORKFLOW_INSTRUCTION>",
        ] {
            assert!(TimelineOutlinePrompt::from_user_prompt(1, command, &[]).is_none());
        }
        let literal = "/compaction and <orbit-context>literal</orbit-context>";
        assert_eq!(
            TimelineOutlinePrompt::from_user_prompt(1, literal, &[])
                .expect("literal")
                .preview,
            literal
        );
    }

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
            acp_agent_id: None,
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
    fn workspace_symbols_wire_roundtrip() {
        let req_id = Uuid::new_v4();
        let search_req = WireMessage::SearchWorkspaceSymbols {
            request_id: req_id,
            cwd: "/workspace/orbit".into(),
            query: "searchSym".into(),
            limit: 25,
        };
        let encoded = search_req.encode_zstd(3).expect("encode symbol search");
        match WireMessage::decode_zstd(&encoded).expect("decode symbol search") {
            WireMessage::SearchWorkspaceSymbols {
                request_id,
                cwd,
                query,
                limit,
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(cwd, "/workspace/orbit");
                assert_eq!(query, "searchSym");
                assert_eq!(limit, 25);
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        // Backward-compatible default limit when omitted
        let minimal_req = serde_json::json!({
            "op": "search_workspace_symbols",
            "request_id": req_id,
            "cwd": "/workspace/orbit",
            "query": "Foo"
        });
        match serde_json::from_value::<WireMessage>(minimal_req).expect("decode default limit") {
            WireMessage::SearchWorkspaceSymbols { limit, .. } => assert_eq!(limit, 30),
            other => panic!("unexpected variant: {other:?}"),
        }

        let resp = WireMessage::WorkspaceSymbolsResponse {
            request_id: req_id,
            cwd: "/workspace/orbit".into(),
            symbols: vec![WorkspaceSymbolEntry {
                name: "search_workspace_symbols".into(),
                kind: "fn".into(),
                path: "crates/orbit-daemon/src/symbols.rs".into(),
                start_line: 42,
                end_line: 78,
                signature: Some("pub async fn search_workspace_symbols(...)".into()),
                container: Some("WorkspaceSymbolIndex".into()),
                language: "rust".into(),
            }],
            truncated: true,
        };
        let z_resp = resp.encode_zstd(3).expect("encode symbol response");
        match WireMessage::decode_zstd(&z_resp).expect("decode symbol response") {
            WireMessage::WorkspaceSymbolsResponse {
                request_id,
                cwd,
                symbols,
                truncated,
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(cwd, "/workspace/orbit");
                assert!(truncated);
                assert_eq!(symbols.len(), 1);
                assert_eq!(symbols[0].name, "search_workspace_symbols");
                assert_eq!(
                    symbols[0].container.as_deref(),
                    Some("WorkspaceSymbolIndex")
                );
                assert_eq!(symbols[0].start_line, 42);
                assert_eq!(symbols[0].end_line, 78);
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn search_messages_rpc_wire_roundtrip() {
        let req_id = Uuid::new_v4();
        let scoped_req = WireMessage::SearchMessages {
            request_id: req_id,
            query: "数据库连接 timeout".into(),
            agent_id: Some("ag_42".into()),
            limit: Some(50),
        };
        let encoded = scoped_req.encode_zstd(3).expect("encode search messages");
        match WireMessage::decode_zstd(&encoded).expect("decode search messages") {
            WireMessage::SearchMessages {
                request_id,
                query,
                agent_id,
                limit,
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(query, "数据库连接 timeout");
                assert_eq!(agent_id.as_deref(), Some("ag_42"));
                assert_eq!(limit, Some(50));
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        // Minimal request omitting optional agent_id / limit
        let minimal_req = serde_json::json!({
            "op": "search_messages",
            "request_id": req_id,
            "query": "rust fts5"
        });
        match serde_json::from_value::<WireMessage>(minimal_req).expect("decode minimal search") {
            WireMessage::SearchMessages {
                agent_id, limit, ..
            } => {
                assert_eq!(agent_id, None);
                assert_eq!(limit, None);
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let resp = WireMessage::SearchMessagesResult {
            request_id: req_id,
            hits: vec![RemoteMessageSearchHit {
                agent_id: "ag_42".into(),
                seq: 17,
                turn_id: Some("turn_3".into()),
                role: "assistant".into(),
                snippet: "Fixed <mark>数据库连接</mark> <mark>timeout</mark>".into(),
                timestamp: Some("2026-11-24T12:00:00Z".into()),
                bm25_score: -4.25,
                agent_title: "DB Pool Hardening".into(),
                cwd: "/workspace/orbit".into(),
            }],
        };
        let z_resp = resp.encode_zstd(3).expect("encode search result");
        match WireMessage::decode_zstd(&z_resp).expect("decode search result") {
            WireMessage::SearchMessagesResult { request_id, hits } => {
                assert_eq!(request_id, req_id);
                assert_eq!(hits.len(), 1);
                assert_eq!(hits[0].agent_id, "ag_42");
                assert_eq!(hits[0].seq, 17);
                assert_eq!(hits[0].turn_id.as_deref(), Some("turn_3"));
                assert_eq!(hits[0].role, "assistant");
                assert!((hits[0].bm25_score - (-4.25)).abs() < 1e-6);
                assert_eq!(hits[0].agent_title, "DB Pool Hardening");
                assert_eq!(hits[0].cwd, "/workspace/orbit");
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn workspace_file_mutations_and_legacy_content_compat_roundtrip() {
        let req_id = Uuid::new_v4();

        // 1. Legacy WorkspaceFileContentResponse JSON without new metadata fields must deserialize cleanly
        let legacy_json = serde_json::json!({
            "op": "workspace_file_content_response",
            "request_id": req_id,
            "cwd": "/workspace/orbit",
            "path": "src/main.rs",
            "resolved_path": "/workspace/orbit/src/main.rs",
            "content_kind": "text",
            "mime_type": "text/plain",
            "content": "fn main() {}\n",
            "size_bytes": 13,
            "truncated": false
        });
        let decoded_legacy: WireMessage =
            serde_json::from_value(legacy_json).expect("decode legacy file content");
        match decoded_legacy {
            WireMessage::WorkspaceFileContentResponse {
                request_id,
                etag,
                mtime_unix_ms,
                writable,
                line_ending,
                ..
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(etag, None);
                assert_eq!(mtime_unix_ms, None);
                assert!(!writable);
                assert_eq!(line_ending, None);
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        // 2. Enriched WorkspaceFileContentResponse roundtrip
        let enriched = WireMessage::WorkspaceFileContentResponse {
            request_id: req_id,
            cwd: "/workspace/orbit".into(),
            path: "src/main.rs".into(),
            resolved_path: "/workspace/orbit/src/main.rs".into(),
            content_kind: "text".into(),
            mime_type: Some("text/plain".into()),
            content: "fn main() {}\n".into(),
            size_bytes: 13,
            truncated: false,
            etag: Some("a".repeat(64)),
            mtime_unix_ms: Some(1_730_000_000_000),
            writable: true,
            line_ending: Some("lf".into()),
        };
        let z = enriched.encode_zstd(3).expect("encode enriched");
        match WireMessage::decode_zstd(&z).expect("decode enriched") {
            WireMessage::WorkspaceFileContentResponse {
                etag,
                mtime_unix_ms,
                writable,
                line_ending,
                ..
            } => {
                assert_eq!(etag.as_deref(), Some("a".repeat(64).as_str()));
                assert_eq!(mtime_unix_ms, Some(1_730_000_000_000));
                assert!(writable);
                assert_eq!(line_ending.as_deref(), Some("lf"));
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        // 3. WriteWorkspaceFile & WorkspaceFileWritten (with conflict)
        let write_req = WireMessage::WriteWorkspaceFile {
            request_id: req_id,
            cwd: "/workspace/orbit".into(),
            path: "src/lib.rs".into(),
            content: "pub fn hi() {}\n".into(),
            expected_etag: Some("deadbeef".into()),
            force: false,
        };
        let z_write = write_req.encode_zstd(3).expect("encode write");
        match WireMessage::decode_zstd(&z_write).expect("decode write") {
            WireMessage::WriteWorkspaceFile {
                request_id,
                path,
                expected_etag,
                force,
                ..
            } => {
                assert_eq!(request_id, req_id);
                assert_eq!(path, "src/lib.rs");
                assert_eq!(expected_etag.as_deref(), Some("deadbeef"));
                assert!(!force);
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        let written_conflict = WireMessage::WorkspaceFileWritten {
            request_id: req_id,
            cwd: "/workspace/orbit".into(),
            path: "src/lib.rs".into(),
            etag: "newetag".into(),
            size_bytes: 20,
            mtime_unix_ms: Some(1_730_000_000_123),
            conflict: Some(WorkspaceFileConflict {
                current_etag: "newetag".into(),
                current_content: Some("pub fn disk() {}\n".into()),
                current_size_bytes: 17,
                current_mtime_unix_ms: Some(1_730_000_000_123),
            }),
        };
        let z_written = written_conflict.encode_zstd(3).expect("encode written");
        match WireMessage::decode_zstd(&z_written).expect("decode written") {
            WireMessage::WorkspaceFileWritten {
                conflict: Some(c), ..
            } => {
                assert_eq!(c.current_etag, "newetag");
                assert_eq!(c.current_content.as_deref(), Some("pub fn disk() {}\n"));
                assert_eq!(c.current_size_bytes, 17);
            }
            other => panic!("unexpected variant: {other:?}"),
        }

        // 4. Create / Rename / Delete / Mutated roundtrip
        for msg in [
            WireMessage::CreateWorkspaceEntry {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                path: "src/new.rs".into(),
                is_dir: false,
                initial_content: Some("// hi\n".into()),
            },
            WireMessage::RenameWorkspaceEntry {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                old_path: "src/new.rs".into(),
                new_path: "src/renamed.rs".into(),
            },
            WireMessage::DeleteWorkspaceEntry {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                path: "src/renamed.rs".into(),
            },
            WireMessage::WorkspaceEntryMutated {
                request_id: req_id,
                cwd: "/workspace/orbit".into(),
                path: "src/new.rs".into(),
                new_path: Some("src/renamed.rs".into()),
                action: "renamed".into(),
            },
        ] {
            let enc = msg.encode_zstd(3).expect("encode mutation msg");
            let _ = WireMessage::decode_zstd(&enc).expect("decode mutation msg");
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
                acp_agent_id: None,
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
    fn provider_kind_all_14_metadata_slugs_and_serde_roundtrip() {
        assert_eq!(ProviderKind::native_all().len(), 13);
        assert_eq!(ProviderKind::all().len(), 14);
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
            ("acp", ProviderKind::Acp),
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
                acp_agent_id: None,
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

    #[test]
    fn acp_wire_messages_roundtrip() {
        let req_id = Uuid::new_v4();
        let spec = AcpAgentSpec {
            id: "opencode".into(),
            name: "OpenCode".into(),
            description: "Open-source terminal coding agent".into(),
            command: "opencode".into(),
            args: vec!["acp".into()],
            env: HashMap::from([("OPENCODE_MODE".into(), "acp".into())]),
            install_hint: Some("brew install opencode".into()),
            homepage: Some("https://opencode.ai".into()),
            version: Some("1.0.0".into()),
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        let messages = vec![
            WireMessage::ListAcpAgents { request_id: req_id },
            WireMessage::AcpAgentsSnapshot {
                request_id: req_id,
                agents: vec![AcpAgentAvailability {
                    spec: spec.clone(),
                    installed: true,
                    resolved_path: Some("/usr/local/bin/opencode".into()),
                }],
                warning: Some("offline fallback".into()),
            },
            WireMessage::UpsertCustomAcpAgent {
                request_id: req_id,
                spec: spec.clone(),
            },
            WireMessage::DeleteCustomAcpAgent {
                request_id: req_id,
                agent_id: "opencode".into(),
            },
            WireMessage::TestAcpAgent {
                request_id: req_id,
                agent_id: "opencode".into(),
                cwd: Some("/workspace/orbit".into()),
            },
            WireMessage::AcpAgentProbeResult {
                request_id: req_id,
                result: AcpProbeResult {
                    agent_id: "opencode".into(),
                    ok: true,
                    protocol_version: Some("1".into()),
                    server_name: Some("opencode".into()),
                    server_version: Some("1.2.3".into()),
                    capabilities_summary: vec!["loadSession".into(), "mcp.http".into()],
                    auth_methods: vec!["oauth".into()],
                    latency_ms: 42,
                    error: None,
                },
            },
            WireMessage::RefreshAcpRegistry {
                request_id: req_id,
                registry_url: Some(
                    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json".into(),
                ),
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode acp wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode acp wire msg");
            let orig_json = serde_json::to_value(&msg).expect("orig json");
            let dec_json = serde_json::to_value(&decoded).expect("dec json");
            assert_eq!(orig_json, dec_json);
        }

        // Verify ProviderKind::Acp serde & display
        assert_eq!(
            serde_json::to_string(&ProviderKind::Acp).expect("serialize provider"),
            "\"acp\""
        );
        assert_eq!(ProviderKind::Acp.display_name(), "ACP Agent");
    }

    #[test]
    fn worktree_handoff_wire_roundtrip() {
        let req_id = Uuid::new_v4();
        let messages = vec![
            WireMessage::PreviewWorktreeHandoff {
                request_id: req_id,
                workspace_id: "ws_wt".into(),
                direction: WorktreeHandoffDirection::WorktreeToLocal,
                target_workspace_id: None,
                include_committed: true,
            },
            WireMessage::WorktreeHandoffPreviewResult {
                request_id: req_id,
                preview: WorktreeHandoffPreview {
                    direction: WorktreeHandoffDirection::WorktreeToLocal,
                    source_cwd: "/tmp/wt".into(),
                    target_cwd: "/tmp/local".into(),
                    source_branch: Some("feat/handoff".into()),
                    target_branch: Some("main".into()),
                    commit_count: 2,
                    files: vec![HandoffFileChange {
                        path: "src/lib.rs".into(),
                        status: "M".into(),
                        insertions: 12,
                        deletions: 3,
                        is_untracked: false,
                        has_conflict: false,
                    }],
                    total_insertions: 12,
                    total_deletions: 3,
                    target_dirty: false,
                    conflicts: Vec::new(),
                    can_apply_cleanly: true,
                    warning: None,
                },
            },
            WireMessage::ExecuteWorktreeHandoff {
                request_id: req_id,
                workspace_id: "ws_local".into(),
                direction: WorktreeHandoffDirection::LocalToWorktree,
                mode: WorktreeHandoffMode::Move,
                target_workspace_id: None,
                include_committed: false,
                branch: Some("orbit/handoff-1".into()),
                allow_three_way_conflicts: false,
            },
            WireMessage::WorktreeHandoffExecuted {
                request_id: req_id,
                outcome: WorktreeHandoffOutcome {
                    direction: WorktreeHandoffDirection::LocalToWorktree,
                    mode: WorktreeHandoffMode::Move,
                    applied_files: 3,
                    backup_ref: Some("refs/orbit/handoff-backup/123456-abcd1234".into()),
                    target_workspace: None,
                    target_cwd: "/tmp/wt-new".into(),
                    had_conflicts: false,
                    conflict_files: Vec::new(),
                    summary: "Moved 3 files to worktree `orbit/handoff-1`".into(),
                },
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode handoff wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode handoff wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn external_mcp_servers_wire_roundtrip() {
        let req_id = Uuid::new_v4();
        let probe = McpProbeResult {
            ok: true,
            server_name: Some("github-mcp".into()),
            server_version: Some("1.0.0".into()),
            protocol_version: Some("2025-03-26".into()),
            tools: vec![McpToolSummary {
                name: "create_issue".into(),
                description: "Create a GitHub issue".into(),
            }],
            latency_ms: 42,
            error: None,
            probed_at_ms: 1_760_000_000_000,
        };
        let record = ExternalMcpServerRecord {
            id: "orbit_user:github".into(),
            name: "github".into(),
            scope: McpServerScope::OrbitUser,
            transport: McpTransportKind::StreamableHttp {
                url: "https://mcp.example.com/mcp".into(),
                header_keys: vec!["Authorization".into()],
            },
            env_keys: vec!["GITHUB_TOKEN".into()],
            masked_env: BTreeMap::from([("GITHUB_TOKEN".into(), "••••1234".into())]),
            masked_headers: BTreeMap::from([("Authorization".into(), "••••5678".into())]),
            enabled: true,
            editable: true,
            source_path: "/home/user/.orbit/mcp-servers.json".into(),
            last_probe: Some(probe.clone()),
        };

        let messages = vec![
            WireMessage::ListExternalMcpServers {
                request_id: req_id,
                cwd: Some("/workspace/orbit".into()),
            },
            WireMessage::ExternalMcpServersSnapshot {
                request_id: req_id,
                servers: vec![record],
            },
            WireMessage::UpsertExternalMcpServer {
                request_id: req_id,
                scope: McpWriteScope::User,
                cwd: None,
                name: "github".into(),
                transport: McpTransportKind::Stdio {
                    command: "npx".into(),
                    args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
                },
                env: BTreeMap::from([("LOG_LEVEL".into(), "info".into())]),
                headers: BTreeMap::new(),
                keep_existing_secrets: true,
                enabled: true,
            },
            WireMessage::ToggleExternalMcpServer {
                request_id: req_id,
                server_id: "orbit_user:github".into(),
                enabled: false,
                cwd: None,
            },
            WireMessage::DeleteExternalMcpServer {
                request_id: req_id,
                server_id: "orbit_user:github".into(),
                cwd: None,
            },
            WireMessage::ProbeExternalMcpServer {
                request_id: req_id,
                server_id: "orbit_user:github".into(),
                cwd: Some("/workspace/orbit".into()),
            },
            WireMessage::ExternalMcpProbeResponse {
                request_id: req_id,
                server_id: "orbit_user:github".into(),
                result: probe,
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode external mcp wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode external mcp wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn usage_insights_and_context_occupancy_wire_roundtrip() {
        let req_id = Uuid::new_v4();
        let messages = vec![
            WireMessage::GetUsageInsights {
                request_id: req_id,
                days: 30,
            },
            WireMessage::UsageInsightsResponse {
                request_id: req_id,
                snapshot: UsageInsightsSnapshot {
                    days: 30,
                    total_cost_usd: 12.45,
                    total_cache_savings_usd: 4.18,
                    total_input_tokens: 1_250_000,
                    total_output_tokens: 185_000,
                    total_cache_read_tokens: 920_000,
                    total_turns: 42,
                    daily: vec![UsageDailyPoint {
                        date: "2026-11-22".into(),
                        input_tokens: 120_000,
                        output_tokens: 24_000,
                        cache_read_tokens: 80_000,
                        cost_usd: 1.42,
                        turns: 5,
                    }],
                    by_model: vec![UsageModelBreakdown {
                        provider: "claude_code".into(),
                        model: "claude-sonnet-4-5".into(),
                        input_tokens: 800_000,
                        output_tokens: 120_000,
                        cache_read_tokens: 600_000,
                        cost_usd: 4.38,
                        turns: 25,
                    }],
                    by_project: vec![UsageProjectBreakdown {
                        workspace_cwd: "/workspace/orbit".into(),
                        cost_usd: 12.45,
                        total_tokens: 2_355_000,
                        turns: 42,
                    }],
                    forecasts: vec![QuotaDepletionForecast {
                        provider: "claude_code".into(),
                        window_label: "5h window".into(),
                        remaining_percent: 42.0,
                        burn_rate_pct_per_hour: 12.5,
                        estimated_depletion_secs: Some(12_096),
                        resets_in_secs: Some(14_400),
                        likely_to_exhaust_before_reset: true,
                    }],
                },
            },
            WireMessage::GetContextOccupancy {
                request_id: req_id,
                agent_id: "ag_ctx".into(),
            },
            WireMessage::ContextOccupancyResponse {
                request_id: req_id,
                breakdown: ContextOccupancyBreakdown {
                    agent_id: "ag_ctx".into(),
                    model: "claude-opus-4-6".into(),
                    context_window_tokens: 200_000,
                    total_used_tokens: 68_000,
                    occupancy_percent: 34.0,
                    segments: vec![
                        ContextSegment {
                            category: "instructions".into(),
                            label: "Instructions".into(),
                            tokens: 4_000,
                            percent: 2.0,
                        },
                        ContextSegment {
                            category: "history".into(),
                            label: "History".into(),
                            tokens: 64_000,
                            percent: 32.0,
                        },
                        ContextSegment {
                            category: "free".into(),
                            label: "Free".into(),
                            tokens: 132_000,
                            percent: 66.0,
                        },
                    ],
                },
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode usage wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode usage wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn conditional_timers_wire_roundtrip() {
        let req_id = Uuid::new_v4();
        let record = ConditionalTimerRecord {
            timer_id: "tmr_abc12345".into(),
            agent_id: "ag_main".into(),
            workspace_id: "ws_1".into(),
            prompt: "Follow up on deployment".into(),
            duration_seconds: 300,
            condition: TimerCondition::FileChanged {
                path: "dist/bundle.js".into(),
            },
            state: ConditionalTimerState::Armed,
            created_at_ms: 1_760_000_000_000,
            fire_at_ms: 1_760_000_300_000,
            resolved_at_ms: None,
            resolved_reason: None,
        };
        let messages = vec![
            WireMessage::CreateConditionalTimer {
                request_id: req_id,
                agent_id: "ag_main".into(),
                duration_seconds: 300,
                prompt: "Follow up on deployment".into(),
                condition: TimerCondition::AnyMessage,
            },
            WireMessage::ConditionalTimerCreated {
                request_id: req_id,
                timer: record.clone(),
            },
            WireMessage::ListConditionalTimers {
                request_id: req_id,
                agent_id: Some("ag_main".into()),
                include_resolved: true,
            },
            WireMessage::ConditionalTimersSnapshot {
                request_id: req_id,
                timers: vec![record.clone()],
            },
            WireMessage::CancelConditionalTimer {
                request_id: req_id,
                timer_id: "tmr_abc12345".into(),
            },
            WireMessage::FireConditionalTimerNow {
                request_id: req_id,
                timer_id: "tmr_abc12345".into(),
            },
            WireMessage::ConditionalTimerUpdated {
                request_id: req_id,
                timer: record.clone(),
            },
            WireMessage::ConditionalTimerEvent { timer: record },
        ];
        for msg in messages {
            let encoded = msg
                .encode_zstd(3)
                .expect("encode conditional timer wire msg");
            let decoded =
                WireMessage::decode_zstd(&encoded).expect("decode conditional timer wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn guardian_session_mode_auto_and_wire_roundtrip() {
        let mode_json = serde_json::to_string(&SessionMode::Auto).expect("serialize auto mode");
        assert_eq!(mode_json, "\"auto\"");
        let parsed_mode: SessionMode =
            serde_json::from_str(&mode_json).expect("deserialize auto mode");
        assert_eq!(parsed_mode, SessionMode::Auto);

        let req_id = Uuid::new_v4();
        let cfg = GuardianPolicyConfig {
            enabled: true,
            auto_approve_max_risk: GuardianRiskLevel::Low,
            use_ai_classifier_for_medium: true,
            classifier_timeout_ms: 4_000,
            allow_read_only_tools: true,
            allow_workspace_tests: true,
            blocked_command_patterns: vec!["rm -rf".into()],
            trusted_command_prefixes: vec!["just test".into()],
        };
        let audit = GuardianAuditEntry {
            id: "ga_1".into(),
            agent_id: "ag_1".into(),
            request_id: "perm_1".into(),
            tool_name: "shell".into(),
            target_summary: "cargo test".into(),
            assessment: GuardianAssessment {
                risk_level: GuardianRiskLevel::Low,
                decision: GuardianDecision::AutoAllow,
                confidence: 0.99,
                classifier_tier: "deterministic".into(),
                reason: "Safe workspace test command".into(),
                signals: vec!["workspace_test".into()],
                latency_ms: 0,
            },
            decided_at_ms: 1_760_000_000_000,
        };
        let messages = vec![
            WireMessage::GetGuardianConfig { request_id: req_id },
            WireMessage::UpdateGuardianConfig {
                request_id: req_id,
                config: cfg.clone(),
            },
            WireMessage::GuardianConfigSnapshot {
                request_id: req_id,
                config: cfg,
                recent_audits: vec![audit],
            },
        ];
        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode guardian wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode guardian wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn goal_and_monitor_wire_messages_roundtrip() {
        let req_id = Uuid::new_v4();
        let goal = GoalRunRecord {
            goal_id: "goal_abc".into(),
            agent_id: "ag_1".into(),
            workspace_id: "ws_1".into(),
            objective: "Make tests green".into(),
            criteria: vec![GoalCriterion {
                id: "c1".into(),
                label: "cargo test exits 0".into(),
                passed: false,
            }],
            verify_command: Some("cargo test".into()),
            max_iterations: 10,
            current_iteration: 1,
            max_duration_secs: 3600,
            status: GoalStatus::Running,
            last_verify_exit_code: Some(1),
            last_verify_output: Some("1 failed".into()),
            consecutive_identical_failures: 1,
            started_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_005_000,
            finished_at_ms: None,
        };
        let monitor = MonitorWatchRecord {
            monitor_id: "mon_xyz".into(),
            agent_id: "ag_1".into(),
            name: "Health".into(),
            kind: MonitorKind::HttpHealth {
                url: "https://example.com/health".into(),
                expect_status: 200,
                interval_secs: 30,
            },
            status: MonitorStatus::Active,
            match_count: 0,
            last_event_preview: None,
            last_triggered_at_ms: None,
            expires_at_ms: Some(1_760_003_600_000),
            created_at_ms: 1_760_000_000_000,
        };

        let messages = vec![
            WireMessage::StartGoalRun {
                request_id: req_id,
                agent_id: "ag_1".into(),
                objective: "Make tests green".into(),
                criteria: vec!["cargo test exits 0".into()],
                verify_command: Some("cargo test".into()),
                max_iterations: Some(10),
                max_duration_secs: Some(3600),
            },
            WireMessage::ListGoalRuns {
                request_id: req_id,
                agent_id: Some("ag_1".into()),
            },
            WireMessage::ControlGoalRun {
                request_id: req_id,
                goal_id: "goal_abc".into(),
                action: GoalControlAction::VerifyNow,
            },
            WireMessage::GoalRunUpdated {
                request_id: req_id,
                goal: goal.clone(),
            },
            WireMessage::GoalRunsSnapshot {
                request_id: req_id,
                goals: vec![goal.clone()],
            },
            WireMessage::GoalRunEvent { goal },
            WireMessage::StartMonitorWatch {
                request_id: req_id,
                agent_id: "ag_1".into(),
                name: "Health".into(),
                kind: MonitorKind::LogFileTail {
                    path: "logs/app.log".into(),
                    pattern: "ERROR".into(),
                },
                expires_in_secs: Some(3600),
            },
            WireMessage::ListMonitorWatches {
                request_id: req_id,
                agent_id: None,
            },
            WireMessage::StopMonitorWatch {
                request_id: req_id,
                monitor_id: "mon_xyz".into(),
            },
            WireMessage::MonitorWatchUpdated {
                request_id: req_id,
                monitor: monitor.clone(),
            },
            WireMessage::MonitorWatchesSnapshot {
                request_id: req_id,
                monitors: vec![monitor.clone()],
            },
            WireMessage::MonitorWatchEvent { monitor },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode goal/monitor wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode goal/monitor wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn event_triggers_wire_messages_roundtrip() {
        let req_id = Uuid::new_v4();
        let poll = GitHubPollConfig {
            repo: "octocat/orbit".into(),
            poll_interval_secs: 120,
            watch_prs: true,
            watch_issues: true,
            watch_ci_failures: false,
            has_token: true,
        };
        let trigger = EventTriggerRecord {
            trigger_id: "trg_abc12345".into(),
            name: "PR Auto-Review".into(),
            source: TriggerSourceKind::GitHub,
            event_filters: vec![
                "pull_request.opened".into(),
                "pull_request.synchronize".into(),
            ],
            prompt_template: "Review {{title}} at {{url}} by {{actor}}".into(),
            target: TriggerDispatchTarget::SpawnWorkspaceAgent {
                cwd: "/workspace/orbit".into(),
                provider: ProviderKind::ClaudeCode,
                model: Some("claude-sonnet-4-5".into()),
                mode: SessionMode::AcceptEdits,
            },
            webhook_path: "/webhooks/trg_abc12345".into(),
            has_secret: true,
            masked_secret: Some("••••".into()),
            github_poll: Some(poll.clone()),
            enabled: true,
            trigger_count: 3,
            last_triggered_at_ms: Some(1_760_000_005_000),
            created_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_005_000,
        };
        let delivery = EventTriggerDelivery {
            delivery_id: "del_xyz".into(),
            trigger_id: "trg_abc12345".into(),
            source: TriggerSourceKind::GitHub,
            event_type: "pull_request.opened".into(),
            status: "dispatched".into(),
            summary: "Dispatched to agent ag_1".into(),
            dispatched_agent_id: Some("ag_1".into()),
            received_at_ms: 1_760_000_005_000,
        };

        let messages = vec![
            WireMessage::ListEventTriggers { request_id: req_id },
            WireMessage::EventTriggersSnapshot {
                request_id: req_id,
                triggers: vec![trigger],
                recent_deliveries: vec![delivery.clone()],
                base_webhook_url: "http://127.0.0.1:6868".into(),
            },
            WireMessage::UpsertEventTrigger {
                request_id: req_id,
                trigger_id: Some("trg_abc12345".into()),
                name: "PR Auto-Review".into(),
                source: TriggerSourceKind::GitHub,
                event_filters: vec!["pull_request.opened".into()],
                prompt_template: "Review {{title}}".into(),
                target: TriggerDispatchTarget::ExistingAgent {
                    agent_id: "ag_main".into(),
                },
                secret: None,
                keep_existing_secret: true,
                github_poll: Some(poll),
                github_token: None,
                keep_existing_token: true,
                enabled: true,
            },
            WireMessage::DeleteEventTrigger {
                request_id: req_id,
                trigger_id: "trg_abc12345".into(),
            },
            WireMessage::TestEventTrigger {
                request_id: req_id,
                trigger_id: "trg_abc12345".into(),
                sample_event_type: "pull_request.opened".into(),
                sample_payload: serde_json::json!({ "title": "Fix bug" }),
            },
            WireMessage::EventTriggerTestResponse {
                request_id: req_id,
                delivery,
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode event trigger wire msg");
            let decoded =
                WireMessage::decode_zstd(&encoded).expect("decode event trigger wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn workflow_wire_messages_roundtrip() {
        let req_id = Uuid::new_v4();
        let run = WorkflowRunRecord {
            run_id: "wf_abc".into(),
            name: "Best-of-2".into(),
            kind: WorkflowKind::BestOfN,
            workspace_cwd: "/tmp/repo".into(),
            owner_agent_id: Some("ag_owner".into()),
            status: WorkflowRunStatus::Running,
            max_concurrency: 2,
            verify_command: Some("cargo test".into()),
            winner_step_id: None,
            steps: vec![WorkflowStepRecord {
                run_id: "wf_abc".into(),
                step_id: "cand_1".into(),
                name: "Claude".into(),
                status: WorkflowStepStatus::Succeeded,
                depends_on: Vec::new(),
                agent_id: Some("ag_1".into()),
                worktree_cwd: Some("/tmp/wt-1".into()),
                provider: ProviderKind::ClaudeCode,
                model: Some("claude-opus-4-6".into()),
                output_preview: Some("Done".into()),
                diff_additions: 12,
                diff_deletions: 4,
                diff_files: 2,
                verify_passed: Some(true),
                verify_output: Some("ok".into()),
                error: None,
                is_winner: false,
                started_at_ms: Some(1_760_000_001_000),
                finished_at_ms: Some(1_760_000_010_000),
            }],
            created_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_010_000,
            finished_at_ms: None,
        };

        let messages = vec![
            WireMessage::StartBatchFanout {
                request_id: req_id,
                name: "Migrate endpoints".into(),
                cwd: "/tmp/repo".into(),
                csv_content: "file,owner\nsrc/a.rs,alice".into(),
                prompt_template: "Migrate {{file}} for {{owner}}".into(),
                provider: ProviderKind::ClaudeCode,
                model: None,
                mode: SessionMode::AcceptEdits,
                max_concurrency: 4,
                isolated_worktree: true,
            },
            WireMessage::StartDagWorkflow {
                request_id: req_id,
                name: "Build & Verify".into(),
                cwd: "/tmp/repo".into(),
                steps: vec![DagStepSpec {
                    step_id: "plan".into(),
                    name: "Plan".into(),
                    depends_on: Vec::new(),
                    prompt_template: "Draft plan".into(),
                    provider: Some(ProviderKind::ClaudeCode),
                    model: None,
                    isolated_worktree: false,
                    retry_count: 1,
                    timeout_secs: 600,
                }],
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::AcceptEdits,
                max_concurrency: 3,
            },
            WireMessage::StartBestOfN {
                request_id: req_id,
                name: "Arena".into(),
                cwd: "/tmp/repo".into(),
                prompt: "Implement feature".into(),
                candidates: vec![
                    BestOfNCandidateSpec {
                        label: "Claude".into(),
                        provider: ProviderKind::ClaudeCode,
                        model: None,
                        effort: Some("high".into()),
                    },
                    BestOfNCandidateSpec {
                        label: "Codex".into(),
                        provider: ProviderKind::Codex,
                        model: None,
                        effort: None,
                    },
                ],
                mode: SessionMode::AcceptEdits,
                verify_command: Some("cargo test".into()),
            },
            WireMessage::ListWorkflowRuns {
                request_id: req_id,
                include_completed: true,
            },
            WireMessage::CancelWorkflowRun {
                request_id: req_id,
                run_id: "wf_abc".into(),
            },
            WireMessage::SelectBestOfNWinner {
                request_id: req_id,
                run_id: "wf_abc".into(),
                winner_step_id: "cand_1".into(),
                apply_to_workspace: true,
            },
            WireMessage::WorkflowRunUpdated {
                request_id: req_id,
                run: run.clone(),
            },
            WireMessage::WorkflowRunsSnapshot {
                request_id: req_id,
                runs: vec![run.clone()],
            },
            WireMessage::WorkflowRunEvent { run },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode workflow wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode workflow wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn im_channels_wire_messages_roundtrip() {
        let req_id = Uuid::new_v4();
        let ch = ImChannelRecord {
            channel_id: "im_lark_01".into(),
            name: "Feishu Eng".into(),
            platform: ImPlatform::Lark,
            enabled: true,
            transport_mode: ImTransportMode::LongConnection,
            status: ImConnectionStatus::Connected,
            status_message: Some("Connected (WS)".into()),
            webhook_path: "/im/lark/im_lark_01".into(),
            app_id: Some("cli_12345".into()),
            has_primary_secret: true,
            has_signing_secret: true,
            has_app_token: false,
            masked_primary_secret: Some("••••9999".into()),
            masked_signing_secret: Some("••••8888".into()),
            masked_app_token: None,
            public_key: None,
            allowed_chat_ids: vec!["oc_1".into()],
            allowed_user_ids: vec!["ou_1".into()],
            default_workspace_cwd: "/workspace/orbit".into(),
            default_provider: ProviderKind::ClaudeCode,
            default_mode: SessionMode::Default,
            forward_approvals: true,
            routing_mode: ImRoutingMode::PerThreadAgent,
            router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            message_count: 5,
            last_activity_at_ms: Some(1_760_000_005_000),
            created_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_005_000,
        };
        let binding = ImThreadBinding {
            channel_id: "im_lark_01".into(),
            external_chat_id: "oc_1".into(),
            external_thread_id: "om_root".into(),
            agent_id: "ag_1".into(),
            updated_at_ms: 1_760_000_005_000,
        };
        let messages = vec![
            WireMessage::ListImChannels { request_id: req_id },
            WireMessage::ImChannelsSnapshot {
                request_id: req_id,
                channels: vec![ch],
                bindings: vec![binding],
                base_webhook_url: "http://127.0.0.1:7788".into(),
            },
            WireMessage::UpsertImChannel {
                request_id: req_id,
                channel_id: Some("im_lark_01".into()),
                name: "Feishu Eng".into(),
                platform: ImPlatform::Lark,
                enabled: true,
                transport_mode: ImTransportMode::LongConnection,
                app_id: Some("cli_12345".into()),
                primary_secret: None,
                signing_secret: None,
                app_token: None,
                public_key: None,
                keep_existing_secrets: true,
                allowed_chat_ids: vec!["oc_1".into()],
                allowed_user_ids: vec!["ou_1".into()],
                default_workspace_cwd: "/workspace/orbit".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PersonalRouter,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            },
            WireMessage::DeleteImChannel {
                request_id: req_id,
                channel_id: "im_lark_01".into(),
            },
            WireMessage::TestImChannel {
                request_id: req_id,
                channel_id: "im_lark_01".into(),
                target_chat_id: "oc_1".into(),
                message: "Orbit test".into(),
            },
            WireMessage::ImChannelTestResponse {
                request_id: req_id,
                ok: true,
                detail: "Delivered".into(),
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode im_channels wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode im_channels wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn push_wire_messages_roundtrip() {
        let req_id = Uuid::new_v4();
        let device = PushDeviceRecord {
            device_id: "ios_1".into(),
            platform: PushPlatform::Apns,
            device_name: "Jess iPhone".into(),
            masked_token: "••••abcdef".into(),
            sandbox: false,
            enabled: true,
            last_delivered_at_ms: Some(1_760_000_005_000),
            last_error: None,
            registered_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_005_000,
        };
        let prefs = PushPreferences {
            push_gateway_url: Some("https://push.orbit.example/v1/dispatch".into()),
            has_gateway_auth_token: true,
            ..PushPreferences::default()
        };
        let messages = vec![
            WireMessage::RegisterPushDevice {
                request_id: req_id,
                device_id: "ios_1".into(),
                platform: PushPlatform::Apns,
                device_name: "Jess iPhone".into(),
                device_token: "apns_token_123456abcdef".into(),
                sandbox: false,
            },
            WireMessage::UnregisterPushDevice {
                request_id: req_id,
                device_id: "ios_1".into(),
            },
            WireMessage::ListPushDevices { request_id: req_id },
            WireMessage::UpdatePushPreferences {
                request_id: req_id,
                preferences: prefs.clone(),
                gateway_auth_token: Some("secret_token".into()),
                keep_existing_gateway_token: false,
            },
            WireMessage::PushDevicesSnapshot {
                request_id: req_id,
                devices: vec![device],
                preferences: prefs,
            },
            WireMessage::TestPushNotification {
                request_id: req_id,
                device_id: Some("ios_1".into()),
            },
            WireMessage::PushTestResponse {
                request_id: req_id,
                attempted: 1,
                delivered: 1,
                detail: "Delivered to 1/1 devices".into(),
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode push wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode push wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }
    }

    #[test]
    fn git_write_wire_messages_roundtrip() {
        let req_id = Uuid::new_v4();
        let sample_status = GitStatusSnapshot {
            cwd: "/workspace/repo".into(),
            is_git_repo: true,
            branch: Some("feat/git-write".into()),
            upstream: Some("origin/feat/git-write".into()),
            ahead: 1,
            behind: 0,
            files: vec![GitFileEntry {
                path: "src/lib.rs".into(),
                status: "M".into(),
                staged: true,
                untracked: false,
            }],
            nested_git_repos: Vec::new(),
        };
        let commit_res = GitCommitResult {
            commit_sha: "0123456789abcdef0123456789abcdef01234567".into(),
            short_sha: "0123456".into(),
            summary: "feat: add safe git write backend".into(),
            branch: Some("feat/git-write".into()),
        };
        let push_res = GitPushResult {
            remote: "origin".into(),
            branch: "feat/git-write".into(),
            output: "To origin\n * [new branch] feat/git-write -> feat/git-write".into(),
        };
        let pr_res = GitCreatePrResult {
            url: "https://github.com/orbit-ai/orbit/pull/42".into(),
            number: Some(42),
            title: "Safe Git Write Backend".into(),
        };
        let messages = vec![
            WireMessage::GitStageFiles {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                paths: vec!["src/lib.rs".into()],
                all: false,
            },
            WireMessage::GitUnstageFiles {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                paths: Vec::new(),
                all: true,
            },
            WireMessage::GitApplyHunk {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                patch:
                    "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,1 +1,2 @@\n fn a() {}\n+fn b() {}\n"
                        .into(),
                reverse: false,
                cached: true,
            },
            WireMessage::GitDiscardFiles {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                paths: vec!["src/lib.rs".into(), "scratch.txt".into()],
                include_untracked: true,
            },
            WireMessage::GitCommit {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                message: "feat: safe git commit\n\nWith $special `chars`".into(),
                amend: false,
            },
            WireMessage::GitPush {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                remote: Some("origin".into()),
                branch: Some("feat/git-write".into()),
                set_upstream: true,
            },
            WireMessage::GitCreatePr {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                title: "Safe Git Write Backend".into(),
                body: "## Summary\n- Adds git write RPCs".into(),
                base: Some("main".into()),
                draft: true,
            },
            WireMessage::GitMutationResult {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                result: GitMutationOutcome::FilesStaged {
                    paths: vec!["src/lib.rs".into()],
                    all: false,
                },
                status: Some(sample_status.clone()),
            },
            WireMessage::GitMutationResult {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                result: GitMutationOutcome::Committed(commit_res),
                status: Some(sample_status.clone()),
            },
            WireMessage::GitMutationResult {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                result: GitMutationOutcome::Pushed(push_res),
                status: None,
            },
            WireMessage::GitMutationResult {
                request_id: req_id,
                cwd: "/workspace/repo".into(),
                result: GitMutationOutcome::PrCreated(pr_res),
                status: Some(sample_status),
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode git write wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode git write wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }

        // Verify internally tagged JSON shape for TypeScript parity
        let committed_json = serde_json::to_value(GitMutationOutcome::Committed(GitCommitResult {
            commit_sha: "abc".into(),
            short_sha: "abc".into(),
            summary: "hi".into(),
            branch: Some("main".into()),
        }))
        .expect("serialize committed outcome");
        assert_eq!(committed_json["kind"], "committed");
        assert_eq!(committed_json["commit_sha"], "abc");
        assert_eq!(committed_json["branch"], "main");
    }

    #[test]
    fn git_collab_wire_messages_roundtrip_and_preserve_json_shapes() {
        let req_id = Uuid::new_v4();
        let stash_list = GitStashListResult {
            entries: vec![GitStashEntry {
                stash_ref: "stash@{0}".into(),
                index: 0,
                branch: Some("feat/git-collab".into()),
                message: "WIP savepoint".into(),
                oid: Some("1234abcdef1234abcdef1234abcdef1234abcdef".into()),
                short_sha: Some("1234abc".into()),
                created_at: Some("2026-11-24T12:00:00Z".into()),
            }],
        };
        let rebase_status = GitRebaseStatus {
            in_progress: true,
            kind: GitRebaseKind::Merge,
            current_step: Some(2),
            total_steps: Some(4),
            head_name: Some("feat/git-collab".into()),
            onto: Some("main".into()),
            stopped_message: Some("feat: add conflict parser".into()),
            conflicted_files: vec!["src/lib.rs".into()],
            merge_in_progress: false,
            cherry_pick_in_progress: false,
        };
        let conflict_list = GitConflictListResult {
            files: vec![GitConflictFile {
                path: "src/lib.rs".into(),
                blocks: vec![GitConflictBlock {
                    index: 0,
                    start_line: 10,
                    end_line: 18,
                    ours_label: "HEAD".into(),
                    base_label: Some("base".into()),
                    theirs_label: "feat/other".into(),
                    ours: "let x = 1;".into(),
                    base: Some("let x = 0;".into()),
                    theirs: "let x = 2;".into(),
                    context_before: vec!["fn main() {".into()],
                    context_after: vec!["}".into()],
                }],
                is_binary: false,
                unmerged_in_index: true,
                total_blocks: 1,
                content_sha256: Some("a".repeat(64)),
            }],
        };
        let pr_status = GitPrStatus {
            availability: GitPrAvailability::Available,
            reason: None,
            number: Some(99),
            title: Some("Deep Git Collaboration Loop".into()),
            url: Some("https://github.com/danielgaoyy/orbit/pull/99".into()),
            state: Some("OPEN".into()),
            is_draft: false,
            mergeable: Some("MERGEABLE".into()),
            review_decision: Some("CHANGES_REQUESTED".into()),
            head_ref: Some("feat/git-collab".into()),
            base_ref: Some("main".into()),
            checks: vec![GitPrCheckItem {
                name: "cargo test".into(),
                state: GitCiCheckState::Failing,
                workflow: Some("CI".into()),
                raw_status: Some("COMPLETED".into()),
                raw_conclusion: Some("FAILURE".into()),
                details_url: Some("https://github.com/danielgaoyy/orbit/actions/runs/1".into()),
            }],
            passing_checks: 3,
            failing_checks: 1,
            pending_checks: 0,
            review_comments: vec![GitPrReviewComment {
                id: "c1".into(),
                author: "reviewer".into(),
                body: "Please handle CRLF line endings.".into(),
                path: Some("crates/orbit-daemon/src/git_collab.rs".into()),
                line: Some(42),
                start_line: Some(38),
                url: Some("https://github.com/danielgaoyy/orbit/pull/99#discussion_r1".into()),
                created_at: Some("2026-11-24T12:10:00Z".into()),
                is_resolved: false,
                is_outdated: false,
                kind: "review_thread".into(),
            }],
            unresolved_comment_count: 1,
        };
        let commit_msg = GitGeneratedCommitMessage {
            message: "feat(daemon): add git_collab module\n\n- Add git_collab.rs (+120/-0)".into(),
            subject: "feat(daemon): add git_collab module".into(),
            body: Some("- Add git_collab.rs (+120/-0)".into()),
            commit_type: "feat".into(),
            scope: Some("daemon".into()),
            diff_source: "staged".into(),
            files_analyzed: 1,
            generator: "deterministic".into(),
        };

        let messages = vec![
            WireMessage::GitStashList {
                request_id: req_id,
                cwd: "/repo".into(),
            },
            WireMessage::GitStashPush {
                request_id: req_id,
                cwd: "/repo".into(),
                message: Some("checkpoint".into()),
                include_untracked: true,
            },
            WireMessage::GitStashApply {
                request_id: req_id,
                cwd: "/repo".into(),
                stash_ref: "stash@{0}".into(),
                expected_oid: Some("1234abcdef1234abcdef1234abcdef1234abcdef".into()),
            },
            WireMessage::GitStashPop {
                request_id: req_id,
                cwd: "/repo".into(),
                stash_ref: "stash@{0}".into(),
                expected_oid: Some("1234abcdef1234abcdef1234abcdef1234abcdef".into()),
            },
            WireMessage::GitStashDrop {
                request_id: req_id,
                cwd: "/repo".into(),
                stash_ref: "stash@{0}".into(),
                expected_oid: Some("1234abcdef1234abcdef1234abcdef1234abcdef".into()),
            },
            WireMessage::GitRebaseStatus {
                request_id: req_id,
                cwd: "/repo".into(),
            },
            WireMessage::GitRebaseStart {
                request_id: req_id,
                cwd: "/repo".into(),
                onto_ref: "origin/main".into(),
            },
            WireMessage::GitRebaseContinue {
                request_id: req_id,
                cwd: "/repo".into(),
            },
            WireMessage::GitRebaseAbort {
                request_id: req_id,
                cwd: "/repo".into(),
            },
            WireMessage::GitRebaseSkip {
                request_id: req_id,
                cwd: "/repo".into(),
                expected_step: Some(2),
            },
            WireMessage::GitListConflicts {
                request_id: req_id,
                cwd: "/repo".into(),
            },
            WireMessage::GitResolveConflict {
                request_id: req_id,
                cwd: "/repo".into(),
                path: "src/lib.rs".into(),
                block_index: Some(0),
                resolution: GitConflictResolution::ManualContent {
                    content: "let x = 3;".into(),
                },
                auto_stage: true,
                expected_sha256: Some("a".repeat(64)),
            },
            WireMessage::GitPrStatus {
                request_id: req_id,
                cwd: "/repo".into(),
            },
            WireMessage::GitGenerateCommitMessage {
                request_id: req_id,
                cwd: "/repo".into(),
            },
            WireMessage::GitCollabResponse {
                request_id: req_id,
                cwd: "/repo".into(),
                payload: GitCollabQueryResult::StashList(stash_list),
            },
            WireMessage::GitCollabResponse {
                request_id: req_id,
                cwd: "/repo".into(),
                payload: GitCollabQueryResult::RebaseStatus(rebase_status.clone()),
            },
            WireMessage::GitCollabResponse {
                request_id: req_id,
                cwd: "/repo".into(),
                payload: GitCollabQueryResult::ConflictList(conflict_list),
            },
            WireMessage::GitCollabResponse {
                request_id: req_id,
                cwd: "/repo".into(),
                payload: GitCollabQueryResult::PrStatus(pr_status),
            },
            WireMessage::GitCollabResponse {
                request_id: req_id,
                cwd: "/repo".into(),
                payload: GitCollabQueryResult::CommitMessage(commit_msg),
            },
            WireMessage::GitMutationResult {
                request_id: req_id,
                cwd: "/repo".into(),
                result: GitMutationOutcome::StashMutated(GitStashMutationResult {
                    action: "push".into(),
                    stash_ref: Some("stash@{0}".into()),
                    summary: "Saved working tree".into(),
                    had_conflicts: false,
                }),
                status: None,
            },
            WireMessage::GitMutationResult {
                request_id: req_id,
                cwd: "/repo".into(),
                result: GitMutationOutcome::RebaseAction(GitRebaseActionResult {
                    action: "start".into(),
                    status: rebase_status,
                    summary: "Paused on conflicts".into(),
                }),
                status: None,
            },
            WireMessage::GitMutationResult {
                request_id: req_id,
                cwd: "/repo".into(),
                result: GitMutationOutcome::ConflictResolved(GitResolveConflictResult {
                    path: "src/lib.rs".into(),
                    remaining_blocks: 0,
                    staged: true,
                    summary: "Resolved and staged src/lib.rs".into(),
                }),
                status: None,
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode git collab wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode git collab wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig"),
                serde_json::to_value(&decoded).expect("decoded")
            );
        }

        let res_json = serde_json::to_value(GitConflictResolution::ManualContent {
            content: "merged".into(),
        })
        .expect("serialize resolution");
        assert_eq!(res_json["strategy"], "manual_content");
        assert_eq!(res_json["content"], "merged");
    }

    #[test]
    fn github_repo_validation_accepts_valid_slugs_and_rejects_traversal_or_injection() {
        for valid in [
            DEFAULT_GITHUB_REPO,
            "my-org/orbit",
            "Org_123/orbit.rs",
            "a/b",
            "foo.bar/baz-qux_9",
        ] {
            assert!(
                is_valid_github_repo(valid),
                "expected valid github repo slug `{valid}`"
            );
        }

        let overlong = format!("{}/orbit", "a".repeat(101));
        for invalid in [
            "",
            "orbit",
            "/orbit",
            "owner/",
            "owner/repo/extra",
            "../orbit",
            "owner/..",
            "owner/a..b",
            "a..b/orbit",
            "./orbit",
            "owner/.",
            "owner/repo?foo=bar",
            "owner/repo#frag",
            "owner/repo;id",
            "owner/repo | sh",
            "owner/repo\n",
            " owner/repo",
            "owner/repo ",
            "owner/repo\0",
            "https://github.com/danielgaoyy/orbit",
            "owner/örbit",
            overlong.as_str(),
        ] {
            assert!(
                !is_valid_github_repo(invalid),
                "expected invalid github repo slug `{invalid:?}` to be rejected"
            );
        }
    }

    #[test]
    fn github_repo_resolution_precedence_and_fallback() {
        assert_eq!(
            resolve_github_repo_from(Some("fork-org/orbit"), Some("compile-org/orbit")),
            "fork-org/orbit"
        );
        assert_eq!(
            resolve_github_repo_from(Some("../evil"), Some("compile-org/orbit")),
            "compile-org/orbit"
        );
        assert_eq!(
            resolve_github_repo_from(None, Some("compile-org/orbit.v2")),
            "compile-org/orbit.v2"
        );
        assert_eq!(
            resolve_github_repo_from(Some("bad slug"), Some("also/../bad")),
            DEFAULT_GITHUB_REPO
        );
        assert_eq!(resolve_github_repo_from(None, None), DEFAULT_GITHUB_REPO);

        let effective = configured_github_repo();
        assert!(
            is_valid_github_repo(&effective),
            "configured_github_repo() must always return a valid slug, got `{effective}`"
        );
    }

    #[test]
    fn agent_message_delivery_status_and_interrupt_preserve_queue_roundtrip_and_compat() {
        let legacy_agent_msg = serde_json::json!({
            "type": "agent_message",
            "message_id": "am_legacy",
            "kind": "finish_notification",
            "source_agent_id": "ag_child",
            "text": "Done"
        });
        let decoded_legacy: TimelinePayload =
            serde_json::from_value(legacy_agent_msg).expect("deserialize legacy AgentMessage");
        match decoded_legacy {
            TimelinePayload::AgentMessage {
                delivery_status,
                delivery_turn_id,
                blocking_turn_id,
                queued_at_ms,
                dispatched_at_ms,
                settled_at_ms,
                ..
            } => {
                assert!(delivery_status.is_none());
                assert!(delivery_turn_id.is_none());
                assert!(blocking_turn_id.is_none());
                assert!(queued_at_ms.is_none());
                assert!(dispatched_at_ms.is_none());
                assert!(settled_at_ms.is_none());
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        let updated = TimelinePayload::AgentMessageDeliveryUpdated {
            message_id: "am_1".into(),
            status: AgentMessageDeliveryStatus::Dispatched,
            delivery_turn_id: Some("turn_2".into()),
            blocking_turn_id: None,
            timestamp_ms: 1_732_400_000_000,
        };
        let roundtrip: TimelinePayload =
            serde_json::from_str(&serde_json::to_string(&updated).expect("ser")).expect("de");
        match roundtrip {
            TimelinePayload::AgentMessageDeliveryUpdated {
                message_id,
                status,
                delivery_turn_id,
                blocking_turn_id,
                timestamp_ms,
            } => {
                assert_eq!(message_id, "am_1");
                assert_eq!(status, AgentMessageDeliveryStatus::Dispatched);
                assert_eq!(delivery_turn_id.as_deref(), Some("turn_2"));
                assert!(blocking_turn_id.is_none());
                assert_eq!(timestamp_ms, 1_732_400_000_000);
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        let req_id = Uuid::new_v4();
        let legacy_interrupt = serde_json::json!({
            "op": "interrupt_turn",
            "request_id": req_id,
            "agent_id": "ag_p"
        });
        let parsed_legacy_int: WireMessage =
            serde_json::from_value(legacy_interrupt).expect("deserialize legacy InterruptTurn");
        match parsed_legacy_int {
            WireMessage::InterruptTurn { preserve_queue, .. } => assert!(!preserve_queue),
            other => panic!("unexpected wire: {other:?}"),
        }

        let continue_wire = WireMessage::InterruptTurn {
            request_id: req_id,
            agent_id: "ag_p".into(),
            preserve_queue: true,
        };
        let encoded = serde_json::to_string(&continue_wire).expect("ser interrupt");
        assert!(encoded.contains("\"preserve_queue\":true"));
        let decoded_wire: WireMessage = serde_json::from_str(&encoded).expect("de interrupt");
        match decoded_wire {
            WireMessage::InterruptTurn { preserve_queue, .. } => assert!(preserve_queue),
            other => panic!("unexpected wire: {other:?}"),
        }
    }
}
