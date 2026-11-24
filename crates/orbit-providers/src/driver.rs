//! Unified Actor interface (`DriverCommand` / `DriverEvent`) shared by Antigravity, Codex, and Claude Code.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageDeliveryStatus, AgentStatus, BackgroundTaskInfo,
    PlanApprovalAction, PlanApprovalPayload, ProviderKind, ProviderSubagentDescriptor,
    QuestionItem, ReasoningEffort, SessionMode, TimelinePayload, ToolCallDetail, ToolCallStatus,
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

/// Persisted still-`Running` background task tool call from SQLite `timeline_events`,
/// passed into drivers on reattach/resume so historical finished tasks are reconciled cleanly
/// and genuinely still-running tasks retain their canonical `call_id` mapping across restarts.
#[derive(Debug, Clone)]
pub struct PersistedBackgroundTask {
    pub call_id: String,
    pub detail: ToolCallDetail,
    pub info: BackgroundTaskInfo,
}

#[derive(Debug, Clone)]
pub struct SessionLaunchConfig {
    pub agent_id: String,
    pub provider: ProviderKind,
    pub executable: PathBuf,
    pub orbit_exe: PathBuf,
    pub shim_socket: PathBuf,
    /// Resolved private runtime directory (`$ORBIT_HOME/runtime`) for ephemeral per-session files
    /// (such as private `0600` Claude MCP configs).
    pub runtime_dir: PathBuf,
    pub cwd: PathBuf,
    pub extra_dirs: Vec<PathBuf>,
    pub attachments_dir: Option<PathBuf>,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    pub fast_mode: bool,
    pub mode: SessionMode,
    pub resume_session_id: Option<String>,
    /// Optional upstream session ID to natively fork from on startup (full-tail forks only).
    /// When unsupported or evicted upstream, drivers fall back cleanly to a fresh session seeded
    /// with `fallback_handoff_context` (unless `native_fork_only` is set).
    pub fork_from_session_id: Option<String>,
    /// When `true` and `fork_from_session_id` is `Some`, exit cleanly without emitting
    /// `SessionInitialized` if native fork is unavailable or returns the parent session ID,
    /// avoiding persisting a blank fallback session before the user sends a prompt.
    pub native_fork_only: bool,
    pub mcp_spec: Option<OrbitMcpLaunchSpec>,
    pub system_prompt_append: Option<String>,
    pub extra_env: HashMap<String, String>,
    /// When `true`, attempt to reattach to an already-running `orbit shim` UDS before spawning.
    pub reattach_existing_shim: bool,
    /// Last consumed `orbit-shim` Stdio frame sequence number (`Attach { after_seq }`) on reattach.
    pub shim_after_seq: Option<u64>,
    /// Lock-free tracker updated by drivers with the latest consumed `orbit-shim` frame sequence.
    pub shim_seq_tracker: Arc<AtomicU64>,
    /// Active foreground `turn_id` persisted in SQLite (`AgentStatus::Running { turn_id }`) when reattaching.
    pub persisted_active_turn_id: Option<String>,
    /// Highest persisted stream segment suffix (`_N`) for `persisted_active_turn_id` when reattaching.
    pub persisted_turn_step_seq: u64,
    /// Compact conversation handoff context used only if resuming a persisted upstream session fails
    /// because the provider evicted/deleted its local session cache and Orbit falls back to a fresh session.
    pub fallback_handoff_context: Option<String>,
    /// Previously persisted `Running` background task tool calls for reconciliation on startup.
    pub persisted_running_bg_tasks: Vec<PersistedBackgroundTask>,
    /// True when the persisted `AgentStatus` in SQLite was `AutonomousWakeup { source: "waiting:..." }`.
    pub persisted_bg_wait: bool,
    /// Optional ACP registry agent ID when `provider == ProviderKind::Acp`.
    pub acp_agent_id: Option<String>,
    /// Optional pre-resolved `AcpAgentSpec` when `provider == ProviderKind::Acp`.
    pub acp_spec: Option<orbit_protocol::AcpAgentSpec>,
}

/// Origin of a `DriverCommand::SendPrompt` so provider drivers can distinguish human preemption
/// from non-interrupting agent/peer notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptOrigin {
    #[default]
    Human,
    Agent,
}

/// Prompt entry held in a provider driver's FIFO queue while waiting for an active turn to settle.
#[derive(Debug, Clone)]
pub struct QueuedDriverPrompt {
    pub turn_id: String,
    pub prompt: String,
    pub message_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum DriverCommand {
    SendPrompt {
        turn_id: String,
        prompt: String,
        behavior: ActiveTurnBehavior,
        origin: PromptOrigin,
        message_ids: Vec<String>,
    },
    /// Hard user stop: cancels the active turn and drops all pending queued prompts.
    Interrupt,
    /// Cancels only the active foreground turn while preserving queued FIFO notifications/prompts
    /// so the driver drains them immediately once settlement completes.
    InterruptAndDrainQueue,
    SetMode {
        mode: SessionMode,
    },
    SetConfig {
        model: Option<String>,
        effort: Option<ReasoningEffort>,
        fast_mode: Option<bool>,
    },
    ResolvePermission {
        request_id: String,
        /// `"allow"` | `"allow_always"` | `"deny"`
        decision: String,
    },
    ResolvePlanApproval {
        request_id: String,
        action: PlanApprovalAction,
        edited_plan_markdown: Option<String>,
        feedback: Option<String>,
        target_mode: SessionMode,
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
    /// Provider-advertised session title update (e.g. ACP `session_info_update`).
    SessionTitleUpdated {
        title: String,
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
        background_task: Option<BackgroundTaskInfo>,
    },
    PermissionRequest {
        request_id: String,
        title: String,
        detail: ToolCallDetail,
        allowed_decisions: Vec<String>,
        plan_approval: Option<PlanApprovalPayload>,
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
    /// Settle stale historical `Running` background-task tool calls in place without appending
    /// out-of-order cards to the latest turn.
    ReconcileBackgroundTasks(Vec<(String, ToolCallStatus)>),
    /// Provider-reported token telemetry for a completed turn.
    TurnUsage {
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
        /// Instantaneous prompt token count of the most recent single LLM API call in the turn
        /// (distinct from cumulative multi-step turn sums).
        last_call_prompt_tokens: Option<u64>,
        /// Authoritative model context window size reported by the provider, when available.
        context_window_tokens: Option<u64>,
        model: Option<String>,
    },
    /// Authoritative delivery lifecycle transition for queued/dispatched/settled/dropped
    /// agent-originated messages & finish notifications.
    PromptDeliveryUpdated {
        turn_id: String,
        message_ids: Vec<String>,
        status: AgentMessageDeliveryStatus,
        blocking_turn_id: Option<String>,
    },
    Exited {
        code: Option<i32>,
    },
}

/// Emit `DriverEvent::PromptDeliveryUpdated` when `message_ids` is non-empty.
pub async fn emit_prompt_delivery(
    evt_tx: &mpsc::Sender<DriverEvent>,
    turn_id: &str,
    message_ids: &[String],
    status: AgentMessageDeliveryStatus,
    blocking_turn_id: Option<String>,
) {
    if message_ids.is_empty() {
        return;
    }
    let _ = evt_tx
        .send(DriverEvent::PromptDeliveryUpdated {
            turn_id: turn_id.to_string(),
            message_ids: message_ids.to_vec(),
            status,
            blocking_turn_id,
        })
        .await;
}

/// Emit `Dropped` for all pending queued prompts being discarded on hard `Interrupt` or exit.
pub async fn emit_dropped_queued_prompts(
    evt_tx: &mpsc::Sender<DriverEvent>,
    replacement: Option<QueuedDriverPrompt>,
    queue: &mut VecDeque<QueuedDriverPrompt>,
) {
    if let Some(rep) = replacement {
        emit_prompt_delivery(
            evt_tx,
            &rep.turn_id,
            &rep.message_ids,
            AgentMessageDeliveryStatus::Dropped,
            None,
        )
        .await;
    }
    while let Some(item) = queue.pop_front() {
        emit_prompt_delivery(
            evt_tx,
            &item.turn_id,
            &item.message_ids,
            AgentMessageDeliveryStatus::Dropped,
            None,
        )
        .await;
    }
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
