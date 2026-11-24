//! Structured Plan Mode approval payloads, actions, and cross-provider extraction helpers.
//!
//! Supports Claude Code (`ExitPlanMode`), Codex Plan Mode (`<proposed_plan>` / plan items /
//! plan approval RPCs), Antigravity (`notify_user` / implementation plan review), and generic
//! ACP plan approval gates.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ToolCallDetail;

/// Structured implementation-plan approval gate surfaced when an agent finishes
/// drafting a plan and asks the user to review before executing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanApprovalPayload {
    /// Full Markdown plan content presented for review and inline editing.
    pub plan_markdown: String,
    /// Optional concise title (explicitly provided or derived from the plan's H1/H2 heading).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_title: Option<String>,
    /// Optional path to a backing plan file on disk (e.g. `~/.claude/plans/foo.md` or
    /// `implementation_plan.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_file_path: Option<String>,
    /// Optional provider review summary/subtitle kept strictly separate from editable
    /// `plan_markdown` (e.g. Antigravity `notify_user` `Message` when reviewing a file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// `true` ONLY when `plan_markdown` was verified to come from `plan_file_path` on disk.
    /// Disk write-back on user edits is strictly gated on this flag.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plan_file_backed: bool,
    /// Optional requested tool/command permissions attached to the plan (e.g. Claude `allowedPrompts`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_prompts: Vec<String>,
    /// Provider origin hint (`"exit_plan_mode"` | `"codex_plan"` | `"antigravity_plan"` | `"acp_plan"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Explicit user decision on a pending [`PlanApprovalPayload`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanApprovalAction {
    /// Approve the plan (optionally with user-edited Markdown), exit Plan mode into
    /// execution mode (default: `SessionMode::Default`), and begin building immediately.
    ApproveAndBuild,
    /// Keep the session in `SessionMode::Plan` and ask the agent to revise the plan
    /// using the user's inline feedback and/or edited plan text.
    RequestChanges,
    /// Cleanly dismiss/reject the gate without switching out of Plan mode.
    Dismiss,
}

impl PlanApprovalAction {
    /// Canonical persisted decision slug stored in `TimelinePayload::PermissionResolved`.
    pub const fn as_decision_str(self) -> &'static str {
        match self {
            Self::ApproveAndBuild => "approve_and_build",
            Self::RequestChanges => "request_changes",
            Self::Dismiss => "dismiss",
        }
    }
}

/// Strip common MCP / provider namespace prefixes (`mcp__server__tool`, `server/tool`, `server:tool`)
/// and normalize to lowercase ASCII alphanumeric characters for resilient tool matching.
fn normalize_tool_leaf(tool_name: &str) -> String {
    let trimmed = tool_name.trim();
    let leaf = trimmed
        .rsplit("__")
        .next()
        .unwrap_or(trimmed)
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .rsplit(':')
        .next()
        .unwrap_or(trimmed);
    leaf.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Returns `true` if `tool_name` represents a dedicated plan-exit / plan-approval tool.
pub fn is_plan_approval_tool_name(tool_name: &str) -> bool {
    matches!(
        normalize_tool_leaf(tool_name).as_str(),
        "exitplanmode"
            | "approveplan"
            | "planapproval"
            | "submitplan"
            | "proposeplan"
            | "reviewplan"
            | "requestplanapproval"
    )
}

/// Returns `true` if `tool_name` is Antigravity's `notify_user` review tool.
pub fn is_notify_user_tool_name(tool_name: &str) -> bool {
    normalize_tool_leaf(tool_name) == "notifyuser"
}

/// Returns `true` if `path` looks like a Markdown implementation plan artifact file.
pub fn looks_like_plan_file_path(path: &str) -> bool {
    let norm = path.trim().replace('\\', "/").to_ascii_lowercase();
    if norm.is_empty() {
        return false;
    }
    let is_md = norm.ends_with(".md") || norm.ends_with(".markdown");
    if !is_md {
        return false;
    }
    let basename = norm.rsplit('/').next().unwrap_or(&norm);
    norm.contains("/.claude/plans/")
        || norm.contains("/plans/")
        || basename.contains("plan")
        || basename.contains("spec")
        || basename.contains("rfc")
}

/// Extract a concise title from the first Markdown heading (`# ...` or `## ...`) in `markdown`.
pub fn extract_plan_title(markdown: &str) -> Option<String> {
    for line in markdown.lines().take(24) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            let heading = rest
                .trim_start_matches('#')
                .trim()
                .trim_end_matches('#')
                .trim();
            if !heading.is_empty() {
                return Some(truncate_title(heading, 120));
            }
        }
    }
    None
}

fn truncate_title(raw: &str, max_chars: usize) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let keep = max_chars.saturating_sub(1);
    let prefix: String = collapsed.chars().take(keep).collect();
    format!("{}…", prefix.trim_end())
}

/// Extract the Markdown body inside a Codex `<proposed_plan>...</proposed_plan>` envelope.
pub fn extract_codex_proposed_plan(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let open_start = lower.find("<proposed_plan")?;
    let after_open = text.get(open_start..)?;
    let close_bracket = after_open.find('>')?;
    let body_start = open_start + close_bracket + 1;
    let rest_lower = lower.get(body_start..)?;
    let close_rel = rest_lower.find("</proposed_plan>")?;
    let body = text.get(body_start..body_start + close_rel)?.trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

/// Extract requested prompt/permission summaries from Claude Code `allowedPrompts` (or similar arrays).
pub fn extract_allowed_prompts(input: &Value) -> Vec<String> {
    let Some(arr) = input
        .get("allowedPrompts")
        .or_else(|| input.get("allowed_prompts"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in arr {
        if let Some(s) = entry.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        } else if let Some(obj) = entry.as_object() {
            let tool = obj
                .get("tool")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let prompt = obj
                .get("prompt")
                .or_else(|| obj.get("command"))
                .or_else(|| obj.get("description"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            match (tool, prompt) {
                (Some(t), Some(p)) => out.push(format!("{t}: {p}")),
                (None, Some(p)) => out.push(p.to_string()),
                (Some(t), None) => out.push(t.to_string()),
                (None, None) => {}
            }
        }
    }
    out
}

/// Extract a backing plan file path from a tool input JSON object, if present.
pub fn extract_plan_file_path(input: &Value) -> Option<String> {
    for key in [
        "planFilePath",
        "plan_file_path",
        "filePath",
        "file_path",
        "planPath",
        "plan_path",
        "path",
    ] {
        if let Some(s) = input.get(key).and_then(Value::as_str) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    for arr_key in ["PathsToReview", "paths_to_review", "pathsToReview"] {
        if let Some(arr) = input.get(arr_key).and_then(Value::as_array) {
            let mut first_non_empty: Option<String> = None;
            for item in arr {
                let Some(s) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                    continue;
                };
                if looks_like_plan_file_path(s) {
                    return Some(s.to_string());
                }
                if first_non_empty.is_none() {
                    first_non_empty = Some(s.to_string());
                }
            }
            if let Some(first) = first_non_empty {
                return Some(first);
            }
        }
    }
    None
}

fn extract_plan_markdown_field(input: &Value, allow_content_key: bool) -> Option<String> {
    let keys: &[&str] = if allow_content_key {
        &[
            "plan",
            "plan_markdown",
            "planMarkdown",
            "markdown",
            "proposed_plan",
            "proposedPlan",
            "content",
        ]
    } else {
        &[
            "plan",
            "plan_markdown",
            "planMarkdown",
            "markdown",
            "proposed_plan",
            "proposedPlan",
        ]
    };
    for key in keys {
        if let Some(s) = input.get(*key).and_then(Value::as_str) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_explicit_title(input: &Value) -> Option<String> {
    for key in ["plan_title", "planTitle", "title", "summary"] {
        if let Some(s) = input.get(key).and_then(Value::as_str) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(truncate_title(trimmed, 120));
            }
        }
    }
    None
}

fn clean_fallback_title(fallback_title: Option<&str>) -> Option<String> {
    let raw = fallback_title?.trim();
    if raw.is_empty() {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    if lower == "exitplanmode"
        || lower == "exit_plan_mode"
        || lower.starts_with("claude wants to use exitplanmode")
        || lower == "approve tool execution"
        || lower == "review implementation plan"
        || lower == "plan approval"
    {
        return None;
    }
    Some(truncate_title(raw, 120))
}

/// Detect and extract a structured [`PlanApprovalPayload`] from an explicit provider
/// plan-review tool call (`ExitPlanMode` / `approve_plan` / Antigravity `notify_user`).
///
/// Ordinary file-editing or shell tools (`Write`, `Edit`, `Bash`, etc.) are NEVER promoted
/// to plan-approval gates even while in `SessionMode::Plan`.
pub fn extract_plan_approval_from_tool_input(
    tool_name: &str,
    input: &Value,
    fallback_title: Option<&str>,
    in_plan_mode: bool,
    source_override: Option<&str>,
) -> Option<PlanApprovalPayload> {
    let explicit_plan_tool = is_plan_approval_tool_name(tool_name);
    let notify_user_tool = is_notify_user_tool_name(tool_name);
    if !explicit_plan_tool && !notify_user_tool {
        return None;
    }

    let plan_file_path = extract_plan_file_path(input);
    let allowed_prompts = extract_allowed_prompts(input);

    if explicit_plan_tool {
        let plan_markdown = extract_plan_markdown_field(input, true).unwrap_or_default();
        let plan_title = extract_explicit_title(input)
            .or_else(|| extract_plan_title(&plan_markdown))
            .or_else(|| clean_fallback_title(fallback_title));
        let source = source_override.unwrap_or("exit_plan_mode").to_string();
        return Some(PlanApprovalPayload {
            plan_markdown,
            plan_title,
            plan_file_path,
            summary: None,
            plan_file_backed: false,
            allowed_prompts,
            source: Some(source),
        });
    }

    if notify_user_tool {
        let blocked_on_user = input
            .get("BlockedOnUser")
            .or_else(|| input.get("blocked_on_user"))
            .or_else(|| input.get("blockedOnUser"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !blocked_on_user {
            return None;
        }
        let has_plan_artifact = plan_file_path
            .as_deref()
            .is_some_and(looks_like_plan_file_path);
        let plan_field = extract_plan_markdown_field(input, false);
        let message_body = input
            .get("Message")
            .or_else(|| input.get("message"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        if has_plan_artifact {
            // Keep the notification `Message` strictly separate as `summary` so enrichment
            // loads the actual verified plan artifact file from disk into `plan_markdown`
            // and editing can never clobber a full plan file with a short notification summary.
            let plan_markdown = plan_field.unwrap_or_default();
            let plan_title = extract_explicit_title(input)
                .or_else(|| extract_plan_title(&plan_markdown))
                .or_else(|| clean_fallback_title(fallback_title));
            return Some(PlanApprovalPayload {
                plan_markdown,
                plan_title,
                plan_file_path,
                summary: message_body,
                plan_file_backed: false,
                allowed_prompts,
                source: Some(source_override.unwrap_or("antigravity_plan").to_string()),
            });
        }

        if in_plan_mode && (plan_field.is_some() || message_body.is_some()) {
            let (plan_markdown, summary) = match (plan_field, message_body) {
                (Some(plan), msg) => (plan, msg),
                (None, Some(msg)) => (msg, None),
                (None, None) => (String::new(), None),
            };
            let plan_title = extract_explicit_title(input)
                .or_else(|| extract_plan_title(&plan_markdown))
                .or_else(|| clean_fallback_title(fallback_title));
            return Some(PlanApprovalPayload {
                plan_markdown,
                plan_title,
                plan_file_path: None,
                summary,
                plan_file_backed: false,
                allowed_prompts,
                source: Some(source_override.unwrap_or("antigravity_plan").to_string()),
            });
        }
    }

    None
}

/// Backward-compatible fallback: synthesize [`PlanApprovalPayload`] from a generic
/// [`ToolCallDetail`] when an older daemon emitted a raw `ExitPlanMode` permission request.
pub fn extract_plan_approval_from_detail(
    detail: &ToolCallDetail,
    fallback_title: Option<&str>,
    in_plan_mode: bool,
) -> Option<PlanApprovalPayload> {
    let ToolCallDetail::Generic {
        tool_name,
        arguments_json,
    } = detail
    else {
        return None;
    };
    let parsed_input = serde_json::from_str::<Value>(arguments_json).unwrap_or(Value::Null);
    extract_plan_approval_from_tool_input(
        tool_name,
        &parsed_input,
        fallback_title,
        in_plan_mode,
        None,
    )
}

/// Returns `true` when `edited_plan` meaningfully differs from `original_plan`.
pub fn is_plan_edited(original_plan: &str, edited_plan: Option<&str>) -> bool {
    let Some(edited) = edited_plan else {
        return false;
    };
    normalize_newlines(original_plan).trim() != normalize_newlines(edited).trim()
}

fn normalize_newlines(raw: &str) -> String {
    raw.replace("\r\n", "\n")
}

/// Format the canonical agent-facing guidance message for a plan approval decision.
///
/// Used both for inline tool deny/feedback messages (Claude Code `ExitPlanMode`) and for
/// follow-up prompts on providers whose approval RPC does not carry edited plan payloads.
pub fn build_plan_resolution_message(
    action: PlanApprovalAction,
    original_plan: &str,
    edited_plan: Option<&str>,
    feedback: Option<&str>,
) -> String {
    let clean_feedback = feedback.map(str::trim).filter(|s| !s.is_empty());
    let edited_differs = is_plan_edited(original_plan, edited_plan);
    let clean_edited = edited_plan
        .filter(|_| edited_differs)
        .map(|s| normalize_newlines(s).trim().to_string())
        .filter(|s| !s.is_empty());

    match action {
        PlanApprovalAction::ApproveAndBuild => match (clean_edited, clean_feedback) {
            (Some(updated), Some(notes)) => format!(
                "Plan approved with user edits. Proceed with implementation using the updated plan below and these notes.\n\nUser notes:\n{notes}\n\nUpdated plan:\n{updated}"
            ),
            (Some(updated), None) => format!(
                "Plan approved with user edits. Proceed with implementation using the updated plan below:\n\n{updated}"
            ),
            (None, Some(notes)) => {
                format!("Plan approved. Proceed with implementation.\n\nUser notes:\n{notes}")
            }
            (None, None) => "Plan approved. Proceed with implementation.".to_string(),
        },
        PlanApprovalAction::RequestChanges => match (clean_feedback, clean_edited) {
            (Some(notes), Some(updated)) => format!(
                "Stay in plan mode and revise the implementation plan based on the user's feedback and edits below, then request plan approval again.\n\nFeedback:\n{notes}\n\nUser-edited draft:\n{updated}"
            ),
            (Some(notes), None) => format!(
                "Stay in plan mode and revise the implementation plan based on the user's feedback below, then request plan approval again.\n\nFeedback:\n{notes}"
            ),
            (None, Some(updated)) => format!(
                "Stay in plan mode and refine the implementation plan around the user's edited draft below, then request plan approval again.\n\nUser-edited draft:\n{updated}"
            ),
            (None, None) => {
                "Stay in plan mode and revise the implementation plan before proceeding."
                    .to_string()
            }
        },
        PlanApprovalAction::Dismiss => {
            "User dismissed plan approval from Orbit. Stay in plan mode and wait for further instructions."
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_claude_exit_plan_mode_with_plan_heading_and_allowed_prompts() {
        let input = json!({
            "plan": "# Auth Overhaul Plan\n\n1. Add JWT refresh rotation\n2. Update tests",
            "planFilePath": "/Users/dev/.claude/plans/auth-overhaul.md",
            "allowedPrompts": [
                { "tool": "Bash", "prompt": "run cargo test" },
                "npm run build"
            ]
        });

        let payload = extract_plan_approval_from_tool_input(
            "ExitPlanMode",
            &input,
            Some("Claude wants to use ExitPlanMode"),
            true,
            None,
        );
        let payload = match payload {
            Some(p) => p,
            None => panic!("expected ExitPlanMode payload"),
        };
        assert_eq!(payload.plan_title.as_deref(), Some("Auth Overhaul Plan"));
        assert!(payload.plan_markdown.contains("Add JWT refresh rotation"));
        assert_eq!(
            payload.plan_file_path.as_deref(),
            Some("/Users/dev/.claude/plans/auth-overhaul.md")
        );
        assert_eq!(
            payload.allowed_prompts,
            vec!["Bash: run cargo test", "npm run build"]
        );
        assert_eq!(payload.source.as_deref(), Some("exit_plan_mode"));
    }

    #[test]
    fn extracts_exit_plan_mode_even_when_input_is_empty_object() {
        let payload = extract_plan_approval_from_tool_input(
            "mcp__claude__exit_plan_mode",
            &json!({}),
            None,
            true,
            None,
        );
        assert!(payload.is_some());
        if let Some(p) = payload {
            assert!(p.plan_markdown.is_empty());
            assert_eq!(p.source.as_deref(), Some("exit_plan_mode"));
        }
    }

    #[test]
    fn extracts_antigravity_notify_user_plan_review_only_when_blocked_with_plan() {
        let blocked_plan = json!({
            "BlockedOnUser": true,
            "PathsToReview": ["/tmp/brain/implementation_plan.md"],
            "Message": "Ready for your review before editing code."
        });
        let extracted =
            extract_plan_approval_from_tool_input("notify_user", &blocked_plan, None, false, None);
        let extracted = match extracted {
            Some(p) => p,
            None => panic!("expected antigravity plan review"),
        };
        assert_eq!(
            extracted.plan_file_path.as_deref(),
            Some("/tmp/brain/implementation_plan.md")
        );
        // Notification summary stays in `summary`, NOT `plan_markdown`, so disk enrichment loads
        // the real plan artifact file instead of clobbering it with a short notification message.
        assert!(extracted.plan_markdown.is_empty());
        assert_eq!(
            extracted.summary.as_deref(),
            Some("Ready for your review before editing code.")
        );
        assert!(!extracted.plan_file_backed);
        assert_eq!(extracted.source.as_deref(), Some("antigravity_plan"));

        let unblocked = json!({
            "BlockedOnUser": false,
            "PathsToReview": ["/tmp/brain/implementation_plan.md"],
            "Message": "Status update"
        });
        assert!(
            extract_plan_approval_from_tool_input("notify_user", &unblocked, None, true, None)
                .is_none()
        );
    }

    #[test]
    fn never_promotes_ordinary_write_or_edit_tools_to_plan_approval() {
        let write_input = json!({
            "file_path": "/repo/docs/implementation_plan.md",
            "content": "# Plan\n\n- Step 1\n- Step 2"
        });
        assert!(
            extract_plan_approval_from_tool_input("Write", &write_input, None, true, None)
                .is_none(),
            "ordinary Write tool call in Plan mode must NEVER be promoted to a plan approval gate"
        );
        assert!(
            extract_plan_approval_from_tool_input("Edit", &write_input, None, true, None).is_none(),
            "ordinary Edit tool call in Plan mode must NEVER be promoted to a plan approval gate"
        );
        let custom_with_plan_key = json!({
            "plan": "# Something",
            "command": "echo hi"
        });
        assert!(
            extract_plan_approval_from_tool_input("Bash", &custom_with_plan_key, None, true, None)
                .is_none(),
            "ordinary Bash tool call must NEVER be promoted to a plan approval gate"
        );
    }

    #[test]
    fn extracts_codex_proposed_plan_envelope() {
        let msg = "Here is the proposal:\n<proposed_plan>\n# Migrate Cache\n\n- Step 1\n- Step 2\n</proposed_plan>\nLet me know.";
        let plan = extract_codex_proposed_plan(msg);
        assert_eq!(
            plan.as_deref(),
            Some("# Migrate Cache\n\n- Step 1\n- Step 2")
        );
        assert_eq!(
            plan.as_deref().and_then(extract_plan_title).as_deref(),
            Some("Migrate Cache")
        );
    }

    #[test]
    fn builds_clean_resolution_messages_for_all_actions() {
        let orig = "# Plan\n- Step 1";
        let unchanged = build_plan_resolution_message(
            PlanApprovalAction::ApproveAndBuild,
            orig,
            Some("# Plan\r\n- Step 1\n"),
            None,
        );
        assert_eq!(unchanged, "Plan approved. Proceed with implementation.");

        let edited = build_plan_resolution_message(
            PlanApprovalAction::ApproveAndBuild,
            orig,
            Some("# Plan\n- Step 1\n- Step 2 (new)"),
            None,
        );
        assert!(edited.contains("Plan approved with user edits"));
        assert!(edited.contains("- Step 2 (new)"));

        let revise = build_plan_resolution_message(
            PlanApprovalAction::RequestChanges,
            orig,
            None,
            Some("Avoid adding new dependencies"),
        );
        assert!(revise.contains("Stay in plan mode"));
        assert!(revise.contains("Avoid adding new dependencies"));

        let dismiss = build_plan_resolution_message(PlanApprovalAction::Dismiss, orig, None, None);
        assert!(dismiss.contains("dismissed"));
    }

    #[test]
    fn fallback_from_generic_tool_call_detail_works() {
        let detail = ToolCallDetail::Generic {
            tool_name: "ExitPlanMode".into(),
            arguments_json: r##"{"plan":"# Ship It\n\n- Build\n- Verify"}"##.into(),
        };
        let payload = extract_plan_approval_from_detail(&detail, None, true);
        let payload = match payload {
            Some(p) => p,
            None => panic!("expected fallback plan payload"),
        };
        assert_eq!(payload.plan_title.as_deref(), Some("Ship It"));
        assert!(payload.plan_markdown.contains("- Verify"));
    }
}
