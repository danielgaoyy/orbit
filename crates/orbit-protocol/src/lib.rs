//! Core wire types, state machines, timeline events, git/worktree models, and binary terminal framing.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;

/// First-class supported coding agent CLIs in Orbit Phase 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Antigravity,
    Codex,
    ClaudeCode,
}

impl ProviderKind {
    pub fn default_binary(self) -> &'static str {
        match self {
            Self::Antigravity => "agy",
            Self::Codex => "codex",
            Self::ClaudeCode => "claude",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Antigravity => "Antigravity",
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
        }
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

/// Workspace record (either a direct local checkout or an isolated git worktree).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub workspace_id: String,
    pub title: String,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub is_worktree: bool,
    pub updated_at: DateTime<Utc>,
}

/// Auto-discovered local project / Git repository choice for zero-typing UI selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredProject {
    pub path: String,
    pub name: String,
    pub git_branch: Option<String>,
    pub is_git_repo: bool,
    pub last_modified_unix: i64,
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
    pub mode: SessionMode,
    pub status: AgentStatus,
    pub parent_agent_id: Option<String>,
    pub latest_seq: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
    StatusChanged {
        status: AgentStatus,
    },
}

/// Client-to-Daemon and Daemon-to-Client RPC envelope over WebSocket / E2EE Relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
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

    // Workspaces & Git / Worktrees
    DiscoverProjects {
        request_id: Uuid,
    },
    DiscoveredProjectsSnapshot {
        request_id: Uuid,
        projects: Vec<DiscoveredProject>,
    },
    ListWorkspaces {
        request_id: Uuid,
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
    },
    WorkspaceCreated {
        request_id: Uuid,
        workspace: WorkspaceSummary,
    },
    GetGitStatus {
        request_id: Uuid,
        cwd: String,
    },
    GitStatusResponse {
        request_id: Uuid,
        status: GitStatusSnapshot,
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

    // Agents & Interactive Orchestration
    ListAgents {
        request_id: Uuid,
        workspace_id: Option<String>,
    },
    AgentsSnapshot {
        request_id: Uuid,
        agents: Vec<AgentSummary>,
    },
    CreateAgent {
        request_id: Uuid,
        cwd: String,
        title: Option<String>,
        provider: ProviderKind,
        model: Option<String>,
        mode: SessionMode,
        initial_prompt: String,
        #[serde(default)]
        worktree: bool,
    },
    AgentCreated {
        request_id: Uuid,
        agent: AgentSummary,
    },
    AgentUpdated {
        agent: AgentSummary,
    },
    SendPrompt {
        request_id: Uuid,
        agent_id: String,
        prompt: String,
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
    SwitchAgentProvider {
        request_id: Uuid,
        agent_id: String,
        provider: ProviderKind,
    },
    ArchiveAgent {
        request_id: Uuid,
        agent_id: String,
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

    // Quotas
    RefreshQuotas {
        request_id: Uuid,
    },
    QuotaReport {
        request_id: Option<Uuid>,
        snapshots: Vec<ProviderQuotaSnapshot>,
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

impl WireMessage {
    /// Encode message as Zstd-compressed JSON bytes for cross-border / mobile bandwidth efficiency.
    pub fn encode_zstd(&self, level: i32) -> Result<Vec<u8>, ProtocolError> {
        let json = serde_json::to_vec(self)?;
        Ok(zstd::encode_all(json.as_slice(), level)?)
    }

    /// Decode a Zstd-compressed WireMessage (with a 16 MiB anti-bomb cap).
    pub fn decode_zstd(compressed: &[u8]) -> Result<Self, ProtocolError> {
        use std::io::Read;
        let decoder = zstd::Decoder::new(compressed)?;
        let mut decompressed = Vec::new();
        decoder
            .take(16 * 1024 * 1024)
            .read_to_end(&mut decompressed)?;
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
            title: Some("Build Orbit".into()),
            provider: ProviderKind::Antigravity,
            model: Some("gemini-pro".into()),
            mode: SessionMode::AcceptEdits,
            initial_prompt: "Build the Rust agent orchestrator!".repeat(20),
            worktree: false,
        };
        let raw_json = serde_json::to_vec(&msg).expect("serialize");
        let compressed = msg.encode_zstd(3).expect("compress");
        assert!(compressed.len() < raw_json.len());
        let decoded = WireMessage::decode_zstd(&compressed).expect("decompress");
        match decoded {
            WireMessage::CreateAgent { provider, .. } => {
                assert_eq!(provider, ProviderKind::Antigravity);
            }
            _ => panic!("unexpected variant"),
        }
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
}
