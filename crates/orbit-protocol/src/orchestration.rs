//! Orchestration domain models for multi-agent delegation, durable finish watches,
//! agent profiles, cron schedules & heartbeats, worktree scripts/services, connected hosts,
//! and provider-native subagent observability.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{ProviderKind, ReasoningEffort, SessionMode, TimelinePayload, ToolCallDetail};

/// Daemon-owned unread/attention indicator for an agent session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionState {
    #[default]
    None,
    Finished,
    Error,
    Permission,
    /// Explicitly marked unread by the user (synced across all connected devices without regressing `read_seq`).
    ManualUnread,
}

/// How an incoming prompt should be delivered when the target agent already has an active turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveTurnBehavior {
    /// Steer the running turn non-destructively without cancelling running subagents or background tasks.
    ///
    /// - Runtimes with native mid-turn injection (Antigravity concurrent `session/prompt`, Codex
    ///   `turn/steer`, Claude stdin `user` frame, Pi `steer` RPC) inject human steers directly into
    ///   the active turn with zero force cancellation.
    /// - Generic ACP runtimes without native mid-turn steering degrade human `Steer` to a
    ///   non-destructive priority queue drained ahead of queued agent notifications once the active
    ///   turn settles naturally.
    /// - Agent-originated messages/notifications on ACP/Antigravity degrade `Steer` to FIFO `Queue`
    ///   so active parent turns are never disturbed.
    #[default]
    Steer,
    /// Queue strictly in FIFO order until the current turn transitions to `Idle`.
    Queue,
    /// Interrupt the current turn, await termination, then dispatch the new prompt.
    Interrupt,
}

/// Authoritative delivery lifecycle state of an agent-originated message / finish notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMessageDeliveryStatus {
    /// Accepted into the recipient driver's FIFO queue behind an active turn.
    Queued,
    /// Dispatched to the underlying provider runtime (e.g. ACP `session/prompt`, Codex `turn/start`/`turn/steer`, Claude stdin frame, Pi RPC).
    Dispatched,
    /// Recipient turn processing this message or notification has settled.
    Settled,
    /// Dropped before dispatch (e.g. hard user Stop or driver exit while still queued).
    Dropped,
}

/// Isolation mode when creating a workspace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceIsolation {
    #[default]
    Local,
    Worktree,
}

/// Strategy for creating a git worktree-backed workspace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeCreateMode {
    #[default]
    BranchOff,
    CheckoutBranch,
    CheckoutPr,
}

/// Lifecycle state of workspace `orbit.json` / `paseo.json` setup hooks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceSetupStatus {
    #[default]
    Idle,
    Running,
    Succeeded,
    Failed,
}

/// Origin category for non-human agent timeline messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMessageKind {
    DirectMessage,
    FinishNotification,
    Heartbeat,
}

/// Inline actionable permission metadata embedded in a child-to-parent permission checkpoint notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionCheckpointRef {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    pub request_id: String,
    pub title: String,
    pub detail: ToolCallDetail,
    pub allowed_decisions: Vec<String>,
}

/// Summary of a currently pending permission approval across managed agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPermissionSummary {
    pub agent_id: String,
    pub agent_title: String,
    pub workspace_id: String,
    pub request_id: String,
    pub title: String,
    pub detail: ToolCallDetail,
    pub allowed_decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guardian_assessment: Option<crate::GuardianAssessment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_approval: Option<crate::PlanApprovalPayload>,
    /// True when explicit human approval is required (plan approval gate, PreToolUse fail-closed,
    /// High/Critical risk, or Guardian fail-closed/fallback verdict). Must never be delegated to
    /// Personal Router or resolved by an agent.
    #[serde(default)]
    pub human_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_recommendation: Option<crate::RouterRecommendation>,
    pub created_at: DateTime<Utc>,
}

/// Lifecycle state of a provider-owned native subagent (Claude Task, Codex collab thread, AGY subagent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSubagentStatus {
    Running,
    Completed,
    Failed,
    Canceled,
}

/// Read-only descriptor for a provider-native subagent surfaced in the parent's Subagents Track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSubagentDescriptor {
    pub subagent_id: String,
    pub parent_agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_subagent_id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub status: ProviderSubagentStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Sequenced read-only timeline entry belonging to a provider-native subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSubagentEvent {
    pub seq: u64,
    pub parent_agent_id: String,
    pub subagent_id: String,
    pub timestamp: DateTime<Utc>,
    pub payload: TimelinePayload,
}

fn default_true() -> bool {
    true
}

fn default_idle_timeout_secs() -> u64 {
    3600
}

fn default_resume_prompt() -> String {
    "Resume working.".to_string()
}

/// Capability pack grouping Orbit MCP orchestration tools to keep default context lightweight
/// while locking session tool definitions at launch to preserve LLM prompt cache prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpToolPack {
    Core,
    Browser,
    Schedules,
    Workflows,
    Goals,
    Terminals,
    Artifacts,
    Lsp,
}

impl McpToolPack {
    pub const ALL: [Self; 8] = [
        Self::Core,
        Self::Browser,
        Self::Schedules,
        Self::Workflows,
        Self::Goals,
        Self::Terminals,
        Self::Artifacts,
        Self::Lsp,
    ];

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Browser => "browser",
            Self::Schedules => "schedules",
            Self::Workflows => "workflows",
            Self::Goals => "goals",
            Self::Terminals => "terminals",
            Self::Artifacts => "artifacts",
            Self::Lsp => "lsp",
        }
    }

    pub fn parse_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "core" => Some(Self::Core),
            "browser" | "cdp" => Some(Self::Browser),
            "schedules" | "schedule" | "cron" | "heartbeats" => Some(Self::Schedules),
            "workflows" | "workflow" => Some(Self::Workflows),
            "goals" | "goal" | "monitors" | "monitor" => Some(Self::Goals),
            "terminals" | "terminal" | "pty" => Some(Self::Terminals),
            "artifacts" | "artifact" => Some(Self::Artifacts),
            "lsp" => Some(Self::Lsp),
            _ => None,
        }
    }
}

/// Per-profile MCP tool catalog policy (e.g., disallow worker profiles from spawning recursive subagents).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolPolicy {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    /// Explicit override list of capability packs to expose for this profile (overrides auto-detection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_packs: Option<Vec<McpToolPack>>,
    /// Capability packs explicitly suppressed even if auto-detected or enabled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_packs: Vec<McpToolPack>,
}

impl Default for McpToolPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            disabled_tools: Vec::new(),
            enabled_packs: None,
            disabled_packs: Vec::new(),
        }
    }
}

/// Reusable host-scoped Agent Profile with specialization guidance (`when_to_use`) for orchestrators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfile {
    pub profile_id: String,
    pub name: String,
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub fast_mode: bool,
    pub mode: SessionMode,
    #[serde(default)]
    pub when_to_use: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_append: Option<String>,
    #[serde(default)]
    pub mcp_policy: McpToolPolicy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Host-level orchestration & lifecycle runtime settings persisted in `$ORBIT_HOME/config.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationConfig {
    /// Inject Orbit's built-in MCP tool server into launched agents (default: `true`).
    #[serde(default = "default_true")]
    pub mcp_inject_enabled: bool,
    /// Collect idle resumable provider runtimes after `idle_timeout_secs` (default: `true`).
    #[serde(default = "default_true")]
    pub idle_cleanup_enabled: bool,
    /// Inactivity window before idle runtime collection (default: 3600s / 1 hour).
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
    /// Automatically reopen agents that were `Running` before a daemon restart (default: `false`).
    #[serde(default)]
    pub resume_on_startup: bool,
    /// Continuation prompt sent on startup recovery (default: `"Resume working."`).
    #[serde(default = "default_resume_prompt")]
    pub resume_prompt: String,
    /// IDs of bundled orchestration skills synced to local agent skill directories.
    #[serde(default)]
    pub installed_skills: Vec<String>,
    /// Automatically cluster closely related new conversations into existing Mission Workspaces (default: `false`; manual AI triage on demand).
    #[serde(default)]
    pub auto_cluster_enabled: bool,
    /// Prevent OS idle sleep while agents, autonomous wakeups, or scheduled runs are active (default: `true`).
    #[serde(default = "default_true")]
    pub keep_awake_while_running: bool,
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        Self {
            mcp_inject_enabled: true,
            idle_cleanup_enabled: true,
            idle_timeout_secs: default_idle_timeout_secs(),
            resume_on_startup: false,
            resume_prompt: default_resume_prompt(),
            installed_skills: Vec::new(),
            auto_cluster_enabled: false,
            keep_awake_while_running: true,
        }
    }
}

/// Live runtime status of the daemon's OS sleep-prevention guard (`KeepAwakeManager`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepAwakeStatus {
    /// Whether automatic keep-awake is enabled in host configuration.
    #[serde(default)]
    pub enabled: bool,
    /// `true` while an OS sleep-prevention assertion/process is actively held.
    #[serde(default)]
    pub active: bool,
    /// Number of currently busy agents or active background runs requesting keep-awake.
    #[serde(default)]
    pub active_agents: usize,
    /// Active OS backend identifier (e.g. `"caffeinate"`, `"systemd-inhibit"`, `"windows-stub"`, `"noop"`).
    #[serde(default)]
    pub backend: String,
}

/// Status projection of a bundled Orbit orchestration skill on a host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestrationSkillInfo {
    pub id: String,
    pub name: String,
    pub slash_command: String,
    pub description: String,
    pub installed: bool,
    #[serde(default)]
    pub installed_targets: Vec<String>,
    #[serde(default)]
    pub outdated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    Schedule,
    Heartbeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleStatus {
    Active,
    Paused,
    Completed,
}

/// Configuration for a Cron Schedule that launches a fresh agent on every trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleNewAgentConfig {
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub provider: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub fast_mode: bool,
    pub mode: SessionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub worktree: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(default = "default_true")]
    pub archive_on_finish: bool,
    #[serde(default)]
    pub background: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleTarget {
    NewAgent(ScheduleNewAgentConfig),
    ExistingAgent { agent_id: String },
}

/// Persisted Cron Schedule or Heartbeat definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSpec {
    pub schedule_id: String,
    pub kind: ScheduleKind,
    pub name: String,
    pub cron_expr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    pub prompt: String,
    pub status: ScheduleStatus,
    pub target: ScheduleTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runs: Option<u32>,
    #[serde(default)]
    pub run_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleRunStatus {
    Running,
    Succeeded,
    Failed,
    Skipped,
}

/// Execution log record for a single trigger of a Schedule or Heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRunRecord {
    pub run_id: String,
    pub schedule_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub status: ScheduleRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectedHostStatus {
    Disabled,
    Connecting,
    Online,
    Error,
}

/// Safe projection of a Daemon-owned outbound Connected Host (for cross-host orchestration).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectedHostSummary {
    pub registration_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    pub label: String,
    pub daemon_pubkey_b64: String,
    #[serde(default)]
    pub direct_endpoints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
    pub enabled: bool,
    pub status: ConnectedHostStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub available_providers: Vec<ProviderKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_version: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceScriptKind {
    Command,
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceScriptStatus {
    Stopped,
    Running,
    Failed,
}

/// Configured script or long-running service parsed from `orbit.json` / `paseo.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceScriptInfo {
    pub workspace_id: String,
    pub name: String,
    pub command: String,
    pub kind: WorkspaceScriptKind,
    pub status: WorkspaceScriptStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Real-time setup hook progress event for worktree workspaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSetupProgress {
    pub workspace_id: String,
    pub status: WorkspaceSetupStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub step_index: u32,
    pub total_steps: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Discovered nested Git repository inside a non-git parent or multi-directory project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NestedGitRepo {
    pub path: String,
    pub relative_path: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
}

/// Daemon-authoritative saved project entry synced across all clients of a host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostProjectEntry {
    pub project_id: String,
    pub name: String,
    pub directories: Vec<String>,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub hidden: bool,
    pub updated_at_ms: i64,
}

/// Daemon-authoritative host-level UI preference snapshot synced across all connected devices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSyncState {
    #[serde(default)]
    pub projects: Vec<HostProjectEntry>,
    /// Ordered list of stable project keys (`project_id` or normalized `primary_cwd`) for Manual sidebar sort.
    #[serde(default)]
    pub project_order: Vec<String>,
    /// Optional preferred sidebar sort mode (`"recent"`, `"alphabetical"`, `"manual"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_sort_mode: Option<String>,
    #[serde(default)]
    pub updated_at_ms: i64,
}

/// Auto-clustering recommendation or reversible auto-grouping metadata attached to a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceClusterSuggestion {
    pub target_workspace_id: String,
    pub target_workspace_title: String,
    pub confidence: f32,
    pub reason: String,
    #[serde(default)]
    pub auto_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_workspace_id: Option<String>,
}

/// Shared Markdown Blackboard state for a Mission Workspace (`BOARD.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceBoardSnapshot {
    pub workspace_id: String,
    pub content: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_path: Option<String>,
}

/// Category of an intra-workspace collaboration or conflict-radar event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEventKind {
    PeerJoined,
    PeerFinished,
    BoardUpdated,
    PeerMessage,
    FileEdited,
    ConflictWarning,
    WorkspaceMerged,
}

/// Persisted chronological activity record inside a Mission Workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceEventRecord {
    pub seq: u64,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_handle: Option<String>,
    pub kind: WorkspaceEventKind,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Phase of an AI-assisted project workspace organization / triage run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceOrganizePhase {
    Running,
    Completed,
    Failed,
}

/// Live progress & outcome notification for AI workspace organization / triage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceOrganizeEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<uuid::Uuid>,
    pub project_root: String,
    pub phase: WorkspaceOrganizePhase,
    pub message: String,
    #[serde(default)]
    pub created_workspaces: u32,
    #[serde(default)]
    pub grouped_agents: u32,
    #[serde(default)]
    pub summarized_agents: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_tool_policy_legacy_json_deserializes_with_defaults() {
        let legacy: McpToolPolicy =
            serde_json::from_str(r#"{"enabled":true,"disabled_tools":["kill_agent"]}"#)
                .expect("deserialize legacy");
        assert!(legacy.enabled);
        assert_eq!(legacy.disabled_tools, vec!["kill_agent"]);
        assert!(legacy.enabled_packs.is_none());
        assert!(legacy.disabled_packs.is_empty());

        for pack in McpToolPack::ALL {
            assert_eq!(McpToolPack::parse_slug(pack.slug()), Some(pack));
        }
    }
}
