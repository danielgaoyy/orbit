use orbit_client_core::personal_agent::PersonalAgentBriefingView;
use orbit_client_core::render::{TerminalScreenView, parse_markdown_blocks};
use orbit_client_core::sidebar::FleetSidebarView;
use orbit_client_core::stage::{
    AgentStageDeltaView, PendingPlanApprovalView, StageTurnView, describe_tool_step,
    process_timeline_turns_slice,
};
use orbit_client_core::viewmodel::{FleetOverviewView, HostDetailView, TimelineItemView};
use orbit_protocol::{
    AgentMessageDeliveryStatus, AgentMessageKind, BackgroundTaskInfo, GuardianAssessment,
    GuardianDecision, GuardianRiskLevel, PermissionCheckpointRef, PlanApprovalPayload,
    QuestionItem, ToolCallDetail, ToolCallStatus,
};
use serde_json::{Value, to_value};
use std::error::Error;

const HOST: &str = "replay-host";
const FRAMES: u32 = 500;
const TURNS: u32 = 100;

fn prompt(seq: u64) -> TimelineItemView {
    TimelineItemView::UserPrompt {
        seq,
        message_id: format!("prompt-{seq}"),
        text: "Show the result.".into(),
        badges: vec![],
        blocks: parse_markdown_blocks("Show the result.", false),
        timestamp_ms: Some(seq as i64 * 25),
    }
}

fn reply(turn_index: u32, seq: u64, text: &str, streaming: bool) -> TimelineItemView {
    TimelineItemView::AssistantMessage {
        seq,
        item_id: format!("reply-{turn_index}"),
        text: text.into(),
        streaming,
        blocks: parse_markdown_blocks(text, streaming),
        timestamp_ms: Some(seq as i64 * 25),
    }
}

fn turn(
    index: u32,
    seq: u64,
    text: &str,
    streaming: bool,
) -> Result<StageTurnView, Box<dyn Error>> {
    let items = [
        prompt(u64::from(index) * 2 + 1),
        reply(index, seq, text, streaming),
    ];
    process_timeline_turns_slice(&items, 0, index, None)
        .turns
        .pop()
        .ok_or_else(|| "stage projection produced no turn".into())
}

fn delta(revision: u64, replace: u32, turns: Vec<StageTurnView>) -> AgentStageDeltaView {
    let total_turn_count = replace + turns.len() as u32;
    AgentStageDeltaView {
        revision,
        host_id: HOST.into(),
        agent: None,
        replace_from_turn_index: replace,
        window_start_turn_index: 0,
        changed_turns: turns,
        total_turn_count,
        pending_permissions: vec![],
        pending_questions: vec![],
        pending_plan_approval: None,
        session_turn_diffs: vec![],
        insights: Default::default(),
        latest_seq: revision,
        reset: replace == 0,
        aux_revision: 1,
        aux_unchanged: replace > 0,
        has_older_turns: false,
        has_older_history: false,
        oldest_loaded_seq: 1,
    }
}

fn timeline_examples() -> Vec<TimelineItemView> {
    let detail = ToolCallDetail::FileRead {
        path: "README.md".into(),
        start_line: Some(1),
        end_line: Some(10),
    };
    let mut tool = TimelineItemView::ToolCall {
        seq: 4,
        call_id: "read".into(),
        status: ToolCallStatus::Completed,
        summary_title: "Read file".into(),
        detail: detail.clone(),
        step_descriptor: None,
        background_task: Some(BackgroundTaskInfo {
            task_id: Some("job".into()),
            label: None,
            log_path: None,
        }),
        diff_files: vec![],
        timestamp_ms: Some(100),
    };
    let descriptor = describe_tool_step(&tool, None);
    if let TimelineItemView::ToolCall {
        step_descriptor, ..
    } = &mut tool
    {
        *step_descriptor = descriptor;
    }
    vec![
        prompt(1),
        TimelineItemView::AssistantThinking {
            seq: 2,
            item_id: "thought".into(),
            text: "Reading".into(),
            timestamp_ms: Some(50),
        },
        reply(0, 3, "Finished.", false),
        tool,
        TimelineItemView::PermissionRequest {
            seq: 5,
            request_id: "approval".into(),
            title: "Review change".into(),
            detail: detail.clone(),
            allowed_decisions: vec!["allow_once".into()],
            resolved_decision: None,
            guardian_assessment: Some(GuardianAssessment {
                risk_level: GuardianRiskLevel::Low,
                decision: GuardianDecision::RequireHuman,
                confidence: 1.,
                classifier_tier: "deterministic_tier1".into(),
                reason: "Requested review".into(),
                signals: vec![],
                latency_ms: 1,
            }),
            plan_approval: Some(PlanApprovalPayload {
                plan_markdown: "Review the file.".into(),
                plan_title: None,
                plan_file_path: None,
                summary: None,
                plan_file_backed: false,
                allowed_prompts: vec![],
                source: None,
            }),
            timestamp_ms: Some(125),
        },
        TimelineItemView::QuestionRequest {
            seq: 6,
            request_id: "question".into(),
            questions: vec![QuestionItem {
                id: "q1".into(),
                header: None,
                question: "Continue?".into(),
                options: vec!["Yes".into()],
                is_multi_select: false,
            }],
            resolved_answers: None,
            timestamp_ms: Some(150),
        },
        TimelineItemView::Notice {
            seq: 7,
            level: "info".into(),
            message: "Ready".into(),
            timestamp_ms: Some(175),
        },
        TimelineItemView::AgentMessage {
            seq: 8,
            message_id: "agent-message".into(),
            kind: AgentMessageKind::DirectMessage,
            source_agent_id: Some("peer".into()),
            source_host_id: Some(HOST.into()),
            source_title: Some("Peer".into()),
            event: Some("review".into()),
            text: "Ready".into(),
            permission_context: Some(PermissionCheckpointRef {
                agent_id: "peer".into(),
                host_id: Some(HOST.into()),
                request_id: "approval".into(),
                title: "Read file".into(),
                detail,
                allowed_decisions: vec!["allow_once".into()],
            }),
            delivery_status: Some(AgentMessageDeliveryStatus::Settled),
            delivery_turn_id: Some("turn".into()),
            blocking_turn_id: Some("prior-turn".into()),
            queued_at_ms: Some(150),
            dispatched_at_ms: Some(175),
            settled_at_ms: Some(200),
            blocks: vec![],
            timestamp_ms: Some(200),
        },
    ]
}

pub(super) fn contracts() -> Result<Vec<(String, Value)>, Box<dyn Error>> {
    let stage_turn = turn(0, 2, "Finished.", false)?;
    let mut frame = delta(2, 0, vec![stage_turn.clone()]);
    frame.pending_plan_approval = Some(PendingPlanApprovalView {
        seq: 2,
        request_id: "approval".into(),
        title: "Review".into(),
        plan_markdown: "Read file".into(),
        plan_title: None,
        plan_file_path: None,
        summary: None,
        allowed_prompts: vec![],
        source: None,
        blocks: vec![],
        timestamp_ms: Some(50),
    });
    let mut result = vec![
        ("AgentStageDeltaView".into(), to_value(frame)?),
        ("StageTurnView".into(), to_value(stage_turn)?),
        (
            "FleetOverviewView".into(),
            to_value(FleetOverviewView {
                revision: 1,
                total_pending_approvals: 0,
                hosts: vec![],
                projects: vec![],
                project_order: vec![],
            })?,
        ),
        (
            "HostDetailView".into(),
            to_value(HostDetailView::default())?,
        ),
        (
            "FleetSidebarView".into(),
            to_value(FleetSidebarView {
                revision: 1,
                projects: vec![],
                launchpad_projects: vec![],
                needs_attention: vec![],
                pinned_conversations: vec![],
            })?,
        ),
        (
            "PersonalAgentBriefingView".into(),
            to_value(PersonalAgentBriefingView::default())?,
        ),
        (
            "TerminalScreenView".into(),
            to_value(TerminalScreenView {
                cols: 80,
                rows: 24,
                cursor_row: 0,
                cursor_col: 0,
                rows_data: vec![],
            })?,
        ),
    ];
    for item in timeline_examples() {
        let value = to_value(item)?;
        let tag = value["item_type"]
            .as_str()
            .ok_or("missing timeline discriminator")?;
        result.push((format!("TimelineItemView.{tag}"), value));
    }
    Ok(result)
}

pub(super) fn recording() -> Result<String, Box<dyn Error>> {
    // The first frame seeds the history. Each further delta replaces only turn 99.
    let mut history = (0..TURNS - 1)
        .map(|i| turn(i, u64::from(i) * 2 + 2, "Done.", false))
        .collect::<Result<Vec<_>, _>>()?;
    const MARKDOWN: &str = concat!(
        "The stream contains one measurement per sensor. Each completed reading contributes ",
        "to a running total. The accumulator below starts at zero and uses the same units ",
        "as the original readings.\n\n",
        "```rust\nlet total: u32 = readings.iter().sum();\n```\n\n",
        "The first two readings give a small example that can be checked by hand.\n\n",
        "| Sensor | Reading |\n|---|---|\n| A | 3 |\n| B | 4 |\n\n",
        "For independent axes, the distance is $r=\\sqrt{x^2+y^2}$. Substituting the readings gives\n\n",
        "$$\nr^2 = 3^2 + 4^2 = 25\n$$\n\n",
        "The diagram shows how both inputs reach the distance calculation.\n\n",
        "```mermaid\ngraph TD\nA[Sensor A] --> C[Distance]\nB[Sensor B] --> C\n```\n\n",
        "The final distance is five units.",
    );
    let mut output = String::from("[\n");
    for i in 0..FRAMES {
        let revision = u64::from(TURNS * 2 + i);
        // ASCII recording: each frame reveals a longer prefix of the same reply.
        let end = (i as usize + 1) * MARKDOWN.len() / FRAMES as usize;
        let last = turn(TURNS - 1, revision, &MARKDOWN[..end], i + 1 != FRAMES)?;
        let frame = if i == 0 {
            history.push(last);
            delta(revision, 0, std::mem::take(&mut history))
        } else {
            delta(revision, TURNS - 1, vec![last])
        };
        if i > 0 {
            output.push_str(",\n");
        }
        output.push_str(&serde_json::to_string(&frame)?);
    }
    output.push_str("\n]\n");
    Ok(output)
}
