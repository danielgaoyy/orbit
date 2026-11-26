//! Platform-independent host, conversation and timeline projections.
use crate::contract::GitReposInspectionView;
use crate::render::{
    DiffFileView, RichBlockView, parse_markdown_blocks, parse_unified_diff_capped,
};
use orbit_protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostConnectionState {
    #[default]
    Disconnected,
    ConnectingDirect,
    ConnectingRelay,
    OnlineDirect,
    OnlineRelay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRouteMode {
    #[default]
    Auto,
    SshTunnel,
    DirectOnly,
    RelayOnly,
}

impl HostRouteMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::SshTunnel => "ssh_tunnel",
            Self::DirectOnly => "direct_only",
            Self::RelayOnly => "relay_only",
        }
    }

    pub fn from_opt_str(raw: Option<&str>) -> Self {
        match raw {
            Some("ssh_tunnel") => Self::SshTunnel,
            Some("direct_only") => Self::DirectOnly,
            Some("relay_only") => Self::RelayOnly,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedHostProfile {
    pub host_id: String,
    pub hostname: String,
    pub custom_name: Option<String>,
    pub ssh_alias: Option<String>,
    pub route_mode: HostRouteMode,
    pub remote_daemon_port: u16,
    pub custom_direct_endpoint: Option<String>,
    pub daemon_pubkey_b64: String,
    pub direct_endpoints: Vec<String>,
    pub relay_url: Option<String>,
    #[serde(default)]
    pub pairing_secret: String,
    #[serde(default)]
    pub pairing_secret_updated_at_ms: i64,
    pub state: HostConnectionState,
}

impl SavedHostProfile {
    pub fn effective_display_name(&self) -> String {
        if let Some(ref cn) = self.custom_name
            && !cn.trim().is_empty()
        {
            return cn.trim().to_string();
        }
        if let Some(ref alias) = self.ssh_alias
            && !alias.trim().is_empty()
        {
            return alias.trim().to_string();
        }
        self.hostname.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientProjectView {
    pub project_id: String,
    pub host_id: String,
    pub name: String,
    pub directories: Vec<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryListingView {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub entries: Vec<DirectoryEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostCardView {
    pub host_id: String,
    pub display_name: String,
    pub hostname: String,
    pub custom_name: Option<String>,
    pub ssh_alias: Option<String>,
    pub route_mode: HostRouteMode,
    pub remote_daemon_port: u16,
    pub custom_direct_endpoint: Option<String>,
    pub via_ssh_tunnel: bool,
    pub last_ping_ms: Option<u32>,
    pub daemon_version: Option<String>,
    #[serde(default)]
    pub daemon_protocol_version: Option<u32>,
    #[serde(default)]
    pub client_protocol_version: u32,
    pub os_arch: Option<String>,
    pub daemon_pubkey_b64: String,
    pub direct_endpoints: Vec<String>,
    pub relay_url: Option<String>,
    pub connection_state: HostConnectionState,
    pub last_error: Option<String>,
    pub available_providers: Vec<ProviderKind>,
    pub workspace_count: u32,
    pub agent_count: u32,
    pub pending_approval_count: u32,
    #[serde(default)]
    pub supports_server_queue: bool,
    #[serde(default)]
    pub is_local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetOverviewView {
    pub revision: u64,
    pub total_pending_approvals: u32,
    pub hosts: Vec<HostCardView>,
    pub projects: Vec<ClientProjectView>,
    #[serde(default)]
    pub project_order: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCardView {
    pub agent_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub title: String,
    pub provider: ProviderKind,
    pub provider_label: String,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub fast_mode: bool,
    pub mode: SessionMode,
    pub status: AgentStatus,
    pub status_label: String,
    pub is_running: bool,
    #[serde(default)]
    pub has_background_tasks: bool,
    #[serde(default)]
    pub has_armed_timers: bool,
    #[serde(default)]
    pub has_running_subagents: bool,
    pub is_awaiting_permission: bool,
    pub parent_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_host_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub auto_archive: bool,
    #[serde(default)]
    pub attention: AttentionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at_rfc3339: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
    pub latest_seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_rfc3339: Option<String>,
    pub updated_at_rfc3339: String,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub seen_seq: u64,
    #[serde(default)]
    pub read_seq: u64,
    #[serde(default)]
    pub is_unread: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queued_prompts: Vec<QueuedPromptView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_suggestion: Option<WorkspaceClusterSuggestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_at_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_agent_id: Option<String>,
}

impl AgentCardView {
    /// Resolve canonical session sovereignty tier (`Conversation`, `Subagent`, `PersonalRouter`, `Automation`).
    pub fn session_tier(&self) -> SessionTier {
        resolve_session_tier(
            self.background,
            self.parent_agent_id.as_deref(),
            &self.labels,
        )
    }

    /// True if this session is a managed subagent/helper that must remain hidden from primary conversation lists.
    pub fn is_subagent(&self) -> bool {
        self.session_tier() == SessionTier::Subagent
    }

    /// True if this session is a user-visible primary `Conversation`.
    pub fn is_primary_conversation(&self) -> bool {
        self.session_tier() == SessionTier::Conversation
    }

    /// Router agent ID if this conversation was dispatched by a `PersonalRouter`.
    pub fn routed_by_agent_id(&self) -> Option<&str> {
        self.labels
            .get(LABEL_ROUTED_BY_AGENT)
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostDetailView {
    pub revision: u64,
    pub host: HostCardView,
    pub discovered_projects: Vec<DiscoveredProject>,
    pub directory_listing: Option<DirectoryListingView>,
    pub models_by_provider: HashMap<ProviderKind, Vec<ModelOption>>,
    pub workspaces: Vec<WorkspaceSummary>,
    pub agents: Vec<AgentCardView>,
    pub last_created_agent_id: Option<String>,
    pub replaced_optimistic_agents: HashMap<String, String>,
    pub quotas: Vec<ProviderQuotaSnapshot>,
    #[serde(default)]
    pub provider_catalogs: Vec<ProviderModelCatalog>,
    pub terminals: Vec<TerminalSummary>,
    pub active_terminal_id: Option<String>,
    pub active_terminal_slot: u8,
    #[serde(default)]
    pub profiles: Vec<AgentProfile>,
    #[serde(default)]
    pub schedules: Vec<ScheduleSpec>,
    #[serde(default)]
    pub conditional_timers: Vec<ConditionalTimerRecord>,
    #[serde(default)]
    pub connected_hosts: Vec<ConnectedHostSummary>,
    #[serde(default)]
    pub orchestration_config: OrchestrationConfig,
    #[serde(default)]
    pub orchestration_skills: Vec<OrchestrationSkillInfo>,
    #[serde(default)]
    pub managed_skills: Vec<ManagedSkillRecord>,
    #[serde(default)]
    pub keep_awake_status: Option<KeepAwakeStatus>,
    #[serde(default)]
    pub workspace_scripts: HashMap<String, Vec<WorkspaceScriptInfo>>,
    #[serde(default)]
    pub pending_permissions: Vec<PendingPermissionSummary>,
    #[serde(default)]
    pub provider_subagents: HashMap<String, Vec<ProviderSubagentDescriptor>>,
    #[serde(default)]
    pub git_repos_by_cwd: HashMap<String, GitReposInspectionView>,
    #[serde(default)]
    pub workspace_boards: HashMap<String, WorkspaceBoardSnapshot>,
    #[serde(default)]
    pub workspace_events: HashMap<String, Vec<WorkspaceEventRecord>>,
    #[serde(default)]
    pub workspace_organize_status: HashMap<String, WorkspaceOrganizeEvent>,
    #[serde(default)]
    pub host_sync_state: HostSyncState,
    #[serde(default)]
    pub acp_agents: Vec<AcpAgentAvailability>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactSummary>,
    #[serde(default)]
    pub goal_runs: Vec<GoalRunRecord>,
    #[serde(default)]
    pub monitor_watches: Vec<MonitorWatchRecord>,
    #[serde(default)]
    pub workflow_runs: Vec<WorkflowRunRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "item_type", rename_all = "snake_case")]
pub enum TimelineItemView {
    UserPrompt {
        seq: u64,
        message_id: String,
        text: String,
        #[serde(default)]
        badges: Vec<PromptBadge>,
        blocks: Vec<RichBlockView>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    AssistantThinking {
        seq: u64,
        item_id: String,
        text: String,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    AssistantMessage {
        seq: u64,
        item_id: String,
        text: String,
        streaming: bool,
        blocks: Vec<RichBlockView>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    ToolCall {
        seq: u64,
        call_id: String,
        status: ToolCallStatus,
        summary_title: String,
        detail: ToolCallDetail,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        background_task: Option<BackgroundTaskInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        step_descriptor: Option<crate::stage::ToolStepDescriptorView>,
        diff_files: Vec<DiffFileView>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    PermissionRequest {
        seq: u64,
        request_id: String,
        title: String,
        detail: ToolCallDetail,
        allowed_decisions: Vec<String>,
        resolved_decision: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        guardian_assessment: Option<GuardianAssessment>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan_approval: Option<PlanApprovalPayload>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    QuestionRequest {
        seq: u64,
        request_id: String,
        questions: Vec<QuestionItem>,
        resolved_answers: Option<Vec<Vec<String>>>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    Notice {
        seq: u64,
        level: String,
        message: String,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
    AgentMessage {
        seq: u64,
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
        blocks: Vec<RichBlockView>,
        #[serde(default)]
        timestamp_ms: Option<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskSideQuestionRequest {
    pub host_id: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    pub cwd: String,
    pub provider: ProviderKind,
    #[serde(default)]
    pub model: Option<String>,
    pub question: String,
    #[serde(default)]
    pub context_summary: String,
}

pub(crate) fn normalize_cwd_trim(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed == "/" {
        "/"
    } else {
        trimmed.trim_end_matches('/')
    }
}

pub(crate) fn cwd_paths_equivalent(stored_cwd: &str, query_cwd: &str) -> bool {
    let a = normalize_cwd_trim(stored_cwd);
    let b = normalize_cwd_trim(query_cwd);
    if a == b {
        return true;
    }
    if let Some(rel) = b.strip_prefix("~/") {
        let suffix = format!("/{rel}");
        if a.ends_with(&suffix) {
            return true;
        }
    }
    if let Some(rel) = a.strip_prefix("~/") {
        let suffix = format!("/{rel}");
        if b.ends_with(&suffix) {
            return true;
        }
    }
    false
}

pub(crate) fn truncate_utf8_prefix(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

pub(crate) fn truncate_utf8_tail(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut start = text.len().saturating_sub(max_bytes);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_string()
}

pub(crate) fn strip_segment_scope(id: &str) -> &str {
    id.split_once("@s").map(|(base, _)| base).unwrap_or(id)
}

pub(crate) fn is_legacy_shared_item_id(raw_id: &str) -> bool {
    matches!(
        raw_id,
        "agy_msg_default" | "agy_thought_default" | "codex_msg" | "codex_rsn" | "item"
    )
}

pub(crate) fn is_pending_optimistic_prompt(ev: &TimelineEvent) -> bool {
    ev.seq == 0 && matches!(ev.payload, TimelinePayload::UserPrompt { .. })
}

pub(crate) fn coalesce_timeline_slice(events: &[TimelineEvent]) -> Vec<TimelineEvent> {
    let mut coalesced = Vec::with_capacity(events.len());
    for ev in events {
        coalesce_or_push_timeline(&mut coalesced, ev.clone());
    }
    coalesced
}

pub(crate) fn coalesce_or_push_timeline(
    list: &mut Vec<TimelineEvent>,
    mut incoming: TimelineEvent,
) -> usize {
    let active_end = list
        .iter()
        .position(is_pending_optimistic_prompt)
        .unwrap_or(list.len());
    let precedes_incoming =
        |ev: &TimelineEvent| -> bool { ev.seq > 0 && (incoming.seq == 0 || ev.seq < incoming.seq) };

    let turn_start_idx = list[..active_end]
        .iter()
        .rposition(|ev| {
            precedes_incoming(ev)
                && matches!(
                    ev.payload,
                    TimelinePayload::UserPrompt { .. } | TimelinePayload::AgentMessage { .. }
                )
        })
        .map(|i| i + 1)
        .unwrap_or(0);

    let seg_start_idx = list[..active_end]
        .iter()
        .rposition(|ev| {
            precedes_incoming(ev)
                && matches!(
                    ev.payload,
                    TimelinePayload::UserPrompt { .. }
                        | TimelinePayload::AgentMessage { .. }
                        | TimelinePayload::ToolCall { .. }
                )
        })
        .map(|i| i + 1)
        .unwrap_or(0);

    let seg_boundary_seq = if seg_start_idx > 0 {
        list[seg_start_idx - 1].seq
    } else {
        0
    };

    match &mut incoming.payload {
        TimelinePayload::UserPrompt {
            text: inc_text,
            message_id: inc_mid,
            ..
        } => {
            if incoming.seq > 0 {
                // Replace optimistic `seq == 0` UserPrompt with matching message_id first, then text
                if !inc_mid.is_empty()
                    && let Some((idx, opt_slot)) = list.iter_mut().enumerate().find(|(_, ev)| {
                        ev.seq == 0
                            && matches!(
                                &ev.payload,
                                TimelinePayload::UserPrompt { message_id, .. } if message_id == inc_mid
                            )
                    })
                {
                    *opt_slot = incoming;
                    return idx;
                }
                if let Some((idx, opt_slot)) = list.iter_mut().enumerate().find(|(_, ev)| {
                    ev.seq == 0
                        && matches!(
                            &ev.payload,
                            TimelinePayload::UserPrompt { text, .. } if text.trim() == inc_text.trim()
                        )
                }) {
                    *opt_slot = incoming;
                    return idx;
                }
            }
            if !inc_mid.is_empty()
                && let Some((idx, dup_slot)) = list.iter_mut().enumerate().find(|(_, ev)| {
                    matches!(
                        &ev.payload,
                        TimelinePayload::UserPrompt { message_id, .. } if message_id == inc_mid
                    )
                })
            {
                *dup_slot = incoming;
                return idx;
            }
        }
        TimelinePayload::AssistantMessage {
            item_id,
            text,
            streaming,
        } if !item_id.is_empty() => {
            let raw_id = strip_segment_scope(item_id).to_string();
            if is_legacy_shared_item_id(&raw_id) {
                let effective_id = format!("{raw_id}@s{seg_boundary_seq}");
                *item_id = effective_id.clone();

                if let Some((rel_idx, prev)) = list[seg_start_idx..active_end]
                    .iter_mut()
                    .enumerate()
                    .rev()
                    .find(|(_, prev_ev)| {
                        matches!(
                            &prev_ev.payload,
                            TimelinePayload::AssistantMessage { item_id: existing_id, .. }
                                if existing_id == &effective_id
                        )
                    })
                {
                    *prev = incoming;
                    return seg_start_idx + rel_idx;
                }
            } else {
                *item_id = raw_id.clone();
                if let Some((idx, prev)) =
                    list[..active_end]
                        .iter_mut()
                        .enumerate()
                        .rev()
                        .find(|(_, prev_ev)| {
                            matches!(
                                &prev_ev.payload,
                                TimelinePayload::AssistantMessage { item_id: existing_id, .. }
                                    if strip_segment_scope(existing_id) == raw_id
                            )
                        })
                {
                    if idx >= seg_start_idx {
                        *prev = incoming;
                    } else if let TimelinePayload::AssistantMessage {
                        text: prev_text,
                        streaming: prev_streaming,
                        ..
                    } = &mut prev.payload
                    {
                        // Cross-segment / cross-turn in-place update: preserve original slot
                        // before any later UserPrompt/ToolCall and settle streaming cleanly.
                        if incoming.seq == 0 || prev.seq == 0 || incoming.seq >= prev.seq {
                            if !text.is_empty() || prev_text.is_empty() {
                                *prev_text = std::mem::take(text);
                            }
                            *prev_streaming = if idx < turn_start_idx {
                                false
                            } else {
                                *streaming
                            };
                        }
                    }
                    return idx;
                }
            }
        }
        TimelinePayload::AssistantThinking { item_id, text } if !item_id.is_empty() => {
            let raw_id = strip_segment_scope(item_id).to_string();
            if is_legacy_shared_item_id(&raw_id) {
                let effective_id = format!("{raw_id}@s{seg_boundary_seq}");
                *item_id = effective_id.clone();

                if let Some((rel_idx, prev)) = list[seg_start_idx..active_end]
                    .iter_mut()
                    .enumerate()
                    .rev()
                    .find(|(_, prev_ev)| {
                        matches!(
                            &prev_ev.payload,
                            TimelinePayload::AssistantThinking { item_id: existing_id, .. }
                                if existing_id == &effective_id
                        )
                    })
                {
                    *prev = incoming;
                    return seg_start_idx + rel_idx;
                }
            } else {
                *item_id = raw_id.clone();
                if let Some((idx, prev)) =
                    list[..active_end]
                        .iter_mut()
                        .enumerate()
                        .rev()
                        .find(|(_, prev_ev)| {
                            matches!(
                                &prev_ev.payload,
                                TimelinePayload::AssistantThinking { item_id: existing_id, .. }
                                    if strip_segment_scope(existing_id) == raw_id
                            )
                        })
                {
                    if idx >= seg_start_idx {
                        *prev = incoming;
                    } else if let TimelinePayload::AssistantThinking {
                        text: prev_text, ..
                    } = &mut prev.payload
                        && (incoming.seq == 0 || prev.seq == 0 || incoming.seq >= prev.seq)
                        && (!text.is_empty() || prev_text.is_empty())
                    {
                        *prev_text = std::mem::take(text);
                    }
                    return idx;
                }
            }
        }
        TimelinePayload::ToolCall { call_id, .. } if !call_id.is_empty() => {
            let target_call_id = call_id.clone();
            if let Some((rel_idx, prev)) = list[turn_start_idx..active_end]
                .iter_mut()
                .enumerate()
                .rev()
                .find(|(_, prev_ev)| {
                    matches!(
                        &prev_ev.payload,
                        TimelinePayload::ToolCall { call_id: existing_id, .. }
                            if existing_id == &target_call_id
                    )
                })
            {
                let preserved_ts = if prev.timestamp <= incoming.timestamp {
                    prev.timestamp
                } else {
                    incoming.timestamp
                };
                incoming.payload =
                    orbit_protocol::merge_tool_call_payload(prev.payload.clone(), incoming.payload);
                incoming.timestamp = preserved_ts;
                *prev = incoming;
                return turn_start_idx + rel_idx;
            }
        }
        _ => {}
    }

    if incoming.seq > 0 {
        if let Some((idx, same_seq)) = list
            .iter_mut()
            .enumerate()
            .find(|(_, e)| e.seq == incoming.seq)
        {
            *same_seq = incoming;
            return idx;
        }
        if let Some(first_opt_idx) = list.iter().position(is_pending_optimistic_prompt) {
            list.insert(first_opt_idx, incoming);
            return first_opt_idx;
        }
    }
    list.push(incoming);
    list.len() - 1
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AgentMessageDeliveryFold {
    pub(crate) delivery_status: Option<AgentMessageDeliveryStatus>,
    pub(crate) delivery_turn_id: Option<String>,
    pub(crate) blocking_turn_id: Option<String>,
    pub(crate) blocking_turn_cleared: bool,
    pub(crate) queued_at_ms: Option<i64>,
    pub(crate) dispatched_at_ms: Option<i64>,
    pub(crate) settled_at_ms: Option<i64>,
}

pub(crate) fn delivery_status_rank(status: AgentMessageDeliveryStatus) -> u8 {
    match status {
        AgentMessageDeliveryStatus::Queued => 0,
        AgentMessageDeliveryStatus::Dispatched => 1,
        AgentMessageDeliveryStatus::Settled | AgentMessageDeliveryStatus::Dropped => 2,
    }
}

pub(crate) fn collect_delivery_updates(
    events: &[TimelineEvent],
) -> HashMap<String, AgentMessageDeliveryFold> {
    let mut map: HashMap<String, AgentMessageDeliveryFold> = HashMap::new();
    for ev in events {
        match &ev.payload {
            TimelinePayload::AgentMessage {
                message_id,
                delivery_status,
                delivery_turn_id,
                blocking_turn_id,
                queued_at_ms,
                dispatched_at_ms,
                settled_at_ms,
                ..
            } => {
                let entry = map.entry(message_id.clone()).or_default();
                if let Some(st) = delivery_status
                    && entry
                        .delivery_status
                        .is_none_or(|prev| delivery_status_rank(*st) > delivery_status_rank(prev))
                {
                    entry.delivery_status = Some(*st);
                }
                if entry.delivery_turn_id.is_none() && delivery_turn_id.is_some() {
                    entry.delivery_turn_id = delivery_turn_id.clone();
                }
                if matches!(
                    delivery_status,
                    Some(
                        AgentMessageDeliveryStatus::Dispatched
                            | AgentMessageDeliveryStatus::Settled
                            | AgentMessageDeliveryStatus::Dropped
                    )
                ) {
                    entry.blocking_turn_id = None;
                    entry.blocking_turn_cleared = true;
                } else if !entry.blocking_turn_cleared
                    && entry.blocking_turn_id.is_none()
                    && blocking_turn_id.is_some()
                {
                    entry.blocking_turn_id = blocking_turn_id.clone();
                }
                if entry.queued_at_ms.is_none() {
                    entry.queued_at_ms = *queued_at_ms;
                }
                if entry.dispatched_at_ms.is_none() {
                    entry.dispatched_at_ms = *dispatched_at_ms;
                }
                if entry.settled_at_ms.is_none() {
                    entry.settled_at_ms = *settled_at_ms;
                }
            }
            TimelinePayload::AgentMessageDeliveryUpdated {
                message_id,
                status,
                delivery_turn_id,
                blocking_turn_id,
                timestamp_ms,
            } => {
                let entry = map.entry(message_id.clone()).or_default();
                if entry
                    .delivery_status
                    .is_none_or(|prev| delivery_status_rank(*status) > delivery_status_rank(prev))
                {
                    entry.delivery_status = Some(*status);
                }
                if delivery_turn_id.is_some() {
                    entry.delivery_turn_id = delivery_turn_id.clone();
                }
                if matches!(
                    status,
                    AgentMessageDeliveryStatus::Dispatched
                        | AgentMessageDeliveryStatus::Settled
                        | AgentMessageDeliveryStatus::Dropped
                ) {
                    entry.blocking_turn_id = None;
                    entry.blocking_turn_cleared = true;
                } else if !entry.blocking_turn_cleared
                    && entry.blocking_turn_id.is_none()
                    && blocking_turn_id.is_some()
                {
                    entry.blocking_turn_id = blocking_turn_id.clone();
                }
                match status {
                    AgentMessageDeliveryStatus::Queued => {
                        entry.queued_at_ms.get_or_insert(*timestamp_ms);
                    }
                    AgentMessageDeliveryStatus::Dispatched => {
                        entry.dispatched_at_ms.get_or_insert(*timestamp_ms);
                    }
                    AgentMessageDeliveryStatus::Settled | AgentMessageDeliveryStatus::Dropped => {
                        entry.settled_at_ms.get_or_insert(*timestamp_ms);
                    }
                }
            }
            _ => {}
        }
    }
    map
}

pub(crate) type ResolvedGatesMap = (HashMap<String, String>, HashMap<String, Vec<Vec<String>>>);

pub(crate) fn collect_resolved_gates(events: &[TimelineEvent]) -> ResolvedGatesMap {
    let mut all_resolved_perms: HashMap<String, String> = HashMap::new();
    let mut all_resolved_questions: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    for ev in events {
        match &ev.payload {
            TimelinePayload::PermissionResolved {
                request_id,
                decision,
            } => {
                all_resolved_perms.insert(request_id.clone(), decision.clone());
            }
            TimelinePayload::QuestionResolved {
                request_id,
                answers,
            } => {
                all_resolved_questions.insert(request_id.clone(), answers.clone());
            }
            _ => {}
        }
    }
    (all_resolved_perms, all_resolved_questions)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_timeline_event_view(
    agent_id: &str,
    agent_cwd: Option<&str>,
    ev: &TimelineEvent,
    all_resolved_perms: &HashMap<String, String>,
    all_resolved_questions: &HashMap<String, Vec<Vec<String>>>,
    delivery_updates: &HashMap<String, AgentMessageDeliveryFold>,
    highlight_diff: bool,
    markdown_cache: &mut HashMap<(String, u64, usize), Vec<RichBlockView>>,
    streaming_markdown_cache: &mut HashMap<(String, String), (usize, Vec<RichBlockView>)>,
) -> Option<TimelineItemView> {
    let ts_ms = Some(ev.timestamp.timestamp_millis());
    match &ev.payload {
        TimelinePayload::UserPrompt {
            message_id,
            text,
            badges,
        } => {
            let trimmed = text.trim_start();
            let cleaned_for_blocks = if trimmed.starts_with("<orbit-")
                || trimmed.starts_with("<WORKSPACE_RULES")
                || trimmed.starts_with("<ATTACHED_CONTEXT")
            {
                orbit_protocol::clean_agent_message_display_body(text)
            } else {
                None
            };
            let parse_src = cleaned_for_blocks.as_deref().unwrap_or(text.as_str());
            let key = (agent_id.to_string(), ev.seq, parse_src.len());
            if markdown_cache.len() > 2048 {
                markdown_cache.clear();
            }
            let blocks = markdown_cache
                .entry(key)
                .or_insert_with(|| {
                    crate::render::parse_markdown_blocks_with_math(parse_src, false, false)
                })
                .clone();
            Some(TimelineItemView::UserPrompt {
                seq: ev.seq,
                message_id: message_id.clone(),
                text: text.clone(),
                badges: badges.clone(),
                blocks,
                timestamp_ms: ts_ms,
            })
        }
        TimelinePayload::AssistantThinking { item_id, text } => {
            Some(TimelineItemView::AssistantThinking {
                seq: ev.seq,
                item_id: item_id.clone(),
                text: text.trim().to_string(),
                timestamp_ms: ts_ms,
            })
        }
        TimelinePayload::AssistantMessage {
            item_id,
            text,
            streaming,
        } => {
            if let Some(warning_msg) = extract_legacy_provider_warning_text(text) {
                Some(TimelineItemView::Notice {
                    seq: ev.seq,
                    level: "warning".into(),
                    message: warning_msg,
                    timestamp_ms: ts_ms,
                })
            } else if *streaming {
                let slot_key = (
                    agent_id.to_string(),
                    if item_id.is_empty() {
                        format!("seq:{}", ev.seq)
                    } else {
                        item_id.clone()
                    },
                );
                let blocks = match streaming_markdown_cache.get(&slot_key) {
                    Some((cached_len, cached_blocks)) if *cached_len == text.len() => {
                        cached_blocks.clone()
                    }
                    _ => {
                        let parsed = parse_markdown_blocks(text, true);
                        if streaming_markdown_cache.len() > 64 {
                            streaming_markdown_cache.clear();
                        }
                        streaming_markdown_cache.insert(slot_key, (text.len(), parsed.clone()));
                        parsed
                    }
                };
                Some(TimelineItemView::AssistantMessage {
                    seq: ev.seq,
                    item_id: item_id.clone(),
                    text: text.clone(),
                    streaming: true,
                    blocks,
                    timestamp_ms: ts_ms,
                })
            } else {
                if !item_id.is_empty() {
                    streaming_markdown_cache.remove(&(agent_id.to_string(), item_id.clone()));
                }
                let key = (agent_id.to_string(), ev.seq, text.len());
                if markdown_cache.len() > 2048 {
                    markdown_cache.clear();
                }
                let blocks = markdown_cache
                    .entry(key)
                    .or_insert_with(|| parse_markdown_blocks(text, false))
                    .clone();
                Some(TimelineItemView::AssistantMessage {
                    seq: ev.seq,
                    item_id: item_id.clone(),
                    text: text.clone(),
                    streaming: false,
                    blocks,
                    timestamp_ms: ts_ms,
                })
            }
        }
        TimelinePayload::ToolCall {
            call_id,
            status,
            detail,
            background_task,
        } => {
            let (summary_title, diff_files) = summarize_tool_detail(detail, highlight_diff);
            let step_descriptor = Some(crate::stage::describe_tool_detail(
                *status,
                &summary_title,
                detail,
                &diff_files,
                agent_cwd,
            ));
            Some(TimelineItemView::ToolCall {
                seq: ev.seq,
                call_id: call_id.clone(),
                status: *status,
                summary_title,
                detail: slim_tool_detail_for_stage(detail),
                background_task: background_task.clone(),
                step_descriptor,
                diff_files,
                timestamp_ms: ts_ms,
            })
        }
        TimelinePayload::PermissionRequest {
            request_id,
            title,
            detail,
            allowed_decisions,
            guardian_assessment,
            plan_approval,
        } => Some(TimelineItemView::PermissionRequest {
            seq: ev.seq,
            request_id: request_id.clone(),
            title: title.clone(),
            detail: detail.clone(),
            allowed_decisions: allowed_decisions.clone(),
            resolved_decision: all_resolved_perms.get(request_id).cloned(),
            guardian_assessment: guardian_assessment.clone(),
            plan_approval: plan_approval.clone(),
            timestamp_ms: ts_ms,
        }),
        TimelinePayload::QuestionRequest {
            request_id,
            questions,
        } => Some(TimelineItemView::QuestionRequest {
            seq: ev.seq,
            request_id: request_id.clone(),
            questions: questions.clone(),
            resolved_answers: all_resolved_questions.get(request_id).cloned(),
            timestamp_ms: ts_ms,
        }),
        TimelinePayload::Notice { level, message } => Some(TimelineItemView::Notice {
            seq: ev.seq,
            level: level.clone(),
            message: message.clone(),
            timestamp_ms: ts_ms,
        }),
        TimelinePayload::AgentMessage {
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
        } => {
            let cleaned_for_blocks = if *kind == orbit_protocol::AgentMessageKind::DirectMessage
                && permission_context.is_none()
            {
                orbit_protocol::clean_agent_message_display_body(text)
            } else {
                None
            };
            let parse_src = cleaned_for_blocks.as_deref().unwrap_or(text.as_str());
            let key = (agent_id.to_string(), ev.seq, parse_src.len());
            if markdown_cache.len() > 2048 {
                markdown_cache.clear();
            }
            let blocks = markdown_cache
                .entry(key)
                .or_insert_with(|| {
                    crate::render::parse_markdown_blocks_with_math(parse_src, false, false)
                })
                .clone();
            let folded = delivery_updates.get(message_id);
            let resolved_status = match (delivery_status, folded.and_then(|f| f.delivery_status)) {
                (Some(a), Some(b)) => {
                    if delivery_status_rank(b) >= delivery_status_rank(*a) {
                        Some(b)
                    } else {
                        Some(*a)
                    }
                }
                (Some(a), None) => Some(*a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            Some(TimelineItemView::AgentMessage {
                seq: ev.seq,
                message_id: message_id.clone(),
                kind: *kind,
                source_agent_id: source_agent_id.clone(),
                source_host_id: source_host_id.clone(),
                source_title: source_title.clone(),
                event: event.clone(),
                text: text.clone(),
                permission_context: permission_context.clone(),
                delivery_status: resolved_status,
                delivery_turn_id: folded
                    .and_then(|f| f.delivery_turn_id.clone())
                    .or_else(|| delivery_turn_id.clone()),
                blocking_turn_id: if folded.is_some_and(|f| f.blocking_turn_cleared) {
                    None
                } else {
                    folded
                        .and_then(|f| f.blocking_turn_id.clone())
                        .or_else(|| blocking_turn_id.clone())
                },
                queued_at_ms: queued_at_ms.or_else(|| folded.and_then(|f| f.queued_at_ms)),
                dispatched_at_ms: folded
                    .and_then(|f| f.dispatched_at_ms)
                    .or(*dispatched_at_ms),
                settled_at_ms: folded.and_then(|f| f.settled_at_ms).or(*settled_at_ms),
                blocks,
                timestamp_ms: ts_ms,
            })
        }
        TimelinePayload::StatusChanged {
            status: AgentStatus::Error { message, .. },
        } => (!message.trim().is_empty()).then(|| TimelineItemView::Notice {
            seq: ev.seq,
            level: "error".into(),
            message: message.clone(),
            timestamp_ms: ts_ms,
        }),
        TimelinePayload::StatusChanged {
            status: AgentStatus::AutonomousWakeup { source, .. },
        } => {
            // `waiting:*` transitions represent passive background-task wait state, not a new
            // wakeup boundary: the stage's trailing live indicator already renders
            // `Waiting for N background tasks…` directly from `currentAgent.status_label`, so
            // emitting a `WakeupDivider` here would duplicate the exact same text right above it.
            if source.trim().starts_with("waiting:") {
                None
            } else {
                Some(TimelineItemView::Notice {
                    seq: ev.seq,
                    level: "wakeup".into(),
                    message: format_autonomous_wakeup_notice(source),
                    timestamp_ms: ts_ms,
                })
            }
        }
        TimelinePayload::PermissionResolved { .. }
        | TimelinePayload::QuestionResolved { .. }
        | TimelinePayload::AgentMessageDeliveryUpdated { .. }
        | TimelinePayload::StatusChanged { .. } => None,
    }
}

pub(crate) fn extract_legacy_provider_warning_text(text: &str) -> Option<String> {
    if !text.contains('⚠') {
        return None;
    }
    let mut warnings = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let rest = trimmed.strip_prefix('⚠')?.trim();
        let lower = rest.to_ascii_lowercase();
        let is_runtime_warning = lower.starts_with("api error")
            || lower.starts_with("the stream was interrupted")
            || lower.starts_with("rate limit")
            || lower.starts_with("request failed")
            || lower.starts_with("connection ")
            || lower.starts_with("network ")
            || lower.starts_with("retrying")
            || lower.starts_with("error:");
        if !is_runtime_warning {
            return None;
        }
        warnings.push(rest);
    }
    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join("\n"))
    }
}

pub(crate) fn format_autonomous_wakeup_notice(source: &str) -> String {
    let trimmed = source.trim();
    if let Some(rest) = trimmed.strip_prefix("waiting:") {
        let desc = rest.trim();
        return if desc.is_empty() {
            "Waiting for background task(s)".to_string()
        } else {
            format!("Waiting for {desc}")
        };
    }
    let cleaned = trimmed.replace('_', " ");
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("background task") {
        "Resumed from background task".to_string()
    } else {
        format!("Resumed from {cleaned}")
    }
}

pub(crate) fn summarize_tool_detail(
    detail: &ToolCallDetail,
    highlight_diff: bool,
) -> (String, Vec<DiffFileView>) {
    match detail {
        ToolCallDetail::Shell { command, .. } => (format!("$ {command}"), Vec::new()),
        ToolCallDetail::FileRead { path, .. } => (format!("Read {path}"), Vec::new()),
        ToolCallDetail::FileEdit { path, unified_diff } => {
            let diffs = unified_diff
                .as_deref()
                .map(|raw| parse_unified_diff_capped(raw, 80, highlight_diff))
                .unwrap_or_default();
            (format!("Edit {path}"), diffs)
        }
        ToolCallDetail::Search { query, .. } => (format!("Search \"{query}\""), Vec::new()),
        ToolCallDetail::Subagent { role, .. } => (format!("Subagent · {role}"), Vec::new()),
        ToolCallDetail::Generic { tool_name, .. } => (tool_name.clone(), Vec::new()),
    }
}

pub(crate) fn slim_tool_detail_for_stage(detail: &ToolCallDetail) -> ToolCallDetail {
    const MAX_STAGE_DIFF_BYTES: usize = 8192;
    const MAX_STAGE_SHELL_PREVIEW_BYTES: usize = 8192;
    const MAX_STAGE_GENERIC_ARGS_BYTES: usize = 4096;

    match detail {
        ToolCallDetail::FileEdit { path, unified_diff } => ToolCallDetail::FileEdit {
            path: path.clone(),
            unified_diff: unified_diff.as_deref().map(|raw| {
                if raw.len() <= MAX_STAGE_DIFF_BYTES {
                    raw.to_string()
                } else {
                    let prefix = truncate_utf8_prefix(raw, MAX_STAGE_DIFF_BYTES);
                    format!("{prefix}\n... (truncated)")
                }
            }),
        },
        ToolCallDetail::Shell {
            command,
            cwd,
            exit_code,
            output_preview,
        } => ToolCallDetail::Shell {
            command: command.clone(),
            cwd: cwd.clone(),
            exit_code: *exit_code,
            output_preview: output_preview.as_deref().map(|raw| {
                if raw.len() <= MAX_STAGE_SHELL_PREVIEW_BYTES {
                    raw.to_string()
                } else {
                    let tail = truncate_utf8_tail(raw, MAX_STAGE_SHELL_PREVIEW_BYTES);
                    format!("... (truncated)\n{tail}")
                }
            }),
        },
        ToolCallDetail::Generic {
            tool_name,
            arguments_json,
        } => ToolCallDetail::Generic {
            tool_name: tool_name.clone(),
            arguments_json: if arguments_json.len() <= MAX_STAGE_GENERIC_ARGS_BYTES {
                arguments_json.clone()
            } else {
                let prefix = truncate_utf8_prefix(arguments_json, MAX_STAGE_GENERIC_ARGS_BYTES);
                format!("{prefix}\n... (truncated)")
            },
        },
        other => other.clone(),
    }
}

pub fn project_agent_card(
    a: &AgentSummary,
    local_seen_seq: u64,
    local_pin: bool,
    has_running_subagents: bool,
    has_armed_conditional_timer: bool,
) -> AgentCardView {
    let (status_label, is_running, has_background_tasks, is_awaiting_permission) = match &a.status {
        AgentStatus::Initializing => ("Starting".to_string(), true, false, false),
        AgentStatus::Idle => ("Idle".to_string(), false, false, false),
        AgentStatus::Running { .. } => ("Working".to_string(), true, false, false),
        AgentStatus::AutonomousWakeup { source, .. } => {
            if let Some(rest) = source.trim().strip_prefix("waiting:") {
                let desc = rest.trim();
                let label = if desc.is_empty() {
                    "Waiting for background tasks".to_string()
                } else if desc.to_ascii_lowercase().starts_with("waiting") {
                    desc.to_string()
                } else {
                    format!("Waiting for {desc}")
                };
                (label, false, true, false)
            } else {
                (format!("Waking ({source})"), true, false, false)
            }
        }
        AgentStatus::AwaitingPermission { .. } => {
            ("Needs approval".to_string(), false, false, true)
        }
        AgentStatus::Error { .. } => ("Error".to_string(), false, false, false),
        AgentStatus::Closed => ("Closed".to_string(), false, false, false),
    };
    let has_armed_timers = has_background_tasks || has_armed_conditional_timer;
    let effective_seen_seq = a.read_seq.max(local_seen_seq);
    let is_pinned = a.is_pinned || local_pin;
    let is_unread = !a.agent_id.starts_with("opt_")
        && !is_running
        && (a.attention != AttentionState::None
            || (a.latest_seq > 0 && a.latest_seq > effective_seen_seq));
    let worktree_branch = a.labels.get("orbit.worktree_branch").cloned();
    let provider_label = if a.provider == ProviderKind::Acp {
        a.acp_agent_id
            .clone()
            .unwrap_or_else(|| a.provider.display_name().to_string())
    } else {
        a.provider.display_name().to_string()
    };
    AgentCardView {
        agent_id: a.agent_id.clone(),
        workspace_id: a.workspace_id.clone(),
        cwd: a.cwd.clone(),
        title: a.title.clone(),
        provider: a.provider,
        provider_label,
        model: a.model.clone(),
        effort: a.effort,
        fast_mode: a.fast_mode,
        mode: a.mode.clone(),
        status: a.status.clone(),
        status_label,
        is_running,
        has_background_tasks,
        has_armed_timers,
        has_running_subagents,
        is_awaiting_permission,
        parent_agent_id: a.parent_agent_id.clone(),
        parent_host_id: a.parent_host_id.clone(),
        profile_id: a.profile_id.clone(),
        background: a.background,
        auto_archive: a.auto_archive,
        attention: a.attention,
        archived_at_rfc3339: a.archived_at.map(|ts| ts.to_rfc3339()),
        labels: a.labels.clone(),
        latest_seq: a.latest_seq,
        created_at_rfc3339: Some(a.created_at.to_rfc3339()),
        updated_at_rfc3339: a.updated_at.to_rfc3339(),
        is_pinned,
        seen_seq: effective_seen_seq,
        read_seq: a.read_seq,
        is_unread,
        queued_prompts: a.queued_prompts.clone(),
        handle: a.handle.clone(),
        cluster_suggestion: a.cluster_suggestion.clone(),
        worktree_branch,
        task_summary: a.task_summary.clone(),
        forked_from_agent_id: a.forked_from_agent_id.clone(),
        forked_at_seq: a.forked_at_seq,
        acp_agent_id: a.acp_agent_id.clone(),
    }
}

pub fn project_timeline_items(
    events: &[TimelineEvent],
    cwd: Option<&str>,
) -> Vec<TimelineItemView> {
    let events = coalesce_timeline_slice(events);
    let (permissions, questions) = collect_resolved_gates(&events);
    let delivery = collect_delivery_updates(&events);
    let mut markdown = HashMap::new();
    let mut streaming = HashMap::new();
    events
        .iter()
        .filter_map(|ev| {
            render_timeline_event_view(
                &ev.agent_id,
                cwd,
                ev,
                &permissions,
                &questions,
                &delivery,
                true,
                &mut markdown,
                &mut streaming,
            )
        })
        .collect()
}

pub fn project_fleet_overview(
    revision: u64,
    mut hosts: Vec<HostCardView>,
    mut projects: Vec<ClientProjectView>,
    project_order: Vec<String>,
) -> FleetOverviewView {
    hosts.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    projects.sort_by_key(|p| std::cmp::Reverse(p.updated_at_ms));
    let total_pending_approvals = hosts.iter().map(|h| h.pending_approval_count).sum();
    FleetOverviewView {
        revision,
        hosts,
        projects,
        project_order,
        total_pending_approvals,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitReviewView {
    pub revision: u64,
    pub host_id: String,
    pub cwd: String,
    pub status: Option<GitStatusSnapshot>,
    pub selected_path: Option<String>,
    pub raw_diff: String,
    pub diff_files: Vec<DiffFileView>,
}

pub fn project_git_review(
    rev: u64,
    host_id: &str,
    cwd: &str,
    path: Option<&str>,
    raw_diff: &str,
    status: Option<GitStatusSnapshot>,
) -> GitReviewView {
    let diff_files = crate::render::parse_unified_diff(raw_diff);
    let capped_raw_diff = truncate_utf8_prefix(raw_diff, 32_768);
    GitReviewView {
        revision: rev,
        host_id: host_id.to_string(),
        cwd: cwd.to_string(),
        status,
        selected_path: path.map(ToOwned::to_owned),
        raw_diff: capped_raw_diff,
        diff_files,
    }
}
