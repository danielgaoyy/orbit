//! Semantic conversation turn coalescing, tool step descriptors, thinking headline extraction,
//! session insights derivation, and work-summary bucketing shared across Desktop, iOS (SwiftUI),
//! Android (Compose), and HarmonyOS (ArkUI V2).

use std::collections::HashMap;

use orbit_protocol::{GitStatusSnapshot, ToolCallDetail, ToolCallStatus};
use serde::{Deserialize, Serialize};

use crate::app_core::{AgentCardView, TimelineItemView};
use crate::render::DiffFileView;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnFileDiffSummaryView {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnWorkCountsView {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSummaryResultView {
    pub counts: TurnWorkCountsView,
    /// Settled collapsed summary label (e.g. `Edited 2 files, ran a command`, `Read omni-composer.tsx`, `Thought for 6s`).
    pub label: String,
    /// Active streaming summary label while steps are still in flight (e.g. `Reading omni-composer.tsx…`, `Thinking`).
    pub live_label: String,
    /// Optional muted secondary context for live status (e.g. active thinking headline).
    #[serde(default)]
    pub live_detail: Option<String>,
    /// Whether the primary `label` already embeds failure wording (e.g. `Command failed`).
    pub label_includes_failure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnCompactionItemView {
    pub seq: u64,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStepKind {
    FileRead,
    FileCreate,
    FileEdit,
    Search,
    Shell,
    Subagent,
    Generic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStepDescriptorView {
    pub kind: ToolStepKind,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnLiveSegmentView {
    StepGroup {
        key: String,
        first_seq: u64,
        items: Vec<TimelineItemView>,
        summary: WorkSummaryResultView,
        #[serde(default)]
        thought_duration_ms: Option<i64>,
        #[serde(default)]
        work_duration_ms: Option<i64>,
        #[serde(default)]
        started_at_ms: Option<i64>,
        #[serde(default)]
        latest_activity_ms: Option<i64>,
        /// True when subsequent prose or another segment has already started after this step batch.
        has_following_content: bool,
        /// True if any tool call inside this group is currently `running`.
        has_running_step: bool,
    },
    AssistantMessage {
        key: String,
        item: TimelineItemView,
    },
    Notice {
        key: String,
        item: TimelineItemView,
    },
    Compaction {
        key: String,
        label: String,
    },
    Wakeup {
        key: String,
        label: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTurnView {
    pub key: String,
    pub turn_index: u32,
    pub anchor_seq: u64,
    pub user_prompt: Option<TimelineItemView>,
    #[serde(default)]
    pub agent_message: Option<TimelineItemView>,
    pub is_compaction_command: bool,
    /// Intermediate steps prior to `terminal_reply` (eligible for pre-reply auto-fold).
    pub folded_steps: Vec<TimelineItemView>,
    /// Aggregated counts of work performed inside `folded_steps`.
    pub work_counts: TurnWorkCountsView,
    /// Settled summary sentence for `folded_steps` (`Edited 2 files, ran a command`).
    pub work_summary_label: String,
    /// Streaming summary label while steps in `folded_steps` are in flight.
    pub work_summary_live_label: String,
    /// Optional muted secondary detail for live status (e.g. active thinking headline).
    #[serde(default)]
    pub work_summary_live_detail: Option<String>,
    /// Whether `work_summary_label` already states failure (`Command failed`).
    pub work_summary_includes_failure: bool,
    /// Duration of thinking blocks in milliseconds when >= 1000ms.
    pub thought_duration_ms: Option<i64>,
    /// Computed work duration in ms across `folded_steps` up to `terminal_reply` when >= 1000ms.
    #[serde(default)]
    pub work_duration_ms: Option<i64>,
    /// End-to-end turn duration in milliseconds when timestamps are available.
    pub turn_duration_ms: Option<i64>,
    /// Final synthesis reply of the turn, kept visible when pre-reply work folds.
    pub terminal_reply: Option<TimelineItemView>,
    /// Most recent non-empty assistant message anywhere in the turn (used for Copy action).
    #[serde(default)]
    pub last_assistant_reply: Option<TimelineItemView>,
    /// Notices (warnings/errors) surfaced unconditionally so system alerts are never buried.
    pub notices: Vec<TimelineItemView>,
    /// Context compaction notices preserved in chronological turn context.
    #[serde(default)]
    pub compactions: Vec<TurnCompactionItemView>,
    /// Chronological segments used while the turn has not yet reached a terminal reply (or when unfolded).
    pub live_segments: Vec<TurnLiveSegmentView>,
    /// Deduplicated per-file diff stats touched during this turn.
    pub turn_diffs: Vec<TurnFileDiffSummaryView>,
    /// Whether any tool call in this turn failed or was denied.
    pub has_error: bool,
    /// Whether the turn ended on an unrecovered tool failure/denial without subsequent success.
    pub has_unrecovered_error: bool,
    /// Whether any tool call or message in this turn is currently in flight.
    pub has_active_item: bool,
    /// Epoch milliseconds when this turn began (user prompt or first body item).
    pub started_at_ms: Option<i64>,
    /// Epoch milliseconds when this turn completed (or latest body event timestamp).
    pub completed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTouchedFileAction {
    Created,
    Edited,
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTouchedFileView {
    pub path: String,
    pub display_path: String,
    pub action: SessionTouchedFileAction,
    pub additions: u32,
    pub deletions: u32,
    pub last_seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionBackgroundTaskKind {
    Shell,
    Wakeup,
    Queued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionBackgroundTaskStatus {
    Running,
    Queued,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionBackgroundTaskView {
    pub id: String,
    pub kind: SessionBackgroundTaskKind,
    pub title: String,
    pub subtitle: String,
    pub status: SessionBackgroundTaskStatus,
    pub started_at_ms: Option<i64>,
    pub command: Option<String>,
    pub output_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSubagentEntryView {
    pub id: String,
    pub role: String,
    pub prompt_preview: String,
    pub status: ToolCallStatus,
    pub child_agent_id: Option<String>,
    pub last_seq: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInsightsView {
    pub touched_files: Vec<SessionTouchedFileView>,
    pub background_tasks: Vec<SessionBackgroundTaskView>,
    pub subagents: Vec<SessionSubagentEntryView>,
    pub detected_local_urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStageView {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<AgentCardView>,
    pub turns: Vec<StageTurnView>,
    pub pending_permissions: Vec<TimelineItemView>,
    pub pending_questions: Vec<TimelineItemView>,
    pub session_turn_diffs: Vec<DiffFileView>,
    #[serde(default)]
    pub insights: SessionInsightsView,
    pub latest_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStageDeltaView {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<AgentCardView>,
    pub replace_from_turn_index: u32,
    pub changed_turns: Vec<StageTurnView>,
    pub total_turn_count: u32,
    pub pending_permissions: Vec<TimelineItemView>,
    pub pending_questions: Vec<TimelineItemView>,
    pub session_turn_diffs: Vec<DiffFileView>,
    #[serde(default)]
    pub insights: SessionInsightsView,
    pub latest_seq: u64,
    #[serde(default)]
    pub reset: bool,
    #[serde(default)]
    pub aux_revision: u64,
    #[serde(default)]
    pub aux_unchanged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGenericToolName {
    pub server_name: Option<String>,
    pub action_label: String,
    pub display_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSubkind {
    Web,
    Glob,
    Grep,
}

pub fn timeline_item_seq(item: &TimelineItemView) -> u64 {
    match item {
        TimelineItemView::UserPrompt { seq, .. }
        | TimelineItemView::AgentMessage { seq, .. }
        | TimelineItemView::AssistantThinking { seq, .. }
        | TimelineItemView::AssistantMessage { seq, .. }
        | TimelineItemView::ToolCall { seq, .. }
        | TimelineItemView::PermissionRequest { seq, .. }
        | TimelineItemView::QuestionRequest { seq, .. }
        | TimelineItemView::Notice { seq, .. } => *seq,
    }
}

/// Returns a rendering key that stays invariant as a coalesced streaming item's `seq` advances.
pub fn stable_timeline_item_key(item: &TimelineItemView) -> String {
    match item {
        TimelineItemView::UserPrompt {
            message_id, seq, ..
        } => {
            if !message_id.is_empty() {
                format!("prompt-{message_id}")
            } else {
                format!("prompt-seq-{seq}")
            }
        }
        TimelineItemView::AgentMessage {
            message_id, seq, ..
        } => {
            if !message_id.is_empty() {
                format!("am-{message_id}")
            } else {
                format!("am-seq-{seq}")
            }
        }
        TimelineItemView::AssistantThinking { item_id, seq, .. } => {
            if !item_id.is_empty() {
                format!("think-{item_id}")
            } else {
                format!("think-seq-{seq}")
            }
        }
        TimelineItemView::AssistantMessage { item_id, seq, .. } => {
            if !item_id.is_empty() {
                format!("msg-{item_id}")
            } else {
                format!("msg-seq-{seq}")
            }
        }
        TimelineItemView::ToolCall { call_id, seq, .. } => {
            if !call_id.is_empty() {
                format!("tool-{call_id}")
            } else {
                format!("tool-seq-{seq}")
            }
        }
        TimelineItemView::PermissionRequest {
            request_id, seq, ..
        } => {
            if !request_id.is_empty() {
                format!("perm-{request_id}")
            } else {
                format!("perm-seq-{seq}")
            }
        }
        TimelineItemView::QuestionRequest {
            request_id, seq, ..
        } => {
            if !request_id.is_empty() {
                format!("q-{request_id}")
            } else {
                format!("q-seq-{seq}")
            }
        }
        TimelineItemView::Notice { seq, .. } => format!("notice-{seq}"),
    }
}

pub fn timeline_item_timestamp_ms(item: &TimelineItemView) -> Option<i64> {
    let raw = match item {
        TimelineItemView::UserPrompt { timestamp_ms, .. }
        | TimelineItemView::AgentMessage { timestamp_ms, .. }
        | TimelineItemView::AssistantThinking { timestamp_ms, .. }
        | TimelineItemView::AssistantMessage { timestamp_ms, .. }
        | TimelineItemView::ToolCall { timestamp_ms, .. }
        | TimelineItemView::PermissionRequest { timestamp_ms, .. }
        | TimelineItemView::QuestionRequest { timestamp_ms, .. }
        | TimelineItemView::Notice { timestamp_ms, .. } => *timestamp_ms,
    };
    raw.filter(|ts| *ts > 0)
}

pub fn strip_workspace_prefix(file_path: &str, cwd: Option<&str>) -> String {
    if file_path.is_empty() {
        return String::new();
    }
    let clean_cwd = cwd.unwrap_or("").trim_end_matches('/');
    if !clean_cwd.is_empty()
        && clean_cwd != "~"
        && let Some(rest) = file_path.strip_prefix(clean_cwd)
        && let Some(rel) = rest.strip_prefix('/')
    {
        return rel.to_string();
    }
    file_path.to_string()
}

pub fn basename_of_path(file_path: &str) -> String {
    if file_path.is_empty() {
        return String::new();
    }
    let trimmed = file_path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

pub fn format_duration_compact(duration_ms: i64) -> String {
    let total_seconds = ((duration_ms as f64) / 1000.0).round().max(1.0) as i64;
    if total_seconds < 60 {
        return format!("{total_seconds}s");
    }
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if minutes >= 60 {
        let hours = minutes / 60;
        let rem_min = minutes % 60;
        if rem_min > 0 {
            format!("{hours}h {rem_min}m")
        } else {
            format!("{hours}h")
        }
    } else if seconds > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{minutes}m")
    }
}

pub fn format_thought_summary_label(duration_ms: Option<i64>, thought_count: u32) -> String {
    if let Some(ms) = duration_ms {
        let rounded_sec = ((ms as f64) / 1000.0).round() as i64;
        if rounded_sec >= 1 {
            return format!("Thought for {}", format_duration_compact(ms));
        }
    }
    if thought_count > 1 {
        "Thought process".to_string()
    } else {
        "Thought about it".to_string()
    }
}

pub fn extract_thinking_headline(raw_text: &str, prefer_latest: bool) -> String {
    if raw_text.is_empty() {
        return String::new();
    }
    let mut cleaned_lines: Vec<String> = Vec::new();
    let mut in_fence = false;

    for raw_line in raw_text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || trimmed.is_empty() {
            continue;
        }

        let mut plain = strip_markdown_line_prefix(trimmed);
        plain = strip_markdown_links(&plain);
        plain = plain
            .chars()
            .filter(|c| !matches!(c, '*' | '_' | '`' | '~'))
            .collect::<String>();
        let collapsed = plain.split_whitespace().collect::<Vec<_>>().join(" ");
        if !collapsed.is_empty() {
            cleaned_lines.push(collapsed);
            if !prefer_latest {
                break;
            }
        }
    }

    let chosen = if prefer_latest {
        cleaned_lines.last().cloned().unwrap_or_default()
    } else {
        cleaned_lines.into_iter().next().unwrap_or_default()
    };

    if chosen.chars().count() > 88 {
        let truncated: String = chosen.chars().take(87).collect();
        format!("{truncated}…")
    } else {
        chosen
    }
}

pub fn parse_generic_tool_name(raw_name: &str) -> ParsedGenericToolName {
    let trimmed = raw_name.trim();
    if trimmed.is_empty() {
        return ParsedGenericToolName {
            server_name: None,
            action_label: "tool".to_string(),
            display_label: "Tool".to_string(),
        };
    }

    if let Some(after_mcp) = trimmed.strip_prefix("mcp__") {
        let parts: Vec<&str> = after_mcp.split("__").filter(|s| !s.is_empty()).collect();
        if parts.len() >= 2 {
            let server_name = format_server_title(parts[0]);
            let action_raw = parts[1..].join(" ");
            let action_label = humanize_identifier(&action_raw).to_lowercase();
            let display_label = format!("{server_name}: {action_label}");
            return ParsedGenericToolName {
                server_name: Some(server_name),
                action_label,
                display_label,
            };
        }
    }

    if let Some(colon_idx) = trimmed.find(':')
        && colon_idx > 0
        && colon_idx < trimmed.len() - 1
    {
        let server_name = format_server_title(&trimmed[..colon_idx]);
        let action_label = humanize_identifier(&trimmed[colon_idx + 1..]).to_lowercase();
        let display_label = format!("{server_name}: {action_label}");
        return ParsedGenericToolName {
            server_name: Some(server_name),
            action_label,
            display_label,
        };
    }

    let humanized = humanize_identifier(trimmed);
    ParsedGenericToolName {
        server_name: None,
        action_label: humanized.to_lowercase(),
        display_label: humanized,
    }
}

pub fn classify_search_tool(query: &str, summary_title: &str) -> SearchSubkind {
    let title_lower = summary_title.trim().to_lowercase();
    let q_trim = query.trim();
    if ["web", "internet", "google", "bing", "url"]
        .iter()
        .any(|w| contains_word(&title_lower, w))
        || starts_with_ignore_ascii_case(q_trim, "http://")
        || starts_with_ignore_ascii_case(q_trim, "https://")
    {
        return SearchSubkind::Web;
    }

    if ["glob", "find files", "file pattern", "list files"]
        .iter()
        .any(|w| contains_word(&title_lower, w))
        || ((q_trim.starts_with("**/") || q_trim.starts_with("*/"))
            && !q_trim.chars().any(char::is_whitespace))
    {
        return SearchSubkind::Glob;
    }

    SearchSubkind::Grep
}

pub fn is_file_create_detail(summary_title: &str, detail: &ToolCallDetail) -> bool {
    let ToolCallDetail::FileEdit { unified_diff, .. } = detail else {
        return false;
    };
    let title_lower = summary_title.trim().to_lowercase();
    for kw in ["create", "created", "write", "wrote"] {
        if let Some(rest) = title_lower.strip_prefix(kw)
            && (rest.is_empty()
                || rest
                    .chars()
                    .next()
                    .is_some_and(|c| !c.is_alphanumeric() && c != '_'))
        {
            return true;
        }
    }
    if let Some(diff) = unified_diff {
        for line in diff.lines() {
            if let Some(after) = line.strip_prefix("---") {
                let trimmed = after.trim_start();
                if trimmed != after && trimmed.starts_with("/dev/null") {
                    return true;
                }
            }
        }
    }
    false
}

pub fn describe_tool_detail(
    status: ToolCallStatus,
    summary_title: &str,
    detail: &ToolCallDetail,
    diff_files: &[DiffFileView],
    cwd: Option<&str>,
) -> ToolStepDescriptorView {
    let is_running = status == ToolCallStatus::Running;
    let is_failed = status == ToolCallStatus::Failed;
    let is_denied = status == ToolCallStatus::Denied;

    let additions: u32 = diff_files.iter().map(|f| f.additions).sum();
    let deletions: u32 = diff_files.iter().map(|f| f.deletions).sum();
    let has_first_diff_lines = diff_files.first().is_some_and(|f| !f.lines.is_empty());

    match detail {
        ToolCallDetail::FileRead {
            path,
            start_line,
            end_line,
        } => {
            let rel_raw = strip_workspace_prefix(path, cwd);
            let target = if rel_raw.is_empty() {
                "file".to_string()
            } else {
                rel_raw
            };
            let secondary_meta = match (start_line, end_line) {
                (Some(s), Some(e)) if *s > 0 && *e > 0 => Some(format!("L{s}–{e}")),
                (Some(s), _) if *s > 0 => Some(format!("from L{s}")),
                _ => None,
            };
            let verb = if is_denied {
                "Declined read"
            } else if is_failed {
                "Couldn’t read"
            } else if is_running {
                "Reading"
            } else {
                "Read"
            };
            ToolStepDescriptorView {
                kind: ToolStepKind::FileRead,
                verb: verb.to_string(),
                target,
                secondary_meta,
                open_path: if path.is_empty() {
                    None
                } else {
                    Some(path.clone())
                },
                additions,
                deletions,
                exit_code: None,
                is_running,
                is_failed,
                is_denied,
                has_expandable_detail: false,
            }
        }
        ToolCallDetail::FileEdit { path, unified_diff } => {
            let is_create = is_file_create_detail(summary_title, detail);
            let rel_raw = strip_workspace_prefix(path, cwd);
            let target = if rel_raw.is_empty() {
                "file".to_string()
            } else {
                rel_raw
            };
            let verb = if is_denied {
                "Declined edit"
            } else if is_failed {
                if is_create {
                    "Failed to create"
                } else {
                    "Failed to edit"
                }
            } else if is_running {
                if is_create { "Creating" } else { "Editing" }
            } else if is_create {
                "Created"
            } else {
                "Edited"
            };
            ToolStepDescriptorView {
                kind: if is_create {
                    ToolStepKind::FileCreate
                } else {
                    ToolStepKind::FileEdit
                },
                verb: verb.to_string(),
                target,
                secondary_meta: None,
                open_path: if path.is_empty() {
                    None
                } else {
                    Some(path.clone())
                },
                additions,
                deletions,
                exit_code: None,
                is_running,
                is_failed,
                is_denied,
                has_expandable_detail: has_first_diff_lines
                    || unified_diff.as_ref().is_some_and(|u| !u.is_empty()),
            }
        }
        ToolCallDetail::Search { query, scope } => {
            let subkind = classify_search_tool(query, summary_title);
            let verb = match subkind {
                SearchSubkind::Web => {
                    if is_failed {
                        "Failed to search the web"
                    } else if is_running {
                        "Searching the web"
                    } else {
                        "Searched the web"
                    }
                }
                SearchSubkind::Glob => {
                    if is_failed {
                        "Failed to find files"
                    } else if is_running {
                        "Looking for files"
                    } else {
                        "Looked for files"
                    }
                }
                SearchSubkind::Grep => {
                    if is_failed {
                        "Search failed"
                    } else if is_running {
                        "Searching"
                    } else {
                        "Searched"
                    }
                }
            };
            let scope_rel = scope
                .as_deref()
                .map(|s| strip_workspace_prefix(s, cwd))
                .filter(|s| !s.is_empty());
            ToolStepDescriptorView {
                kind: ToolStepKind::Search,
                verb: verb.to_string(),
                target: query.clone(),
                secondary_meta: scope_rel.map(|s| format!("in {s}")),
                open_path: None,
                additions,
                deletions,
                exit_code: None,
                is_running,
                is_failed,
                is_denied,
                has_expandable_detail: false,
            }
        }
        ToolCallDetail::Shell {
            command,
            exit_code,
            output_preview,
            ..
        } => {
            let verb = if is_denied {
                "Stopped running the command"
            } else if is_failed {
                "Command failed"
            } else if is_running {
                "Running"
            } else {
                "Ran"
            };
            ToolStepDescriptorView {
                kind: ToolStepKind::Shell,
                verb: verb.to_string(),
                target: if command.is_empty() {
                    "command".to_string()
                } else {
                    command.clone()
                },
                secondary_meta: None,
                open_path: None,
                additions,
                deletions,
                exit_code: *exit_code,
                is_running,
                is_failed,
                is_denied,
                has_expandable_detail: !command.is_empty()
                    || output_preview.as_ref().is_some_and(|o| !o.is_empty()),
            }
        }
        ToolCallDetail::Subagent {
            role,
            prompt_preview,
            ..
        } => {
            let verb = if is_denied {
                "Stopped subagent"
            } else if is_failed {
                "Subagent failed"
            } else if is_running {
                "Running subagent"
            } else {
                "Ran subagent"
            };
            ToolStepDescriptorView {
                kind: ToolStepKind::Subagent,
                verb: verb.to_string(),
                target: if !role.is_empty() {
                    role.clone()
                } else {
                    prompt_preview.clone()
                },
                secondary_meta: None,
                open_path: None,
                additions,
                deletions,
                exit_code: None,
                is_running,
                is_failed,
                is_denied,
                has_expandable_detail: !prompt_preview.is_empty(),
            }
        }
        ToolCallDetail::Generic {
            tool_name,
            arguments_json,
        } => {
            let parsed = parse_generic_tool_name(tool_name);
            let raw_title = summary_title.trim();
            let target = if !raw_title.is_empty()
                && !raw_title.eq_ignore_ascii_case(tool_name.trim())
                && !raw_title.eq_ignore_ascii_case(&parsed.display_label)
            {
                raw_title.to_string()
            } else {
                String::new()
            };
            let verb = if is_denied {
                format!("Declined {}", parsed.display_label)
            } else if is_failed {
                format!("{} failed", parsed.display_label)
            } else {
                parsed.display_label
            };
            let clean_args = arguments_json.trim();
            let has_non_empty_args =
                !clean_args.is_empty() && clean_args != "{}" && clean_args != "null";
            ToolStepDescriptorView {
                kind: ToolStepKind::Generic,
                verb,
                target,
                secondary_meta: None,
                open_path: None,
                additions,
                deletions,
                exit_code: None,
                is_running,
                is_failed,
                is_denied,
                has_expandable_detail: has_non_empty_args,
            }
        }
    }
}

pub fn describe_tool_step(
    item: &TimelineItemView,
    cwd: Option<&str>,
) -> Option<ToolStepDescriptorView> {
    match item {
        TimelineItemView::ToolCall {
            status,
            summary_title,
            detail,
            diff_files,
            ..
        } => Some(describe_tool_detail(
            *status,
            summary_title,
            detail,
            diff_files,
            cwd,
        )),
        _ => None,
    }
}

pub fn summarize_work_items(
    items: &[TimelineItemView],
    thought_duration_ms: Option<i64>,
) -> WorkSummaryResultView {
    struct ToolRef<'a> {
        status: ToolCallStatus,
        summary_title: &'a str,
        detail: &'a ToolCallDetail,
    }

    let mut tool_calls: Vec<ToolRef<'_>> = Vec::new();
    let mut thought_count: u32 = 0;
    let mut narration_count: u32 = 0;
    let mut permission_count: u32 = 0;
    let mut question_count: u32 = 0;
    let mut additions: u32 = 0;
    let mut deletions: u32 = 0;

    for it in items {
        match it {
            TimelineItemView::AssistantThinking { .. } => {
                thought_count += 1;
            }
            TimelineItemView::AssistantMessage { text, .. } => {
                if !text.trim().is_empty() {
                    narration_count += 1;
                }
            }
            TimelineItemView::PermissionRequest { .. } => {
                permission_count += 1;
            }
            TimelineItemView::QuestionRequest { .. } => {
                question_count += 1;
            }
            TimelineItemView::ToolCall {
                status,
                summary_title,
                detail,
                diff_files,
                ..
            } => {
                tool_calls.push(ToolRef {
                    status: *status,
                    summary_title,
                    detail,
                });
                for df in diff_files {
                    additions += df.additions;
                    deletions += df.deletions;
                }
            }
            _ => {}
        }
    }

    let mut failed_count: u32 = 0;
    let mut denied_count: u32 = 0;
    let mut trailing_failure_streak: u32 = 0;
    for tc in &tool_calls {
        match tc.status {
            ToolCallStatus::Failed => {
                failed_count += 1;
                trailing_failure_streak += 1;
            }
            ToolCallStatus::Denied => {
                denied_count += 1;
                trailing_failure_streak += 1;
            }
            ToolCallStatus::Completed => {
                trailing_failure_streak = 0;
            }
            ToolCallStatus::Running => {}
        }
    }

    let total_tc = tool_calls.len() as u32;
    let all_failed = total_tc > 0 && failed_count + denied_count == total_tc;
    let unrecovered_failure_count = if all_failed {
        failed_count + denied_count
    } else {
        trailing_failure_streak
    };

    let mut edited_paths: Vec<String> = Vec::new();
    let mut edit_pathless: u32 = 0;
    let mut edit_calls: u32 = 0;

    let mut created_paths: Vec<String> = Vec::new();
    let mut create_pathless: u32 = 0;
    let mut create_calls: u32 = 0;

    let mut read_paths: Vec<String> = Vec::new();
    let mut read_pathless: u32 = 0;
    let mut read_calls: u32 = 0;

    let mut command_count: u32 = 0;
    let mut grep_count: u32 = 0;
    let mut glob_count: u32 = 0;
    let mut web_search_count: u32 = 0;
    let mut subagent_count: u32 = 0;
    let mut mcp_servers: Vec<String> = Vec::new();
    let mut mcp_call_count: u32 = 0;
    let mut other_tool_count: u32 = 0;

    let mut first_seen: HashMap<&'static str, usize> = HashMap::new();
    let mut mark_seen = |key: &'static str| {
        let next_order = first_seen.len();
        first_seen.entry(key).or_insert(next_order);
    };

    for tc in &tool_calls {
        match tc.detail {
            ToolCallDetail::FileEdit { path, .. } => {
                if is_file_create_detail(tc.summary_title, tc.detail) {
                    mark_seen("create");
                    create_calls += 1;
                    if !path.is_empty() {
                        push_unique(&mut created_paths, path);
                    } else {
                        create_pathless += 1;
                    }
                } else {
                    mark_seen("edit");
                    edit_calls += 1;
                    if !path.is_empty() {
                        push_unique(&mut edited_paths, path);
                    } else {
                        edit_pathless += 1;
                    }
                }
            }
            ToolCallDetail::FileRead { path, .. } => {
                mark_seen("read");
                read_calls += 1;
                if !path.is_empty() {
                    push_unique(&mut read_paths, path);
                } else {
                    read_pathless += 1;
                }
            }
            ToolCallDetail::Search { query, .. } => {
                match classify_search_tool(query, tc.summary_title) {
                    SearchSubkind::Web => {
                        mark_seen("search_web");
                        web_search_count += 1;
                    }
                    SearchSubkind::Glob => {
                        mark_seen("look_files");
                        glob_count += 1;
                    }
                    SearchSubkind::Grep => {
                        mark_seen("search_files");
                        grep_count += 1;
                    }
                }
            }
            ToolCallDetail::Shell { .. } => {
                mark_seen("shell");
                command_count += 1;
            }
            ToolCallDetail::Subagent { .. } => {
                mark_seen("subagent");
                subagent_count += 1;
            }
            ToolCallDetail::Generic { tool_name, .. } => {
                let parsed = parse_generic_tool_name(tool_name);
                if let Some(server) = parsed.server_name {
                    mark_seen("mcp");
                    mcp_call_count += 1;
                    push_unique(&mut mcp_servers, &server);
                } else {
                    mark_seen("other");
                    other_tool_count += 1;
                }
            }
        }
    }

    let edited_file_count = edited_paths.len() as u32 + edit_pathless;
    let created_file_count = created_paths.len() as u32 + create_pathless;
    let read_file_count = read_paths.len() as u32 + read_pathless;
    let search_count = grep_count + glob_count + web_search_count;
    let mcp_server_count = mcp_servers.len() as u32;
    let action_steps = total_tc;
    let total_steps = action_steps + thought_count + permission_count + question_count;

    let active_tail = items
        .iter()
        .rev()
        .find(|it| {
            matches!(
                it,
                TimelineItemView::ToolCall {
                    status: ToolCallStatus::Running,
                    ..
                }
            )
        })
        .or_else(|| items.last());
    let (live_label, live_detail) = build_live_step_status(active_tail);

    let mut label_includes_failure = false;
    let label = if tool_calls.is_empty() {
        if thought_count > 0 {
            format_thought_summary_label(thought_duration_ms, thought_count)
        } else if question_count > 0 {
            if question_count == 1 {
                "Asked a question".to_string()
            } else {
                format!("Asked {question_count} questions")
            }
        } else if permission_count > 0 {
            "Checked permissions".to_string()
        } else if narration_count > 0 {
            if narration_count == 1 {
                "1 note".to_string()
            } else {
                format!("{narration_count} notes")
            }
        } else {
            "Completed steps".to_string()
        }
    } else if tool_calls.len() == 1 {
        let single = &tool_calls[0];
        let (single_label, inc_fail) =
            summarize_single_tool_call(single.status, single.summary_title, single.detail);
        label_includes_failure = inc_fail;
        single_label
    } else if denied_count == total_tc {
        label_includes_failure = true;
        "You declined".to_string()
    } else {
        struct Bucket {
            label: String,
            tier: u8,
            raw_call_count: u32,
            first_order: usize,
        }

        let is_sole_category = first_seen.len() == 1;
        let mut buckets: Vec<Bucket> = Vec::new();

        if edited_file_count > 0 {
            let sole_name = if is_sole_category && edited_paths.len() == 1 {
                basename_of_path(&edited_paths[0])
            } else {
                String::new()
            };
            let lbl = if !sole_name.is_empty() {
                format!("Edited {sole_name}")
            } else if edited_file_count == 1 {
                "Edited a file".to_string()
            } else {
                format!("Edited {edited_file_count} files")
            };
            buckets.push(Bucket {
                label: lbl,
                tier: 1,
                raw_call_count: edit_calls,
                first_order: first_seen.get("edit").copied().unwrap_or(0),
            });
        }

        if created_file_count > 0 {
            let sole_name = if is_sole_category && created_paths.len() == 1 {
                basename_of_path(&created_paths[0])
            } else {
                String::new()
            };
            let lbl = if !sole_name.is_empty() {
                format!("Created {sole_name}")
            } else if created_file_count == 1 {
                "Created a file".to_string()
            } else {
                format!("Created {created_file_count} files")
            };
            buckets.push(Bucket {
                label: lbl,
                tier: 1,
                raw_call_count: create_calls,
                first_order: first_seen.get("create").copied().unwrap_or(0),
            });
        }

        if subagent_count > 0 {
            buckets.push(Bucket {
                label: if subagent_count == 1 {
                    "Ran an agent".to_string()
                } else {
                    format!("Ran {subagent_count} agents")
                },
                tier: 2,
                raw_call_count: subagent_count,
                first_order: first_seen.get("subagent").copied().unwrap_or(0),
            });
        }

        if mcp_server_count > 0 {
            let first_server = mcp_servers
                .first()
                .map(String::as_str)
                .unwrap_or("connector");
            buckets.push(Bucket {
                label: if mcp_server_count == 1 {
                    format!("Used {first_server} integration")
                } else {
                    format!("Used {mcp_server_count} integrations")
                },
                tier: 2,
                raw_call_count: mcp_call_count,
                first_order: first_seen.get("mcp").copied().unwrap_or(0),
            });
        }

        if read_file_count > 0 {
            let sole_name = if is_sole_category && read_paths.len() == 1 {
                basename_of_path(&read_paths[0])
            } else {
                String::new()
            };
            let lbl = if !sole_name.is_empty() {
                format!("Read {sole_name}")
            } else if read_file_count == 1 {
                "Read a file".to_string()
            } else {
                format!("Read {read_file_count} files")
            };
            buckets.push(Bucket {
                label: lbl,
                tier: 3,
                raw_call_count: read_calls,
                first_order: first_seen.get("read").copied().unwrap_or(0),
            });
        }

        if grep_count > 0 {
            buckets.push(Bucket {
                label: "Searched files".to_string(),
                tier: 3,
                raw_call_count: grep_count,
                first_order: first_seen.get("search_files").copied().unwrap_or(0),
            });
        }

        if glob_count > 0 {
            buckets.push(Bucket {
                label: "Looked for files".to_string(),
                tier: 3,
                raw_call_count: glob_count,
                first_order: first_seen.get("look_files").copied().unwrap_or(0),
            });
        }

        if web_search_count > 0 {
            buckets.push(Bucket {
                label: "Searched the web".to_string(),
                tier: 3,
                raw_call_count: web_search_count,
                first_order: first_seen.get("search_web").copied().unwrap_or(0),
            });
        }

        if command_count > 0 {
            buckets.push(Bucket {
                label: if command_count == 1 {
                    "Ran a command".to_string()
                } else {
                    format!("Ran {command_count} commands")
                },
                tier: 4,
                raw_call_count: command_count,
                first_order: first_seen.get("shell").copied().unwrap_or(0),
            });
        }

        if other_tool_count > 0 {
            buckets.push(Bucket {
                label: if other_tool_count == 1 {
                    "Used a tool".to_string()
                } else {
                    format!("Used {other_tool_count} tools")
                },
                tier: 4,
                raw_call_count: other_tool_count,
                first_order: first_seen.get("other").copied().unwrap_or(0),
            });
        }

        buckets.sort_by(|a, b| {
            a.tier
                .cmp(&b.tier)
                .then_with(|| b.raw_call_count.cmp(&a.raw_call_count))
                .then_with(|| a.first_order.cmp(&b.first_order))
        });

        let visible_limit = if buckets.len() > 3 { 2 } else { 3 };
        let shown_len = buckets.len().min(visible_limit);
        let hidden_steps: u32 = buckets[shown_len..].iter().map(|b| b.raw_call_count).sum();
        let formatted_shown = buckets[..shown_len]
            .iter()
            .enumerate()
            .map(|(idx, b)| {
                if idx == 0 {
                    b.label.clone()
                } else {
                    lowercase_clause_lead(&b.label)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        if shown_len > 0 && hidden_steps > 0 {
            let noun = if hidden_steps == 1 { "step" } else { "steps" };
            format!("{formatted_shown}, and {hidden_steps} more {noun}")
        } else if !formatted_shown.is_empty() {
            formatted_shown
        } else {
            format!("Used {total_tc} tools")
        }
    };

    WorkSummaryResultView {
        counts: TurnWorkCountsView {
            edited_file_count,
            created_file_count,
            command_count,
            read_file_count,
            search_count,
            subagent_count,
            mcp_server_count,
            other_tool_count,
            thought_count,
            narration_count,
            action_steps,
            total_steps,
            failed_count,
            denied_count,
            unrecovered_failure_count,
            additions,
            deletions,
        },
        label,
        live_label,
        live_detail,
        label_includes_failure,
    }
}

#[derive(Debug, Clone)]
pub struct SlicedStageTurns {
    pub turns: Vec<StageTurnView>,
    pub turn_item_starts: Vec<usize>,
}

/// Builds semantic turns starting at `start_item_idx` (and `start_turn_index`),
/// allowing `OrbitAppCore` to recompute only the active tail when streaming deltas arrive.
pub fn process_timeline_turns_slice(
    items: &[TimelineItemView],
    start_item_idx: usize,
    start_turn_index: u32,
    cwd: Option<&str>,
) -> SlicedStageTurns {
    let mut turns: Vec<StageTurnView> = Vec::new();
    let mut turn_item_starts: Vec<usize> = Vec::new();

    let mut current_prompt: Option<TimelineItemView> = None;
    let mut current_body: Vec<TimelineItemView> = Vec::new();
    let mut current_turn_start: Option<usize> = None;
    let mut has_open_turn = false;

    let flush_turn = |turns: &mut Vec<StageTurnView>,
                      turn_item_starts: &mut Vec<usize>,
                      current_prompt: &mut Option<TimelineItemView>,
                      current_body: &mut Vec<TimelineItemView>,
                      current_turn_start: &mut Option<usize>,
                      has_open_turn: &mut bool| {
        if !*has_open_turn {
            return;
        }
        let idx = start_turn_index + turns.len() as u32;
        let prompt = current_prompt.take();
        let body = std::mem::take(current_body);
        let item_start = current_turn_start.take().unwrap_or(start_item_idx);
        *has_open_turn = false;
        turns.push(finalize_turn(idx, prompt, body, cwd));
        turn_item_starts.push(item_start);
    };

    for (abs_idx, raw_item) in items.iter().enumerate().skip(start_item_idx) {
        let item = enrich_item_descriptor(raw_item, cwd);
        if let TimelineItemView::PermissionRequest {
            ref resolved_decision,
            ..
        } = item
            && resolved_decision.is_none()
        {
            continue;
        }
        if let TimelineItemView::QuestionRequest {
            ref resolved_answers,
            ..
        } = item
            && resolved_answers.is_none()
        {
            continue;
        }

        if matches!(
            item,
            TimelineItemView::UserPrompt { .. } | TimelineItemView::AgentMessage { .. }
        ) {
            flush_turn(
                &mut turns,
                &mut turn_item_starts,
                &mut current_prompt,
                &mut current_body,
                &mut current_turn_start,
                &mut has_open_turn,
            );
            current_prompt = Some(item);
            current_body.clear();
            current_turn_start = Some(abs_idx);
            has_open_turn = true;
            continue;
        }

        if !has_open_turn {
            has_open_turn = true;
            current_prompt = None;
            current_turn_start = Some(abs_idx);
        }
        current_body.push(item);
    }

    flush_turn(
        &mut turns,
        &mut turn_item_starts,
        &mut current_prompt,
        &mut current_body,
        &mut current_turn_start,
        &mut has_open_turn,
    );

    SlicedStageTurns {
        turns,
        turn_item_starts,
    }
}

/// Collects unresolved HITL gates, session-wide tool diffs, and derived session insights in a single pass.
pub fn collect_stage_auxiliary_state(
    items: &[TimelineItemView],
    cwd: Option<&str>,
    current_agent: Option<&AgentCardView>,
    host_agents: &[AgentCardView],
) -> (
    Vec<TimelineItemView>,
    Vec<TimelineItemView>,
    Vec<DiffFileView>,
    SessionInsightsView,
) {
    let mut pending_permissions: Vec<TimelineItemView> = Vec::new();
    let mut pending_questions: Vec<TimelineItemView> = Vec::new();
    let mut session_turn_diffs: Vec<DiffFileView> = Vec::new();

    for raw_item in items {
        let item = enrich_item_descriptor(raw_item, cwd);
        match item {
            TimelineItemView::ToolCall { ref diff_files, .. } => {
                for df in diff_files {
                    session_turn_diffs.push(df.clone());
                }
            }
            TimelineItemView::PermissionRequest {
                ref resolved_decision,
                ..
            } if resolved_decision.is_none() => {
                pending_permissions.push(item);
            }
            TimelineItemView::QuestionRequest {
                ref resolved_answers,
                ..
            } if resolved_answers.is_none() => {
                pending_questions.push(item);
            }
            _ => {}
        }
    }

    let insights = derive_session_insights(items, cwd, current_agent, host_agents);
    (
        pending_permissions,
        pending_questions,
        session_turn_diffs,
        insights,
    )
}

pub fn process_timeline_for_stage(
    items: &[TimelineItemView],
    cwd: Option<&str>,
) -> (
    Vec<StageTurnView>,
    Vec<TimelineItemView>,
    Vec<TimelineItemView>,
    Vec<DiffFileView>,
) {
    let sliced = process_timeline_turns_slice(items, 0, 0, cwd);
    let (pending_permissions, pending_questions, session_turn_diffs, _) =
        collect_stage_auxiliary_state(items, cwd, None, &[]);
    (
        sliced.turns,
        pending_permissions,
        pending_questions,
        session_turn_diffs,
    )
}

fn enrich_item_descriptor(item: &TimelineItemView, cwd: Option<&str>) -> TimelineItemView {
    let mut cloned = item.clone();
    if let TimelineItemView::ToolCall {
        status,
        ref summary_title,
        ref detail,
        ref diff_files,
        ref mut step_descriptor,
        ..
    } = cloned
        && step_descriptor.is_none()
    {
        *step_descriptor = Some(describe_tool_detail(
            status,
            summary_title,
            detail,
            diff_files,
            cwd,
        ));
    }
    cloned
}

fn finalize_turn(
    turn_index: u32,
    opener: Option<TimelineItemView>,
    body_items: Vec<TimelineItemView>,
    cwd: Option<&str>,
) -> StageTurnView {
    let (user_prompt, agent_message) = match opener {
        Some(item @ TimelineItemView::AgentMessage { .. }) => (None, Some(item)),
        other => (other, None),
    };
    let anchor_opener = user_prompt.as_ref().or(agent_message.as_ref());

    let anchor_seq = anchor_opener
        .map(timeline_item_seq)
        .or_else(|| body_items.first().map(timeline_item_seq))
        .unwrap_or(u64::from(turn_index));

    let key = match (&user_prompt, &agent_message) {
        (Some(TimelineItemView::UserPrompt { message_id, .. }), _) => format!("turn-{message_id}"),
        (_, Some(TimelineItemView::AgentMessage { message_id, .. })) => {
            format!("turn-am-{message_id}")
        }
        _ => body_items
            .first()
            .map(|it| format!("turn-init-{}", stable_timeline_item_key(it)))
            .unwrap_or_else(|| format!("turn-init-idx-{turn_index}")),
    };

    let is_compaction_command = match &user_prompt {
        Some(TimelineItemView::UserPrompt { text, badges, .. }) => {
            let trimmed = text.trim();
            is_compact_slash_text(trimmed)
                || (trimmed.is_empty()
                    && badges
                        .iter()
                        .any(|b| b.kind == "slash" && b.label == "/compact"))
        }
        _ => false,
    };

    let mut notices: Vec<TimelineItemView> = Vec::new();
    let mut compactions: Vec<TurnCompactionItemView> = Vec::new();
    let mut non_notice_body: Vec<TimelineItemView> = Vec::new();
    let mut has_error = false;
    let mut has_active_item = false;

    let mut diff_paths_order: Vec<String> = Vec::new();
    let mut diff_by_path: HashMap<String, TurnFileDiffSummaryView> = HashMap::new();

    for it in &body_items {
        match it {
            TimelineItemView::ToolCall {
                status,
                detail,
                diff_files,
                ..
            } => {
                if *status == ToolCallStatus::Running {
                    has_active_item = true;
                }
                if matches!(status, ToolCallStatus::Failed | ToolCallStatus::Denied) {
                    has_error = true;
                }
                for df in diff_files {
                    let norm_path = strip_workspace_prefix(&df.path, cwd);
                    let key_path = if norm_path.is_empty() {
                        df.path.clone()
                    } else {
                        norm_path
                    };
                    if let Some(existing) = diff_by_path.get_mut(&key_path) {
                        existing.additions += df.additions;
                        existing.deletions += df.deletions;
                    } else {
                        diff_paths_order.push(key_path.clone());
                        diff_by_path.insert(
                            key_path.clone(),
                            TurnFileDiffSummaryView {
                                path: key_path,
                                additions: df.additions,
                                deletions: df.deletions,
                            },
                        );
                    }
                }
                if let ToolCallDetail::FileEdit { path, .. } = detail
                    && !path.is_empty()
                {
                    let norm_path = strip_workspace_prefix(path, cwd);
                    let key_path = if norm_path.is_empty() {
                        path.clone()
                    } else {
                        norm_path
                    };
                    if !diff_by_path.contains_key(&key_path) {
                        diff_paths_order.push(key_path.clone());
                        diff_by_path.insert(
                            key_path.clone(),
                            TurnFileDiffSummaryView {
                                path: key_path,
                                additions: 0,
                                deletions: 0,
                            },
                        );
                    }
                }
            }
            TimelineItemView::AssistantMessage {
                streaming: true, ..
            } => {
                has_active_item = true;
            }
            _ => {}
        }

        if let TimelineItemView::Notice {
            seq,
            ref level,
            ref message,
            ..
        } = *it
        {
            if level != "error" && contains_ascii_case_insensitive(message, "compact") {
                compactions.push(TurnCompactionItemView {
                    seq,
                    label: message.clone(),
                });
            } else {
                notices.push(it.clone());
            }
            continue;
        }

        non_notice_body.push(it.clone());
    }

    let mut terminal_reply_index: Option<usize> = None;
    for (idx, candidate) in non_notice_body.iter().enumerate().rev() {
        if matches!(candidate, TimelineItemView::AssistantMessage { .. }) {
            terminal_reply_index = Some(idx);
            break;
        }
        if is_step_row_item(candidate) {
            break;
        }
    }

    let candidate_folded: &[TimelineItemView] = match terminal_reply_index {
        Some(idx) if idx > 0 => &non_notice_body[..idx],
        None => &non_notice_body,
        _ => &[],
    };

    let folded_has_running_tool = candidate_folded.iter().any(|it| {
        matches!(
            it,
            TimelineItemView::ToolCall {
                status: ToolCallStatus::Running,
                ..
            }
        )
    });

    let terminal_reply = match terminal_reply_index {
        Some(idx) if !folded_has_running_tool => Some(non_notice_body[idx].clone()),
        _ => None,
    };

    let folded_steps: Vec<TimelineItemView> =
        if let (Some(_), Some(idx)) = (&terminal_reply, terminal_reply_index) {
            non_notice_body[..idx].to_vec()
        } else if terminal_reply_index.is_none() {
            non_notice_body.clone()
        } else {
            Vec::new()
        };

    let thought_duration_ms = compute_thought_duration_ms(&folded_steps, terminal_reply.as_ref());
    let summary = summarize_work_items(&folded_steps, thought_duration_ms);
    let full_turn_tool_summary = summarize_work_items(&non_notice_body, thought_duration_ms);

    let started_at_ms = anchor_opener
        .and_then(timeline_item_timestamp_ms)
        .or_else(|| body_items.iter().find_map(timeline_item_timestamp_ms));

    let work_start_ms = folded_steps
        .iter()
        .find_map(timeline_item_timestamp_ms)
        .or_else(|| anchor_opener.and_then(timeline_item_timestamp_ms));
    let work_end_ms = terminal_reply
        .as_ref()
        .and_then(timeline_item_timestamp_ms)
        .or_else(|| {
            folded_steps
                .iter()
                .rev()
                .find_map(timeline_item_timestamp_ms)
        });
    let work_duration_ms = match (work_start_ms, work_end_ms) {
        (Some(start), Some(end)) if end - start >= 1000 => Some(end - start),
        _ => None,
    };

    let completed_at_ms = terminal_reply
        .as_ref()
        .and_then(timeline_item_timestamp_ms)
        .or_else(|| body_items.iter().rev().find_map(timeline_item_timestamp_ms))
        .or(started_at_ms);

    let turn_duration_ms = match (started_at_ms, completed_at_ms) {
        (Some(start), Some(end)) if end > start => Some(end - start),
        _ => None,
    };

    let last_assistant_reply = non_notice_body
        .iter()
        .rev()
        .find(|it| matches!(it, TimelineItemView::AssistantMessage { text, .. } if !text.trim().is_empty()))
        .cloned();

    let turn_diffs: Vec<TurnFileDiffSummaryView> = diff_paths_order
        .into_iter()
        .filter_map(|p| diff_by_path.remove(&p))
        .collect();

    StageTurnView {
        key,
        turn_index,
        anchor_seq,
        user_prompt,
        agent_message,
        is_compaction_command,
        folded_steps,
        work_counts: summary.counts,
        work_summary_label: summary.label,
        work_summary_live_label: summary.live_label,
        work_summary_live_detail: summary.live_detail,
        work_summary_includes_failure: summary.label_includes_failure,
        thought_duration_ms,
        work_duration_ms,
        turn_duration_ms,
        terminal_reply,
        last_assistant_reply,
        notices,
        compactions,
        live_segments: build_live_segments(&body_items, started_at_ms),
        turn_diffs,
        has_error,
        has_unrecovered_error: full_turn_tool_summary.counts.unrecovered_failure_count > 0,
        has_active_item,
        started_at_ms,
        completed_at_ms,
    }
}

fn build_live_segments(
    body_items: &[TimelineItemView],
    fallback_start_ms: Option<i64>,
) -> Vec<TurnLiveSegmentView> {
    let mut raw_segments: Vec<TurnLiveSegmentView> = Vec::new();
    let mut step_buf: Vec<TimelineItemView> = Vec::new();

    let flush_steps = |raw_segments: &mut Vec<TurnLiveSegmentView>,
                       step_buf: &mut Vec<TimelineItemView>,
                       has_following_content: bool,
                       following_item: Option<&TimelineItemView>| {
        if step_buf.is_empty() {
            return;
        }
        let first_seq = timeline_item_seq(&step_buf[0]);
        let has_running_step = step_buf.iter().any(|it| {
            matches!(
                it,
                TimelineItemView::ToolCall {
                    status: ToolCallStatus::Running,
                    ..
                }
            )
        });
        let thought_ms = compute_thought_duration_ms(step_buf, following_item);
        let started_at_ms = step_buf
            .iter()
            .find_map(timeline_item_timestamp_ms)
            .or_else(|| {
                raw_segments
                    .is_empty()
                    .then_some(fallback_start_ms)
                    .flatten()
            });
        let latest_activity_ms = step_buf.iter().rev().find_map(timeline_item_timestamp_ms);
        let end_ms = following_item
            .and_then(timeline_item_timestamp_ms)
            .or(latest_activity_ms);
        let work_duration_ms = match (started_at_ms, end_ms) {
            (Some(s), Some(e)) if e - s >= 1000 => Some(e - s),
            _ => None,
        };
        let items = std::mem::take(step_buf);
        let group_key = format!("steps-{}", stable_timeline_item_key(&items[0]));
        let summary = summarize_work_items(&items, thought_ms);
        raw_segments.push(TurnLiveSegmentView::StepGroup {
            key: group_key,
            first_seq,
            items,
            summary,
            thought_duration_ms: thought_ms,
            work_duration_ms,
            started_at_ms,
            latest_activity_ms,
            has_following_content,
            has_running_step,
        });
    };

    for it in body_items {
        if is_step_row_item(it) {
            step_buf.push(it.clone());
            continue;
        }

        match it {
            TimelineItemView::AssistantMessage {
                seq,
                item_id,
                text,
                streaming,
                ..
            } => {
                if text.trim().is_empty() && !*streaming {
                    continue;
                }
                flush_steps(&mut raw_segments, &mut step_buf, true, Some(it));
                let msg_key = if !item_id.is_empty() {
                    format!("msg-{item_id}")
                } else {
                    format!("msg-seq-{seq}")
                };
                raw_segments.push(TurnLiveSegmentView::AssistantMessage {
                    key: msg_key,
                    item: it.clone(),
                });
            }
            TimelineItemView::Notice {
                seq,
                level,
                message,
                ..
            } => {
                if level == "wakeup" {
                    flush_steps(&mut raw_segments, &mut step_buf, true, Some(it));
                    raw_segments.push(TurnLiveSegmentView::Wakeup {
                        key: format!("wakeup-{seq}"),
                        label: if message.is_empty() {
                            "Resumed from background task".to_string()
                        } else {
                            message.clone()
                        },
                    });
                } else if level != "error" && contains_ascii_case_insensitive(message, "compact") {
                    flush_steps(&mut raw_segments, &mut step_buf, true, Some(it));
                    raw_segments.push(TurnLiveSegmentView::Compaction {
                        key: format!("compact-{seq}"),
                        label: message.clone(),
                    });
                } else {
                    flush_steps(&mut raw_segments, &mut step_buf, false, Some(it));
                    raw_segments.push(TurnLiveSegmentView::Notice {
                        key: format!("notice-{seq}"),
                        item: it.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    flush_steps(&mut raw_segments, &mut step_buf, false, None);
    raw_segments
}

fn compute_thought_duration_ms(
    items: &[TimelineItemView],
    following_item: Option<&TimelineItemView>,
) -> Option<i64> {
    let (first_idx, start_ts) = items.iter().enumerate().find_map(|(idx, it)| {
        if matches!(it, TimelineItemView::AssistantThinking { .. }) {
            timeline_item_timestamp_ms(it).map(|ts| (idx, ts))
        } else {
            None
        }
    })?;

    let end_ts = items[first_idx + 1..]
        .iter()
        .find(|it| !matches!(it, TimelineItemView::AssistantThinking { .. }))
        .and_then(timeline_item_timestamp_ms)
        .or_else(|| {
            following_item
                .filter(|it| !matches!(it, TimelineItemView::AssistantThinking { .. }))
                .and_then(timeline_item_timestamp_ms)
        })?;

    let delta = end_ts - start_ts;
    if delta >= 1000 { Some(delta) } else { None }
}

fn is_step_row_item(item: &TimelineItemView) -> bool {
    matches!(
        item,
        TimelineItemView::AssistantThinking { .. }
            | TimelineItemView::ToolCall { .. }
            | TimelineItemView::PermissionRequest { .. }
            | TimelineItemView::QuestionRequest { .. }
    )
}

fn summarize_single_tool_call(
    status: ToolCallStatus,
    summary_title: &str,
    detail: &ToolCallDetail,
) -> (String, bool) {
    if status == ToolCallStatus::Denied {
        let lbl = if matches!(detail, ToolCallDetail::Shell { .. }) {
            "Stopped running the command"
        } else {
            "You declined"
        };
        return (lbl.to_string(), true);
    }

    let failed = status == ToolCallStatus::Failed;
    let with_failed = |base: String| -> (String, bool) {
        if failed {
            (format!("{base} (failed)"), true)
        } else {
            (base, false)
        }
    };

    match detail {
        ToolCallDetail::FileRead { path, .. } => {
            let name = basename_of_path(path);
            with_failed(if !name.is_empty() {
                format!("Read {name}")
            } else {
                "Read a file".to_string()
            })
        }
        ToolCallDetail::FileEdit { path, .. } => {
            let is_create = is_file_create_detail(summary_title, detail);
            let name = basename_of_path(path);
            let prefix = if is_create { "Created" } else { "Edited" };
            with_failed(if !name.is_empty() {
                format!("{prefix} {name}")
            } else {
                format!("{prefix} a file")
            })
        }
        ToolCallDetail::Search { query, .. } => match classify_search_tool(query, summary_title) {
            SearchSubkind::Web => {
                if failed {
                    ("Failed to search the web".to_string(), true)
                } else {
                    ("Searched the web".to_string(), false)
                }
            }
            SearchSubkind::Glob => with_failed("Looked for files".to_string()),
            SearchSubkind::Grep => with_failed("Searched files".to_string()),
        },
        ToolCallDetail::Shell { .. } => {
            if failed {
                ("Command failed".to_string(), true)
            } else {
                ("Ran a command".to_string(), false)
            }
        }
        ToolCallDetail::Subagent { .. } => with_failed("Ran an agent".to_string()),
        ToolCallDetail::Generic { tool_name, .. } => {
            let parsed = parse_generic_tool_name(tool_name);
            if let Some(server) = parsed.server_name {
                with_failed(format!("Used {server} integration"))
            } else if !parsed.display_label.is_empty() && parsed.display_label != "Tool" {
                with_failed(format!("Used {}", parsed.display_label))
            } else if failed {
                ("Failed".to_string(), true)
            } else {
                ("Used a tool".to_string(), false)
            }
        }
    }
}

fn build_live_step_status(item: Option<&TimelineItemView>) -> (String, Option<String>) {
    let Some(item) = item else {
        return ("Working…".to_string(), None);
    };
    match item {
        TimelineItemView::AssistantThinking { text, .. } => {
            let headline = extract_thinking_headline(text, true);
            if headline.is_empty() {
                ("Thinking…".to_string(), None)
            } else {
                ("Thinking".to_string(), Some(headline))
            }
        }
        TimelineItemView::ToolCall {
            summary_title,
            detail,
            ..
        } => match detail {
            ToolCallDetail::FileRead { path, .. } => {
                let name = basename_of_path(path);
                let label = if !name.is_empty() {
                    format!("Reading {name}…")
                } else {
                    "Reading a file…".to_string()
                };
                (label, None)
            }
            ToolCallDetail::FileEdit { path, .. } => {
                let is_create = is_file_create_detail(summary_title, detail);
                let name = basename_of_path(path);
                let verb = if is_create { "Creating" } else { "Editing" };
                let label = if !name.is_empty() {
                    format!("{verb} {name}…")
                } else {
                    format!("{verb} a file…")
                };
                (label, None)
            }
            ToolCallDetail::Search { query, .. } => {
                match classify_search_tool(query, summary_title) {
                    SearchSubkind::Web => ("Searching the web…".to_string(), None),
                    SearchSubkind::Glob => ("Looking for files…".to_string(), None),
                    SearchSubkind::Grep => {
                        let q = query.split_whitespace().collect::<Vec<_>>().join(" ");
                        let char_len = q.chars().count();
                        let label = if (1..=28).contains(&char_len) {
                            format!("Searching \"{q}\"…")
                        } else {
                            "Searching files…".to_string()
                        };
                        (label, None)
                    }
                }
            }
            ToolCallDetail::Shell { command, .. } => {
                let first_line = command
                    .lines()
                    .map(|ln| ln.split_whitespace().collect::<Vec<_>>().join(" "))
                    .find(|ln| !ln.is_empty())
                    .unwrap_or_default();
                let label = if first_line.is_empty() {
                    "Running a command…".to_string()
                } else {
                    let short_cmd = truncate_chars_head(&first_line, 36).trim_end().to_string();
                    if short_cmd.ends_with('…') {
                        format!("Running {short_cmd}")
                    } else {
                        format!("Running {short_cmd}…")
                    }
                };
                (label, None)
            }
            ToolCallDetail::Subagent { role, .. } => {
                let clean_role = role.split_whitespace().collect::<Vec<_>>().join(" ");
                let char_len = clean_role.chars().count();
                let label = if (1..=28).contains(&char_len) {
                    format!("Running {clean_role}…")
                } else {
                    "Running subagent…".to_string()
                };
                (label, None)
            }
            ToolCallDetail::Generic { tool_name, .. } => {
                let parsed = parse_generic_tool_name(tool_name);
                let label = if let Some(server) = parsed.server_name {
                    format!("Using {server}…")
                } else if !parsed.display_label.is_empty() {
                    format!("Using {}…", parsed.display_label)
                } else {
                    "Using a tool…".to_string()
                };
                (label, None)
            }
        },
        _ => ("Working…".to_string(), None),
    }
}

fn truncate_chars_head(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    collected
}

fn truncate_chars_tail(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_string();
    }
    s.chars().skip(total - max_chars).collect()
}

fn normalize_detected_url(raw: &str) -> String {
    let replaced = raw.replace("0.0.0.0", "localhost");
    replaced
        .trim_end_matches(&['.', ',', ';', ':', '!', '?'][..])
        .to_string()
}

fn collect_local_urls(text: &str, out: &mut Vec<String>) {
    if text.is_empty() || out.len() >= 8 {
        return;
    }
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut offset = 0;

    while offset < lower.len() && out.len() < 8 {
        let rel_http = lower[offset..].find("http://");
        let rel_https = lower[offset..].find("https://");
        let (rel, scheme_len) = match (rel_http, rel_https) {
            (Some(a), Some(b)) => {
                if a <= b {
                    (a, 7)
                } else {
                    (b, 8)
                }
            }
            (Some(a), None) => (a, 7),
            (None, Some(b)) => (b, 8),
            (None, None) => break,
        };
        let start = offset + rel;
        let word_boundary = if start == 0 {
            true
        } else {
            text[..start]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        };
        let after_scheme = start + scheme_len;
        if word_boundary && lower.is_char_boundary(after_scheme) {
            let rest_lower = &lower[after_scheme..];
            let host_len = if rest_lower.starts_with("localhost:") {
                Some("localhost".len())
            } else if rest_lower.starts_with("127.0.0.1:") {
                Some("127.0.0.1".len())
            } else if rest_lower.starts_with("0.0.0.0:") {
                Some("0.0.0.0".len())
            } else if rest_lower.starts_with("[::1]:") {
                Some("[::1]".len())
            } else {
                None
            };

            if let Some(hl) = host_len {
                let port_start = after_scheme + hl + 1;
                let mut port_end = port_start;
                while port_end < bytes.len()
                    && bytes[port_end].is_ascii_digit()
                    && (port_end - port_start) < 5
                {
                    port_end += 1;
                }
                let port_digits = port_end - port_start;
                let port_terminated_by_digit =
                    port_end < bytes.len() && bytes[port_end].is_ascii_digit();
                if (2..=5).contains(&port_digits) && !port_terminated_by_digit {
                    let mut url_end = port_end;
                    if url_end < bytes.len() && bytes[url_end] == b'/' {
                        url_end += 1;
                        while url_end < text.len() {
                            let Some(ch) = text[url_end..].chars().next() else {
                                break;
                            };
                            if ch.is_whitespace()
                                || matches!(ch, '"' | '\'' | '`' | '<' | '>' | ')' | ']')
                            {
                                break;
                            }
                            url_end += ch.len_utf8();
                        }
                    }
                    let cleaned = normalize_detected_url(&text[start..url_end]);
                    if !cleaned.is_empty() {
                        push_unique(out, &cleaned);
                    }
                    offset = url_end;
                    continue;
                }
            }
        }
        offset = after_scheme;
    }
}

/// Derive compact, deduplicated session inventory (touched files, active background tasks,
/// subagents, and detected localhost preview URLs) from the active timeline & host state.
pub fn derive_session_insights(
    items: &[TimelineItemView],
    cwd: Option<&str>,
    current_agent: Option<&AgentCardView>,
    host_agents: &[AgentCardView],
) -> SessionInsightsView {
    let mut file_map: HashMap<String, SessionTouchedFileView> = HashMap::new();
    let mut running_shells: Vec<SessionBackgroundTaskView> = Vec::new();
    let mut subagent_map: HashMap<String, SessionSubagentEntryView> = HashMap::new();
    let mut detected_local_urls: Vec<String> = Vec::new();

    let last_user_prompt_index = items
        .iter()
        .rposition(|it| matches!(it, TimelineItemView::UserPrompt { .. }));
    let agent_running = current_agent.is_some_and(|a| a.is_running);

    for (idx, item) in items.iter().enumerate() {
        match item {
            TimelineItemView::AssistantMessage { text, .. } => {
                collect_local_urls(text, &mut detected_local_urls);
            }
            TimelineItemView::ToolCall {
                seq,
                call_id,
                status,
                summary_title,
                detail,
                diff_files,
                timestamp_ms,
                ..
            } => match detail {
                ToolCallDetail::FileRead { path, .. } => {
                    let raw_path = path.trim();
                    if raw_path.is_empty() {
                        continue;
                    }
                    let display_path = strip_workspace_prefix(raw_path, cwd);
                    file_map
                        .entry(display_path.clone())
                        .and_modify(|prev| prev.last_seq = prev.last_seq.max(*seq))
                        .or_insert_with(|| SessionTouchedFileView {
                            path: raw_path.to_string(),
                            display_path,
                            action: SessionTouchedFileAction::Read,
                            additions: 0,
                            deletions: 0,
                            last_seq: *seq,
                        });
                }
                ToolCallDetail::FileEdit { path, .. } => {
                    let raw_path = path.trim();
                    if raw_path.is_empty() {
                        continue;
                    }
                    let display_path = strip_workspace_prefix(raw_path, cwd);
                    let is_create = is_file_create_detail(summary_title, detail);
                    let add: u32 = diff_files.iter().map(|f| f.additions).sum();
                    let del: u32 = diff_files.iter().map(|f| f.deletions).sum();
                    file_map
                        .entry(display_path.clone())
                        .and_modify(|prev| {
                            prev.action =
                                if prev.action == SessionTouchedFileAction::Created || is_create {
                                    SessionTouchedFileAction::Created
                                } else {
                                    SessionTouchedFileAction::Edited
                                };
                            prev.additions += add;
                            prev.deletions += del;
                            prev.last_seq = prev.last_seq.max(*seq);
                        })
                        .or_insert_with(|| SessionTouchedFileView {
                            path: raw_path.to_string(),
                            display_path,
                            action: if is_create {
                                SessionTouchedFileAction::Created
                            } else {
                                SessionTouchedFileAction::Edited
                            },
                            additions: add,
                            deletions: del,
                            last_seq: *seq,
                        });
                }
                ToolCallDetail::Shell {
                    command,
                    cwd: shell_cwd,
                    output_preview,
                    ..
                } => {
                    if let Some(out) = output_preview {
                        collect_local_urls(out, &mut detected_local_urls);
                    }
                    let after_last_prompt =
                        last_user_prompt_index.is_none_or(|prompt_idx| idx > prompt_idx);
                    if agent_running && after_last_prompt && *status == ToolCallStatus::Running {
                        let first_line = command
                            .lines()
                            .map(str::trim)
                            .find(|l| !l.is_empty())
                            .unwrap_or("Running command")
                            .to_string();
                        let subtitle = shell_cwd
                            .as_deref()
                            .map(|c| strip_workspace_prefix(c, cwd))
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "Shell command".to_string());
                        running_shells.push(SessionBackgroundTaskView {
                            id: format!("shell:{call_id}"),
                            kind: SessionBackgroundTaskKind::Shell,
                            title: first_line,
                            subtitle,
                            status: SessionBackgroundTaskStatus::Running,
                            started_at_ms: *timestamp_ms,
                            command: Some(command.clone()),
                            output_preview: output_preview.clone(),
                        });
                    }
                }
                ToolCallDetail::Subagent {
                    child_agent_id,
                    role,
                    prompt_preview,
                } => {
                    let key = child_agent_id
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .unwrap_or(call_id.as_str())
                        .to_string();
                    subagent_map.insert(
                        key,
                        SessionSubagentEntryView {
                            id: call_id.clone(),
                            role: if role.is_empty() {
                                "Subagent".to_string()
                            } else {
                                role.clone()
                            },
                            prompt_preview: prompt_preview.clone(),
                            status: *status,
                            child_agent_id: child_agent_id.clone(),
                            last_seq: *seq,
                        },
                    );
                }
                ToolCallDetail::Search { .. } | ToolCallDetail::Generic { .. } => {}
            },
            _ => {}
        }
    }

    if let Some(cur) = current_agent {
        for ag in host_agents {
            if ag.parent_agent_id.as_deref() == Some(cur.agent_id.as_str()) {
                let status = if ag.is_running {
                    ToolCallStatus::Running
                } else if contains_ascii_case_insensitive(&ag.status_label, "error") {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                };
                if let Some(existing) = subagent_map.get_mut(&ag.agent_id) {
                    existing.status = status;
                    existing.child_agent_id = Some(ag.agent_id.clone());
                } else {
                    subagent_map.insert(
                        ag.agent_id.clone(),
                        SessionSubagentEntryView {
                            id: ag.agent_id.clone(),
                            role: if ag.title.is_empty() {
                                "Subagent".to_string()
                            } else {
                                ag.title.clone()
                            },
                            prompt_preview: ag.status_label.clone(),
                            status,
                            child_agent_id: Some(ag.agent_id.clone()),
                            last_seq: ag.latest_seq,
                        },
                    );
                }
            }
        }
    }

    let mut background_tasks = running_shells;
    if let Some(cur) = current_agent
        && cur.is_running
    {
        let trimmed = cur.status_label.trim_start();
        if starts_with_ignore_ascii_case(trimmed, "waking")
            && trimmed["waking".len()..].trim_start().starts_with('(')
        {
            background_tasks.insert(
                0,
                SessionBackgroundTaskView {
                    id: format!("wakeup:{}", cur.agent_id),
                    kind: SessionBackgroundTaskKind::Wakeup,
                    title: cur.status_label.clone(),
                    subtitle: "Autonomous background wakeup".to_string(),
                    status: SessionBackgroundTaskStatus::Running,
                    started_at_ms: None,
                    command: None,
                    output_preview: None,
                },
            );
        }
    }

    let mut touched_files: Vec<SessionTouchedFileView> = file_map.into_values().collect();
    touched_files.sort_by(|a, b| {
        let a_edit = u8::from(a.action != SessionTouchedFileAction::Read);
        let b_edit = u8::from(b.action != SessionTouchedFileAction::Read);
        b_edit
            .cmp(&a_edit)
            .then_with(|| b.last_seq.cmp(&a.last_seq))
    });

    let mut subagents: Vec<SessionSubagentEntryView> = subagent_map.into_values().collect();
    subagents.sort_by(|a, b| {
        let a_run = u8::from(a.status == ToolCallStatus::Running);
        let b_run = u8::from(b.status == ToolCallStatus::Running);
        b_run.cmp(&a_run).then_with(|| b.last_seq.cmp(&a.last_seq))
    });

    SessionInsightsView {
        touched_files,
        background_tasks,
        subagents,
        detected_local_urls,
    }
}

/// Build a concise, bounded read-only context summary of the active session for `/btw` side questions.
pub fn build_side_question_context_summary(
    cwd: &str,
    branch_name: Option<&str>,
    items: &[TimelineItemView],
    git_status: Option<&GitStatusSnapshot>,
) -> String {
    let mut sections: Vec<String> = Vec::new();
    let display_cwd = if cwd.trim().is_empty() { "~" } else { cwd };
    let branch_suffix = branch_name
        .filter(|b| !b.trim().is_empty())
        .map(|b| format!(" (branch: {b})"))
        .unwrap_or_default();
    sections.push(format!("Workspace: {display_cwd}{branch_suffix}"));

    if let Some(status) = git_status
        && !status.files.is_empty()
    {
        let total = status.files.len();
        let list: String = status
            .files
            .iter()
            .take(20)
            .map(|f| format!("{} {}", f.status, f.path))
            .collect::<Vec<_>>()
            .join(", ");
        sections.push(format!("Modified files ({total}): {list}"));
    }

    let relevant: Vec<&TimelineItemView> = items
        .iter()
        .filter(|it| match it {
            TimelineItemView::UserPrompt { .. } | TimelineItemView::AssistantMessage { .. } => true,
            TimelineItemView::ToolCall { detail, .. } => matches!(
                detail,
                ToolCallDetail::FileEdit { .. } | ToolCallDetail::Shell { .. }
            ),
            _ => false,
        })
        .collect();

    let start_idx = relevant.len().saturating_sub(18);
    let mut recent_exchanges: Vec<String> = Vec::new();
    for it in &relevant[start_idx..] {
        match it {
            TimelineItemView::UserPrompt { text, .. } => {
                let snippet = truncate_chars_head(text.trim(), 700);
                recent_exchanges.push(format!("User: {snippet}"));
            }
            TimelineItemView::AssistantMessage { text, .. } => {
                let snippet = truncate_chars_head(text.trim(), 1200);
                recent_exchanges.push(format!("Assistant: {snippet}"));
            }
            TimelineItemView::ToolCall { status, detail, .. } => {
                let status_str = match status {
                    ToolCallStatus::Running => "running",
                    ToolCallStatus::Completed => "completed",
                    ToolCallStatus::Failed => "failed",
                    ToolCallStatus::Denied => "denied",
                };
                match detail {
                    ToolCallDetail::FileEdit { path, .. } => {
                        let rel = strip_workspace_prefix(path, Some(cwd));
                        recent_exchanges.push(format!("Tool [edit {rel}] ({status_str})"));
                    }
                    ToolCallDetail::Shell {
                        command, exit_code, ..
                    } => {
                        let cmd = truncate_chars_head(command, 140);
                        let exit_suffix =
                            exit_code.map(|c| format!(" exit={c}")).unwrap_or_default();
                        recent_exchanges
                            .push(format!("Tool [shell: {cmd}] ({status_str}{exit_suffix})"));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if !recent_exchanges.is_empty() {
        recent_exchanges.shrink_to_fit();
        sections.push(format!(
            "Recent conversation trace:\n{}",
            recent_exchanges.join("\n\n")
        ));
    }

    truncate_chars_tail(&sections.join("\n\n"), 10_500)
}

fn lowercase_clause_lead(clause: &str) -> String {
    let mut chars = clause.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = first.to_lowercase().collect::<String>();
    out.push_str(chars.as_str());
    out
}

fn push_unique(vec: &mut Vec<String>, val: &str) {
    if !vec.iter().any(|existing| existing == val) {
        vec.push(val.to_string());
    }
}

fn humanize_identifier(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return raw.to_string();
    }
    let mut spaced = String::with_capacity(trimmed.len() + 4);
    let mut prev_lower_or_digit = false;
    for ch in trimmed.chars() {
        if matches!(ch, '.' | '_' | '-') {
            spaced.push(' ');
            prev_lower_or_digit = false;
            continue;
        }
        if prev_lower_or_digit && ch.is_ascii_uppercase() {
            spaced.push(' ');
        }
        prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        spaced.push(ch);
    }
    let clean = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.is_empty() {
        return raw.to_string();
    }
    let mut chars = clean.chars();
    let Some(first) = chars.next() else {
        return raw.to_string();
    };
    let mut out = first.to_uppercase().collect::<String>();
    out.push_str(&chars.as_str().to_lowercase());
    out
}

fn format_server_title(raw_server: &str) -> String {
    let mut base = raw_server;
    if starts_with_ignore_ascii_case(base, "plugin_") {
        let after_plugin = &base[7..];
        if let Some(underscore_idx) = after_plugin.find('_')
            && underscore_idx > 0
            && underscore_idx + 1 < after_plugin.len()
        {
            base = &after_plugin[underscore_idx + 1..];
        }
    }
    let replaced: String = base
        .chars()
        .map(|c| if matches!(c, '.' | '_' | '-') { ' ' } else { c })
        .collect();
    let parts: Vec<&str> = replaced.split_whitespace().collect();
    if parts.is_empty() {
        return raw_server.to_string();
    }
    parts
        .into_iter()
        .map(|part| {
            if part.len() <= 3
                && matches!(
                    part.to_ascii_lowercase().as_str(),
                    "gh" | "pr" | "ci" | "db" | "ui" | "ai" | "os" | "ip"
                )
            {
                part.to_ascii_uppercase()
            } else {
                let mut chars = part.chars();
                match chars.next() {
                    Some(f) => {
                        let mut s = f.to_uppercase().collect::<String>();
                        s.push_str(chars.as_str());
                        s
                    }
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_markdown_line_prefix(trimmed: &str) -> String {
    let mut s = trimmed;
    // Heading `^#{1,6}\s+`
    let hash_count = s.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hash_count) {
        let after = &s[hash_count..];
        if after.chars().next().is_some_and(char::is_whitespace) {
            s = after.trim_start();
        }
    }

    // Bullet `^[-*+]\s+` or numbered `^\d{1,3}\.\s+`
    if let Some(after_bullet) = s.strip_prefix(['-', '*', '+'])
        && after_bullet.chars().next().is_some_and(char::is_whitespace)
    {
        s = after_bullet.trim_start();
    } else {
        let digits = s.bytes().take_while(u8::is_ascii_digit).count();
        if (1..=3).contains(&digits)
            && let Some(after_dot) = s[digits..].strip_prefix('.')
            && after_dot.chars().next().is_some_and(char::is_whitespace)
        {
            s = after_dot.trim_start();
        }
    }

    // Blockquote `^>\s*`
    if let Some(after_quote) = s.strip_prefix('>') {
        s = after_quote.trim_start();
    }

    s.to_string()
}

fn strip_markdown_links(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while !rest.is_empty() {
        let link_prefix = if let Some(after_bang) = rest.strip_prefix("![") {
            Some(after_bang)
        } else {
            rest.strip_prefix('[')
        };

        if let Some(after_open) = link_prefix
            && let Some(close_bracket) = after_open.find(']')
        {
            let label = &after_open[..close_bracket];
            let after_close = &after_open[close_bracket + 1..];
            if let Some(after_paren) = after_close.strip_prefix('(')
                && let Some(close_paren) = after_paren.find(')')
            {
                out.push_str(label);
                rest = &after_paren[close_paren + 1..];
                continue;
            }
        }

        if let Some(ch) = rest.chars().next() {
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        } else {
            break;
        }
    }
    out
}

fn starts_with_ignore_ascii_case(haystack: &str, prefix: &str) -> bool {
    haystack.len() >= prefix.len()
        && haystack.is_char_boundary(prefix.len())
        && haystack[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn contains_ascii_case_insensitive(haystack: &str, needle_lower: &str) -> bool {
    haystack.to_ascii_lowercase().contains(needle_lower)
}

fn is_compact_slash_text(trimmed: &str) -> bool {
    if !starts_with_ignore_ascii_case(trimmed, "/compact") {
        return false;
    }
    let rem = &trimmed["/compact".len()..];
    rem.is_empty()
        || rem
            .chars()
            .next()
            .is_some_and(|c| !c.is_alphanumeric() && c != '_')
}

fn contains_word(haystack: &str, phrase: &str) -> bool {
    let mut offset = 0;
    while let Some(pos) = haystack[offset..].find(phrase) {
        let abs = offset + pos;
        let end = abs + phrase.len();
        let before_ok = if abs == 0 {
            true
        } else {
            haystack[..abs]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        };
        let after_ok = if end >= haystack.len() {
            true
        } else {
            haystack[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        };
        if before_ok && after_ok {
            return true;
        }
        offset = abs + 1;
        while offset < haystack.len() && !haystack.is_char_boundary(offset) {
            offset += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use orbit_protocol::QuestionItem;

    use super::*;
    use crate::render::{DiffLineKind, DiffLineView};

    fn make_tool(
        seq: u64,
        status: ToolCallStatus,
        summary_title: &str,
        detail: ToolCallDetail,
        diff_files: Vec<DiffFileView>,
        ts: i64,
    ) -> TimelineItemView {
        TimelineItemView::ToolCall {
            seq,
            call_id: format!("tc_{seq}"),
            status,
            summary_title: summary_title.to_string(),
            detail,
            step_descriptor: None,
            diff_files,
            timestamp_ms: Some(ts),
        }
    }

    #[test]
    fn single_tool_calls_and_step_descriptors_match_desktop() {
        let read_item = make_tool(
            1,
            ToolCallStatus::Completed,
            "Read file",
            ToolCallDetail::FileRead {
                path: "/workspace/orbit/src/omni-composer.tsx".into(),
                start_line: Some(10),
                end_line: Some(45),
            },
            Vec::new(),
            1000,
        );
        let desc = describe_tool_step(&read_item, Some("/workspace/orbit")).expect("desc");
        assert_eq!(desc.kind, ToolStepKind::FileRead);
        assert_eq!(desc.verb, "Read");
        assert_eq!(desc.target, "src/omni-composer.tsx");
        assert_eq!(desc.secondary_meta.as_deref(), Some("L10–45"));

        let summary = summarize_work_items(&[read_item], None);
        assert_eq!(summary.label, "Read omni-composer.tsx");
        assert!(!summary.label_includes_failure);

        let failed_shell = make_tool(
            2,
            ToolCallStatus::Failed,
            "$ cargo test",
            ToolCallDetail::Shell {
                command: "cargo test".into(),
                cwd: None,
                exit_code: Some(101),
                output_preview: Some("test failed".into()),
            },
            Vec::new(),
            2000,
        );
        let shell_summary = summarize_work_items(&[failed_shell], None);
        assert_eq!(shell_summary.label, "Command failed");
        assert!(shell_summary.label_includes_failure);
    }

    #[test]
    fn multi_tool_category_bucketing_prioritizes_and_collapses_overflow() {
        let items = vec![
            make_tool(
                1,
                ToolCallStatus::Completed,
                "Read a",
                ToolCallDetail::FileRead {
                    path: "/repo/a.rs".into(),
                    start_line: None,
                    end_line: None,
                },
                Vec::new(),
                1000,
            ),
            make_tool(
                2,
                ToolCallStatus::Completed,
                "Edit a",
                ToolCallDetail::FileEdit {
                    path: "/repo/a.rs".into(),
                    unified_diff: None,
                },
                Vec::new(),
                1500,
            ),
            make_tool(
                3,
                ToolCallStatus::Completed,
                "Edit b",
                ToolCallDetail::FileEdit {
                    path: "/repo/b.rs".into(),
                    unified_diff: None,
                },
                Vec::new(),
                2000,
            ),
            make_tool(
                4,
                ToolCallStatus::Completed,
                "$ cargo check",
                ToolCallDetail::Shell {
                    command: "cargo check".into(),
                    cwd: None,
                    exit_code: Some(0),
                    output_preview: None,
                },
                Vec::new(),
                2500,
            ),
        ];

        // 3 categories (edit tier 1, read tier 3, shell tier 4) -> joined up to 3 clauses
        let three_cat = summarize_work_items(&items, None);
        assert_eq!(
            three_cat.label,
            "Edited 2 files, read a file, ran a command"
        );

        // 2 categories (edit + shell) -> exact `Edited 2 files, ran a command`
        let two_cat = summarize_work_items(&items[1..], None);
        assert_eq!(two_cat.label, "Edited 2 files, ran a command");

        // 4 categories -> collapses into top 2 + `and N more steps`
        let mut four_cat_items = items;
        four_cat_items.push(make_tool(
            5,
            ToolCallStatus::Completed,
            "mcp__github__list_pulls",
            ToolCallDetail::Generic {
                tool_name: "mcp__github__list_pulls".into(),
                arguments_json: "{}".into(),
            },
            Vec::new(),
            3000,
        ));
        let four_cat = summarize_work_items(&four_cat_items, None);
        assert_eq!(
            four_cat.label,
            "Edited 2 files, used Github integration, and 2 more steps"
        );
    }

    #[test]
    fn stage_turn_auto_folding_thought_duration_and_active_running_guard() {
        let diff_file = DiffFileView {
            path: "src/lib.rs".into(),
            additions: 5,
            deletions: 2,
            lines: vec![DiffLineView {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(1),
                content: "pub mod stage;".into(),
            }],
        };

        // Settled turn with 6s thinking -> tool call -> terminal reply, plus a pending permission!
        let items = vec![
            TimelineItemView::UserPrompt {
                seq: 1,
                message_id: "m1".into(),
                text: "Implement stage module".into(),
                badges: Vec::new(),
                blocks: Vec::new(),
                timestamp_ms: Some(10_000),
            },
            TimelineItemView::AssistantThinking {
                seq: 2,
                item_id: "th1".into(),
                text: "### Planning\nLet's design [stage.rs](file:///src/stage.rs) carefully."
                    .into(),
                timestamp_ms: Some(11_000),
            },
            TimelineItemView::AssistantMessage {
                seq: 3,
                item_id: "a1".into(),
                text: "Done!".into(),
                streaming: false,
                blocks: Vec::new(),
                timestamp_ms: Some(17_000),
            },
            // Turn 2: running tool call + unresolved permission & question
            TimelineItemView::UserPrompt {
                seq: 4,
                message_id: "m2".into(),
                text: "Edit lib.rs".into(),
                badges: Vec::new(),
                blocks: Vec::new(),
                timestamp_ms: Some(20_000),
            },
            make_tool(
                5,
                ToolCallStatus::Running,
                "Editing lib.rs",
                ToolCallDetail::FileEdit {
                    path: "/repo/src/lib.rs".into(),
                    unified_diff: Some("@@ -1 +1 @@\n+pub mod stage;".into()),
                },
                vec![diff_file],
                22_000,
            ),
            TimelineItemView::PermissionRequest {
                seq: 6,
                request_id: "perm_1".into(),
                title: "Allow command".into(),
                detail: ToolCallDetail::Shell {
                    command: "rm -rf target".into(),
                    cwd: None,
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                resolved_decision: None,
                timestamp_ms: Some(23_000),
            },
            TimelineItemView::QuestionRequest {
                seq: 7,
                request_id: "q_1".into(),
                questions: vec![QuestionItem {
                    id: "q1".into(),
                    header: None,
                    question: "Proceed?".into(),
                    options: vec!["Yes".into(), "No".into()],
                    is_multi_select: false,
                }],
                resolved_answers: None,
                timestamp_ms: Some(23_500),
            },
        ];

        let (turns, perms, questions, session_diffs) =
            process_timeline_for_stage(&items, Some("/repo"));
        assert_eq!(turns.len(), 2);
        assert_eq!(perms.len(), 1);
        assert_eq!(questions.len(), 1);
        assert_eq!(session_diffs.len(), 1);

        // Turn 1: settled pure-thinking pre-reply folds to "Thought for 6s" with terminal_reply present
        let t1 = &turns[0];
        assert_eq!(t1.thought_duration_ms, Some(6_000));
        assert_eq!(t1.turn_duration_ms, Some(7_000));
        assert_eq!(t1.work_summary_label, "Thought for 6s");
        assert!(t1.terminal_reply.is_some());
        assert_eq!(t1.folded_steps.len(), 1);
        assert!(!t1.has_active_item);

        // Turn 2: running tool call -> no terminal_reply, active item true, live_label "Editing lib.rs…"
        let t2 = &turns[1];
        assert!(t2.terminal_reply.is_none());
        assert!(t2.has_active_item);
        assert_eq!(t2.work_summary_live_label, "Editing lib.rs…");
        assert_eq!(t2.turn_diffs.len(), 1);
        assert_eq!(t2.turn_diffs[0].path, "src/lib.rs");
        assert_eq!(t2.turn_diffs[0].additions, 5);
    }

    #[test]
    fn wakeup_live_detail_work_duration_compactions_and_insights_match_desktop() {
        use orbit_protocol::{AgentStatus, ProviderKind, SessionMode};

        let items = vec![
            TimelineItemView::UserPrompt {
                seq: 1,
                message_id: "m1".into(),
                text: "Start task".into(),
                badges: Vec::new(),
                blocks: Vec::new(),
                timestamp_ms: Some(1_000),
            },
            TimelineItemView::AssistantThinking {
                seq: 2,
                item_id: "th1".into(),
                text: "## Inspecting codebase\nChecking modules".into(),
                timestamp_ms: Some(2_000),
            },
            TimelineItemView::AssistantMessage {
                seq: 3,
                item_id: "a1".into(),
                text: "Interim reply at http://0.0.0.0:5173/app!".into(),
                streaming: false,
                blocks: Vec::new(),
                timestamp_ms: Some(5_000),
            },
            TimelineItemView::Notice {
                seq: 4,
                level: "wakeup".into(),
                message: "Resumed from background timer".into(),
                timestamp_ms: Some(6_000),
            },
            TimelineItemView::Notice {
                seq: 5,
                level: "info".into(),
                message: "Compacted conversation history".into(),
                timestamp_ms: Some(7_000),
            },
            make_tool(
                6,
                ToolCallStatus::Running,
                "Running cargo test --workspace",
                ToolCallDetail::Shell {
                    command: "cargo test --workspace".into(),
                    cwd: Some("/repo/crates".into()),
                    exit_code: None,
                    output_preview: Some("listening on http://127.0.0.1:8080/".into()),
                },
                Vec::new(),
                8_000,
            ),
            make_tool(
                7,
                ToolCallStatus::Completed,
                "Subagent review",
                ToolCallDetail::Subagent {
                    child_agent_id: Some("child_1".into()),
                    role: "Reviewer".into(),
                    prompt_preview: "Audit stage".into(),
                },
                Vec::new(),
                9_000,
            ),
        ];

        let (turns, _, _, _) = process_timeline_for_stage(&items, Some("/repo"));
        assert_eq!(turns.len(), 1);
        let t = &turns[0];
        assert!(t.terminal_reply.is_none());
        assert!(t.last_assistant_reply.is_some());
        assert_eq!(t.compactions.len(), 1);
        assert!(matches!(
            t.live_segments.iter().find(|s| matches!(s, TurnLiveSegmentView::Wakeup { .. })),
            Some(TurnLiveSegmentView::Wakeup { label, .. }) if label == "Resumed from background timer"
        ));

        // Thinking only group should emit live_label="Thinking" + live_detail="Checking modules"
        let thinking_only = vec![items[1].clone()];
        let think_sum = summarize_work_items(&thinking_only, None);
        assert_eq!(think_sum.live_label, "Thinking");
        assert_eq!(think_sum.live_detail.as_deref(), Some("Checking modules"));

        let cur_agent = AgentCardView {
            agent_id: "parent_1".into(),
            workspace_id: "ws_1".into(),
            cwd: "/repo".into(),
            title: "Main".into(),
            provider: ProviderKind::ClaudeCode,
            provider_label: "Claude Code".into(),
            model: None,
            effort: None,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::AutonomousWakeup {
                source: "cron".into(),
                turn_id: "wk_cron".into(),
            },
            status_label: "Waking (cron)".into(),
            is_running: true,
            is_awaiting_permission: false,
            parent_agent_id: None,
            parent_host_id: None,
            profile_id: None,
            background: false,
            auto_archive: false,
            attention: orbit_protocol::AttentionState::None,
            archived_at_rfc3339: None,
            labels: HashMap::new(),
            latest_seq: 7,
            updated_at_rfc3339: "2026-11-20T00:00:00Z".into(),
            is_pinned: false,
            seen_seq: 0,
            is_unread: false,
        };
        let insights = derive_session_insights(&items, Some("/repo"), Some(&cur_agent), &[]);
        assert_eq!(
            insights.detected_local_urls,
            vec![
                "http://localhost:5173/app".to_string(),
                "http://127.0.0.1:8080/".to_string()
            ]
        );
        assert_eq!(insights.background_tasks.len(), 2);
        assert_eq!(
            insights.background_tasks[0].kind,
            SessionBackgroundTaskKind::Wakeup
        );
        assert_eq!(
            insights.background_tasks[1].kind,
            SessionBackgroundTaskKind::Shell
        );
        assert_eq!(insights.subagents.len(), 1);

        let ctx = build_side_question_context_summary("/repo", Some("main"), &items, None);
        assert!(ctx.contains("Workspace: /repo (branch: main)"));
        assert!(ctx.contains("Tool [shell: cargo test --workspace] (running)"));
    }

    #[test]
    fn streaming_seq_updates_preserve_stable_turn_and_segment_keys() {
        let initial = vec![
            TimelineItemView::AssistantThinking {
                seq: 1,
                item_id: "think_1".into(),
                text: "Scanning modules".into(),
                timestamp_ms: Some(1_000),
            },
            TimelineItemView::AssistantMessage {
                seq: 2,
                item_id: "msg_1".into(),
                text: "| A | B |\n| - | - |\n| 1 | 2".into(),
                streaming: true,
                blocks: Vec::new(),
                timestamp_ms: Some(1_500),
            },
        ];
        let updated = vec![
            TimelineItemView::AssistantThinking {
                seq: 10,
                item_id: "think_1".into(),
                text: "Scanning modules deeply".into(),
                timestamp_ms: Some(1_000),
            },
            TimelineItemView::AssistantMessage {
                seq: 11,
                item_id: "msg_1".into(),
                text: "| A | B |\n| - | - |\n| 1 | 2 |\n\nMore trailing text...".into(),
                streaming: true,
                blocks: Vec::new(),
                timestamp_ms: Some(1_500),
            },
        ];

        let (turns_a, _, _, _) = process_timeline_for_stage(&initial, Some("/repo"));
        let (turns_b, _, _, _) = process_timeline_for_stage(&updated, Some("/repo"));
        assert_eq!(turns_a.len(), 1);
        assert_eq!(turns_b.len(), 1);
        assert_eq!(turns_a[0].key, turns_b[0].key);
        assert_eq!(turns_a[0].live_segments.len(), 2);
        assert_eq!(turns_b[0].live_segments.len(), 2);

        fn key_of(seg: &TurnLiveSegmentView) -> &str {
            match seg {
                TurnLiveSegmentView::StepGroup { key, .. } => key.as_str(),
                TurnLiveSegmentView::AssistantMessage { key, .. } => key.as_str(),
                TurnLiveSegmentView::Notice { key, .. } => key.as_str(),
                TurnLiveSegmentView::Compaction { key, .. } => key.as_str(),
                TurnLiveSegmentView::Wakeup { key, .. } => key.as_str(),
            }
        }
        assert_eq!(
            key_of(&turns_a[0].live_segments[0]),
            key_of(&turns_b[0].live_segments[0])
        );
        assert_eq!(
            key_of(&turns_a[0].live_segments[1]),
            key_of(&turns_b[0].live_segments[1])
        );
        assert_eq!(key_of(&turns_b[0].live_segments[1]), "msg-msg_1");
    }
}
