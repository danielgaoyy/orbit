//! Unified Actor interface (`DriverCommand` / `DriverEvent`) shared by Antigravity, Codex, and Claude Code.

use std::collections::HashMap;
use std::path::PathBuf;

use orbit_protocol::{
    ActiveTurnBehavior, AgentStatus, ProviderKind, ProviderSubagentDescriptor, QuestionItem,
    ReasoningEffort, SessionMode, TimelinePayload, ToolCallDetail, ToolCallStatus,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Connection parameters for injecting Orbit's built-in MCP tool server into a provider session.
#[derive(Debug, Clone)]
pub struct OrbitMcpLaunchSpec {
    pub server_name: String,
    pub http_url: String,
    pub bearer_token: String,
    pub stdio_command: PathBuf,
    pub stdio_args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SessionLaunchConfig {
    pub agent_id: String,
    pub provider: ProviderKind,
    pub executable: PathBuf,
    pub orbit_exe: PathBuf,
    pub shim_socket: PathBuf,
    pub cwd: PathBuf,
    pub extra_dirs: Vec<PathBuf>,
    pub attachments_dir: Option<PathBuf>,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    pub mode: SessionMode,
    pub resume_session_id: Option<String>,
    pub mcp_spec: Option<OrbitMcpLaunchSpec>,
    pub system_prompt_append: Option<String>,
    pub extra_env: HashMap<String, String>,
    /// When `true`, attempt to reattach to an already-running `orbit shim` UDS before spawning.
    pub reattach_existing_shim: bool,
}

#[derive(Debug, Clone)]
pub enum DriverCommand {
    SendPrompt {
        turn_id: String,
        prompt: String,
        behavior: ActiveTurnBehavior,
    },
    Interrupt,
    SetMode {
        mode: SessionMode,
    },
    SetConfig {
        model: Option<String>,
        effort: Option<ReasoningEffort>,
    },
    ResolvePermission {
        request_id: String,
        /// `"allow"` | `"allow_always"` | `"deny"`
        decision: String,
    },
    AnswerQuestion {
        request_id: String,
        answers: Vec<Vec<String>>,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum DriverEvent {
    SessionInitialized {
        persistence_ref: String,
        model: Option<String>,
    },
    StatusChanged(AgentStatus),
    ThinkingText {
        item_id: String,
        text: String,
    },
    AssistantText {
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
    QuestionRequest {
        request_id: String,
        questions: Vec<QuestionItem>,
    },
    Notice {
        level: String,
        message: String,
    },
    ProviderSubagentUpsert(ProviderSubagentDescriptor),
    ProviderSubagentTimeline {
        subagent_id: String,
        payload: TimelinePayload,
    },
    Exited {
        code: Option<i32>,
    },
}

pub struct ProviderSessionHandle {
    pub cmd_tx: mpsc::Sender<DriverCommand>,
    pub evt_rx: mpsc::Receiver<DriverEvent>,
}

/// Untyped JSON-RPC 2.0 request/response ID (`number` or `string`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    Num(i64),
    Str(String),
}

impl RpcId {
    pub fn as_key(&self) -> String {
        match self {
            Self::Num(n) => format!("n:{n}"),
            Self::Str(s) => format!("s:{s}"),
        }
    }
}
