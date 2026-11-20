//! Core wire types, state machines, timeline events, git/worktree models, and binary terminal framing.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 2;

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
    pub mode: SessionMode,
    pub status: AgentStatus,
    pub parent_agent_id: Option<String>,
    pub latest_seq: u64,
    /// Monotonic counter incremented on every server-side timeline rewind so offline/unsubscribed
    /// clients detect history resets even after new post-rewind events surpass prior `latest_seq`.
    #[serde(default)]
    pub timeline_epoch: u64,
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

/// Compact semantic chip attached to a user turn (e.g. `/plan`, `@file`, `Working Tree Diff`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptBadge {
    pub kind: String,
    pub label: String,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        badges: Vec<PromptBadge>,
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
                mode: SessionMode::Default,
                status: AgentStatus::Idle,
                parent_agent_id: None,
                latest_seq: 12,
                timeline_epoch: 1,
                created_at: now,
                updated_at: now,
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
}
