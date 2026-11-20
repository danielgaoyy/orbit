//! Semantic conversation turn coalescing, tool step descriptors, thinking headline extraction,
//! and work-summary bucketing shared across iOS (SwiftUI), Android (Compose), HarmonyOS (ArkUI V2),
//! and Desktop.
//!
//! Direct pure-Rust port of `apps/desktop/src/lib/timeline-coalesce.ts`.

use std::collections::HashMap;

use orbit_protocol::{ToolCallDetail, ToolCallStatus};
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
    /// Active streaming summary label while steps are still in flight (e.g. `Reading omni-composer.tsx…`).
    pub live_label: String,
    /// Whether the primary `label` already embeds failure wording (e.g. `Command failed`).
    pub label_includes_failure: bool,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTurnView {
    pub key: String,
    pub turn_index: u32,
    pub anchor_seq: u64,
    pub user_prompt: Option<TimelineItemView>,
    pub is_compaction_command: bool,
    /// Intermediate steps prior to `terminal_reply` (eligible for pre-reply auto-fold).
    pub folded_steps: Vec<TimelineItemView>,
    /// Aggregated counts of work performed inside `folded_steps`.
    pub work_counts: TurnWorkCountsView,
    /// Settled summary sentence for `folded_steps` (`Edited 2 files, ran a command`).
    pub work_summary_label: String,
    /// Streaming summary label while steps in `folded_steps` are in flight.
    pub work_summary_live_label: String,
    /// Whether `work_summary_label` already states failure (`Command failed`).
    pub work_summary_includes_failure: bool,
    /// Duration of thinking blocks in milliseconds when >= 1000ms.
    pub thought_duration_ms: Option<i64>,
    /// End-to-end turn duration in milliseconds when timestamps are available.
    pub turn_duration_ms: Option<i64>,
    /// Final synthesis reply of the turn, kept visible when pre-reply work folds.
    pub terminal_reply: Option<TimelineItemView>,
    /// Notices (warnings/errors) surfaced unconditionally so system alerts are never buried.
    pub notices: Vec<TimelineItemView>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStageView {
    pub revision: u64,
    pub host_id: String,
    pub agent: Option<AgentCardView>,
    pub turns: Vec<StageTurnView>,
    pub pending_permissions: Vec<TimelineItemView>,
    pub pending_questions: Vec<TimelineItemView>,
    pub session_turn_diffs: Vec<DiffFileView>,
    pub latest_seq: u64,
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
    let live_label = build_live_step_label(active_tail);

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
        label_includes_failure,
    }
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
    let mut turns: Vec<StageTurnView> = Vec::new();
    let mut pending_permissions: Vec<TimelineItemView> = Vec::new();
    let mut pending_questions: Vec<TimelineItemView> = Vec::new();
    let mut session_turn_diffs: Vec<DiffFileView> = Vec::new();

    let mut current_prompt: Option<TimelineItemView> = None;
    let mut current_body: Vec<TimelineItemView> = Vec::new();
    let mut has_open_turn = false;

    let flush_turn = |turns: &mut Vec<StageTurnView>,
                      current_prompt: &mut Option<TimelineItemView>,
                      current_body: &mut Vec<TimelineItemView>,
                      has_open_turn: &mut bool| {
        if !*has_open_turn {
            return;
        }
        let idx = turns.len() as u32;
        let prompt = current_prompt.take();
        let body = std::mem::take(current_body);
        *has_open_turn = false;
        turns.push(finalize_turn(idx, prompt, body, cwd));
    };

    for raw_item in items {
        let item = enrich_item_descriptor(raw_item, cwd);
        if let TimelineItemView::ToolCall { ref diff_files, .. } = item {
            for df in diff_files {
                session_turn_diffs.push(df.clone());
            }
        }

        if let TimelineItemView::PermissionRequest {
            ref resolved_decision,
            ..
        } = item
            && resolved_decision.is_none()
        {
            pending_permissions.push(item);
            continue;
        }

        if let TimelineItemView::QuestionRequest {
            ref resolved_answers,
            ..
        } = item
            && resolved_answers.is_none()
        {
            pending_questions.push(item);
            continue;
        }

        if matches!(
            item,
            TimelineItemView::UserPrompt { .. } | TimelineItemView::AgentMessage { .. }
        ) {
            flush_turn(
                &mut turns,
                &mut current_prompt,
                &mut current_body,
                &mut has_open_turn,
            );
            current_prompt = Some(item);
            current_body.clear();
            has_open_turn = true;
            continue;
        }

        if !has_open_turn {
            has_open_turn = true;
            current_prompt = None;
        }
        current_body.push(item);
    }

    flush_turn(
        &mut turns,
        &mut current_prompt,
        &mut current_body,
        &mut has_open_turn,
    );

    (
        turns,
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
    user_prompt: Option<TimelineItemView>,
    body_items: Vec<TimelineItemView>,
    cwd: Option<&str>,
) -> StageTurnView {
    let anchor_seq = user_prompt
        .as_ref()
        .map(timeline_item_seq)
        .or_else(|| body_items.first().map(timeline_item_seq))
        .unwrap_or(u64::from(turn_index));

    let key = match &user_prompt {
        Some(TimelineItemView::UserPrompt { message_id, .. }) => format!("turn-{message_id}"),
        Some(TimelineItemView::AgentMessage { message_id, .. }) => format!("turn-am-{message_id}"),
        _ => format!("turn-init-{anchor_seq}"),
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

        if let TimelineItemView::Notice { ref message, .. } = *it {
            if !contains_ascii_case_insensitive(message, "compact") {
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

    let thought_duration_ms = compute_thought_duration_ms(&non_notice_body, None);
    let summary = summarize_work_items(&folded_steps, thought_duration_ms);
    let full_turn_tool_summary = summarize_work_items(&non_notice_body, thought_duration_ms);

    let started_at_ms = user_prompt
        .as_ref()
        .and_then(timeline_item_timestamp_ms)
        .or_else(|| body_items.iter().find_map(timeline_item_timestamp_ms));

    let completed_at_ms = terminal_reply
        .as_ref()
        .and_then(timeline_item_timestamp_ms)
        .or_else(|| body_items.iter().rev().find_map(timeline_item_timestamp_ms))
        .or(started_at_ms);

    let turn_duration_ms = match (started_at_ms, completed_at_ms) {
        (Some(start), Some(end)) if end > start => Some(end - start),
        _ => None,
    };

    let turn_diffs: Vec<TurnFileDiffSummaryView> = diff_paths_order
        .into_iter()
        .filter_map(|p| diff_by_path.remove(&p))
        .collect();

    StageTurnView {
        key,
        turn_index,
        anchor_seq,
        user_prompt,
        is_compaction_command,
        folded_steps,
        work_counts: summary.counts,
        work_summary_label: summary.label,
        work_summary_live_label: summary.live_label,
        work_summary_includes_failure: summary.label_includes_failure,
        thought_duration_ms,
        turn_duration_ms,
        terminal_reply,
        notices,
        live_segments: build_live_segments(&body_items),
        turn_diffs,
        has_error,
        has_unrecovered_error: full_turn_tool_summary.counts.unrecovered_failure_count > 0,
        has_active_item,
        started_at_ms,
        completed_at_ms,
    }
}

fn build_live_segments(body_items: &[TimelineItemView]) -> Vec<TurnLiveSegmentView> {
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
        let items = std::mem::take(step_buf);
        let summary = summarize_work_items(&items, thought_ms);
        raw_segments.push(TurnLiveSegmentView::StepGroup {
            key: format!("steps-{first_seq}"),
            first_seq,
            items,
            summary,
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
            TimelineItemView::AssistantMessage { seq, item_id, .. } => {
                flush_steps(&mut raw_segments, &mut step_buf, true, Some(it));
                raw_segments.push(TurnLiveSegmentView::AssistantMessage {
                    key: format!("msg-{item_id}-{seq}"),
                    item: it.clone(),
                });
            }
            TimelineItemView::Notice { seq, message, .. } => {
                if contains_ascii_case_insensitive(message, "compact") {
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

fn build_live_step_label(item: Option<&TimelineItemView>) -> String {
    let Some(item) = item else {
        return "Working…".to_string();
    };
    match item {
        TimelineItemView::AssistantThinking { text, .. } => {
            let headline = extract_thinking_headline(text, true);
            if headline.is_empty() {
                "Thinking…".to_string()
            } else {
                headline
            }
        }
        TimelineItemView::ToolCall {
            summary_title,
            detail,
            ..
        } => match detail {
            ToolCallDetail::FileRead { path, .. } => {
                let name = basename_of_path(path);
                if !name.is_empty() {
                    format!("Reading {name}…")
                } else {
                    "Reading a file…".to_string()
                }
            }
            ToolCallDetail::FileEdit { path, .. } => {
                let is_create = is_file_create_detail(summary_title, detail);
                let name = basename_of_path(path);
                let verb = if is_create { "Creating" } else { "Editing" };
                if !name.is_empty() {
                    format!("{verb} {name}…")
                } else {
                    format!("{verb} a file…")
                }
            }
            ToolCallDetail::Search { query, .. } => {
                match classify_search_tool(query, summary_title) {
                    SearchSubkind::Web => "Searching the web…".to_string(),
                    SearchSubkind::Glob => "Looking for files…".to_string(),
                    SearchSubkind::Grep => "Searching files…".to_string(),
                }
            }
            ToolCallDetail::Shell { .. } => "Running a command…".to_string(),
            ToolCallDetail::Subagent { .. } => "Running subagent…".to_string(),
            ToolCallDetail::Generic { tool_name, .. } => {
                let parsed = parse_generic_tool_name(tool_name);
                if let Some(server) = parsed.server_name {
                    format!("Using {server}…")
                } else if !parsed.display_label.is_empty() {
                    format!("Using {}…", parsed.display_label)
                } else {
                    "Using a tool…".to_string()
                }
            }
        },
        _ => "Working…".to_string(),
    }
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
}
