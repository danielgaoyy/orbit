//! Daemon-side Plan Mode approval enrichment and resolution orchestration.
//!
//! Coordinates mode transitions (`SessionMode::Plan` -> `SessionMode::Default`),
//! safe plan-file edit persistence, and provider-appropriate follow-up prompts
//! without duplicating prompts on providers that natively consume inline responses
//! (such as Claude Code's `ExitPlanMode` `control_response`).

use std::path::Path;

use orbit_protocol::{
    ActiveTurnBehavior, PendingPermissionSummary, PlanApprovalAction, PlanApprovalPayload,
    ProviderKind, TimelinePayload, ToolCallDetail, build_plan_resolution_message, is_plan_edited,
    looks_like_plan_file_path,
};
use orbit_store::OrbitStore;

/// Follow-up prompt to dispatch after resolving a plan approval gate, when the provider's
/// approval RPC cannot natively carry edited plans or revision feedback (or when the gate
/// was emitted at the end of a completed Plan Mode turn, as with Codex `<proposed_plan>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFollowupPrompt {
    pub wire_prompt: String,
    pub display_prompt: String,
    pub behavior: ActiveTurnBehavior,
}

/// Enrich a [`PlanApprovalPayload`] whose `plan_markdown` is empty by checking workspace
/// plan files and recent SQLite timeline events (`FileEdit` plan path or `AssistantMessage`).
pub fn enrich_plan_approval_from_store(
    store: &OrbitStore,
    agent_id: &str,
    workspace_cwd: &Path,
    payload: &mut PlanApprovalPayload,
) {
    let needs_file_verification = payload.plan_file_path.is_some() && !payload.plan_file_backed;
    if !payload.plan_markdown.trim().is_empty()
        && payload.plan_title.is_some()
        && !needs_file_verification
    {
        return;
    }

    let need_fallback_file =
        payload.plan_markdown.trim().is_empty() && payload.plan_file_path.is_none();
    let need_fallback_assistant = payload.plan_markdown.trim().is_empty();

    let mut fallback_file: Option<String> = None;
    let mut fallback_assistant: Option<String> = None;

    if (need_fallback_file || need_fallback_assistant)
        && let Ok(recent) = store.fetch_recent_timeline(agent_id, 24)
    {
        for ev in recent.iter().rev() {
            match &ev.payload {
                TimelinePayload::AssistantMessage { text, .. }
                    if need_fallback_assistant
                        && fallback_assistant.is_none()
                        && !text.trim().is_empty() =>
                {
                    fallback_assistant = Some(text.clone());
                }
                TimelinePayload::ToolCall {
                    detail: ToolCallDetail::FileEdit { path, .. },
                    ..
                } if need_fallback_file
                    && fallback_file.is_none()
                    && looks_like_plan_file_path(path) =>
                {
                    fallback_file = Some(path.clone());
                }
                _ => {}
            }
            if (!need_fallback_file || fallback_file.is_some())
                && (!need_fallback_assistant || fallback_assistant.is_some())
            {
                break;
            }
        }
    }

    orbit_providers::plan_approval::enrich_plan_payload(
        payload,
        workspace_cwd,
        fallback_file.as_deref(),
        fallback_assistant.as_deref(),
    );
}

/// Decide whether the daemon should send a follow-up prompt after resolving a plan gate.
pub fn decide_plan_followup(
    provider: ProviderKind,
    pending: Option<&PendingPermissionSummary>,
    request_id: &str,
    action: PlanApprovalAction,
    edited_plan_markdown: Option<&str>,
    feedback: Option<&str>,
    driver_is_live: bool,
) -> Option<PlanFollowupPrompt> {
    if matches!(action, PlanApprovalAction::Dismiss) {
        return None;
    }

    let original_plan = pending
        .and_then(|p| p.plan_approval.as_ref())
        .map(|p| p.plan_markdown.as_str())
        .unwrap_or("");
    let source = pending
        .and_then(|p| p.plan_approval.as_ref())
        .and_then(|p| p.source.as_deref());

    let edited = is_plan_edited(original_plan, edited_plan_markdown);
    let clean_feedback = feedback.map(str::trim).filter(|s| !s.is_empty());

    let is_synthetic_turn_end_gate = source == Some("codex_plan")
        || (request_id.starts_with("cplan_") && !request_id.starts_with("cplan_rpc_"));

    let wire_prompt =
        build_plan_resolution_message(action, original_plan, edited_plan_markdown, feedback);

    match action {
        PlanApprovalAction::ApproveAndBuild => {
            if is_synthetic_turn_end_gate || !driver_is_live {
                let display_prompt = match (edited, clean_feedback) {
                    (true, Some(notes)) => {
                        format!("Approved edited plan and started build: {notes}")
                    }
                    (true, None) => "Approved edited plan and started build.".to_string(),
                    (false, Some(notes)) => format!("Approved plan and started build: {notes}"),
                    (false, None) => "Approved plan and started build.".to_string(),
                };
                return Some(PlanFollowupPrompt {
                    wire_prompt,
                    display_prompt,
                    behavior: ActiveTurnBehavior::Queue,
                });
            }

            // Claude Code consumes `updatedInput.plan` and `userFeedback` directly inside
            // the `ExitPlanMode` `control_response` without needing an extra user turn.
            if provider == ProviderKind::ClaudeCode {
                return None;
            }

            // Non-Claude live RPC providers (Codex RPC, Antigravity, ACP, Pi):
            // When the user edited the plan, the provider driver declines/cancels the stale
            // unedited gate after switching mode, and we dispatch ONE clean build turn carrying
            // the approved edited plan text (and any user notes).
            if edited || clean_feedback.is_some() {
                let display_prompt = match (edited, clean_feedback) {
                    (true, Some(notes)) => {
                        format!("Approved edited plan and started build: {notes}")
                    }
                    (true, None) => "Approved edited plan and started build.".to_string(),
                    (false, Some(notes)) => notes.to_string(),
                    (false, None) => "Approved plan and started build.".to_string(),
                };
                return Some(PlanFollowupPrompt {
                    wire_prompt,
                    display_prompt,
                    behavior: ActiveTurnBehavior::Queue,
                });
            }
            None
        }
        PlanApprovalAction::RequestChanges => {
            // Claude Code consumes the inline `RequestChanges` feedback & edited plan
            // directly in the `ExitPlanMode` `deny` control response inside the active turn.
            if provider == ProviderKind::ClaudeCode && driver_is_live && !is_synthetic_turn_end_gate
            {
                return None;
            }

            let display_prompt = match (clean_feedback, edited) {
                (Some(notes), true) => format!("Requested plan changes (with edits): {notes}"),
                (Some(notes), false) => format!("Requested plan changes: {notes}"),
                (None, true) => "Requested plan changes using edited draft.".to_string(),
                (None, false) => "Requested changes to the plan.".to_string(),
            };
            Some(PlanFollowupPrompt {
                wire_prompt,
                display_prompt,
                behavior: ActiveTurnBehavior::Queue,
            })
        }
        PlanApprovalAction::Dismiss => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use orbit_protocol::ToolCallDetail;

    fn sample_pending(source: &str, plan_markdown: &str) -> PendingPermissionSummary {
        PendingPermissionSummary {
            agent_id: "ag_1".into(),
            agent_title: "Planner".into(),
            workspace_id: "ws_1".into(),
            request_id: if source == "codex_plan" {
                "cplan_123".into()
            } else {
                "clp_123".into()
            },
            title: "Plan".into(),
            detail: ToolCallDetail::Generic {
                tool_name: "ExitPlanMode".into(),
                arguments_json: "{}".into(),
            },
            allowed_decisions: vec!["allow".into(), "deny".into()],
            guardian_assessment: None,
            plan_approval: Some(PlanApprovalPayload {
                plan_markdown: plan_markdown.into(),
                plan_title: Some("Plan".into()),
                plan_file_path: None,
                summary: None,
                plan_file_backed: false,
                allowed_prompts: Vec::new(),
                source: Some(source.into()),
            }),
            human_only: true,
            router_recommendation: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn claude_live_exit_plan_mode_needs_no_duplicate_followup_prompt() {
        let pending = sample_pending("exit_plan_mode", "# Plan\n- Step 1");
        assert!(
            decide_plan_followup(
                ProviderKind::ClaudeCode,
                Some(&pending),
                &pending.request_id,
                PlanApprovalAction::ApproveAndBuild,
                Some("# Plan\n- Step 1\n- Step 2"),
                None,
                true,
            )
            .is_none()
        );
        assert!(
            decide_plan_followup(
                ProviderKind::ClaudeCode,
                Some(&pending),
                &pending.request_id,
                PlanApprovalAction::RequestChanges,
                None,
                Some("Add rollback step"),
                true,
            )
            .is_none()
        );
        assert!(
            decide_plan_followup(
                ProviderKind::ClaudeCode,
                Some(&pending),
                &pending.request_id,
                PlanApprovalAction::Dismiss,
                None,
                None,
                true,
            )
            .is_none()
        );
    }

    #[test]
    fn codex_synthetic_plan_sends_kickoff_and_revision_prompts() {
        let pending = sample_pending("codex_plan", "# Plan\n- Step 1");
        let approve = decide_plan_followup(
            ProviderKind::Codex,
            Some(&pending),
            &pending.request_id,
            PlanApprovalAction::ApproveAndBuild,
            Some("# Plan\n- Step 1\n- Step 2"),
            None,
            true,
        );
        let approve = match approve {
            Some(f) => f,
            None => panic!("expected codex synthetic approve followup"),
        };
        assert!(approve.wire_prompt.contains("- Step 2"));
        assert!(approve.display_prompt.contains("edited plan"));

        let revise = decide_plan_followup(
            ProviderKind::Codex,
            Some(&pending),
            &pending.request_id,
            PlanApprovalAction::RequestChanges,
            None,
            Some("Keep migrations backward compatible"),
            true,
        );
        let revise = match revise {
            Some(f) => f,
            None => panic!("expected codex synthetic revision followup"),
        };
        assert!(revise.wire_prompt.contains("Stay in plan mode"));
        assert!(
            revise
                .wire_prompt
                .contains("Keep migrations backward compatible")
        );

        assert!(
            decide_plan_followup(
                ProviderKind::Codex,
                Some(&pending),
                &pending.request_id,
                PlanApprovalAction::Dismiss,
                None,
                None,
                true,
            )
            .is_none()
        );
    }

    #[test]
    fn acp_live_plan_forwards_edits_or_request_changes_but_skips_unmodified_approve() {
        let pending = sample_pending("acp_plan", "# Plan\n- Step 1");
        assert!(
            decide_plan_followup(
                ProviderKind::OpenCode,
                Some(&pending),
                &pending.request_id,
                PlanApprovalAction::ApproveAndBuild,
                Some("# Plan\n- Step 1"),
                None,
                true,
            )
            .is_none()
        );

        let edited = decide_plan_followup(
            ProviderKind::OpenCode,
            Some(&pending),
            &pending.request_id,
            PlanApprovalAction::ApproveAndBuild,
            Some("# Plan\n- Step 1\n- Step 2"),
            None,
            true,
        );
        assert!(edited.is_some());

        let revise = decide_plan_followup(
            ProviderKind::OpenCode,
            Some(&pending),
            &pending.request_id,
            PlanApprovalAction::RequestChanges,
            None,
            Some("Use SQLite instead"),
            true,
        );
        assert!(revise.is_some());
    }
}
