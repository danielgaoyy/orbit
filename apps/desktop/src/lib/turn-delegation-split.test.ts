import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { TimelineItemView, TurnLiveSegmentView } from "./orbit-ipc.ts";
import {
  coalescePeerFinishes,
  computeWorkChunkDurationMs,
  extractFallbackDispatchCall,
  extractStateChangeMilestoneCall,
  isDelegationMilestoneToolCall,
  splitStepGroupChronologically,
  summarizeWorkChunk,
} from "./turn-delegation-split.ts";

function makeReadTool(seq: number, path: string, timestampMs?: number): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `read-${seq}`,
    status: "completed",
    summary_title: `Read ${path}`,
    detail: { kind: "file_read", path, start_line: null, end_line: null },
    diff_files: [],
    timestamp_ms: timestampMs ?? null,
  };
}

function makeShellTool(seq: number, command: string, status: "running" | "completed" | "failed" = "completed"): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `shell-${seq}`,
    status,
    summary_title: command,
    detail: { kind: "shell", command, cwd: null, exit_code: status === "failed" ? 1 : 0, output_preview: null },
    diff_files: [],
  };
}

function makeSendMessageTool(
  seq: number,
  argsJson = "{}",
  status: "running" | "completed" | "failed" | "denied" = "completed",
): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `msg-${seq}`,
    status,
    summary_title: "Send message",
    detail: {
      kind: "generic",
      tool_name: "send_message",
      arguments_json: argsJson,
    },
    diff_files: [],
  };
}

function genericTool(seq: number, toolName: string, args: object | string) {
  return {
    item_type: "tool_call", seq, call_id: `call-${seq}`, status: "completed", diff_files: [],
    summary_title: toolName,
    detail: {
      kind: "generic", tool_name: toolName,
      arguments_json: typeof args === "string" ? args : JSON.stringify(args),
    },
  } satisfies TimelineItemView;
}

function makeSubagentTool(
  seq: number,
  role: string,
  promptPreview = "Audit crate",
  status: "running" | "completed" | "failed" | "denied" = "completed",
): TimelineItemView {
  return {
    item_type: "tool_call",
    seq,
    call_id: `sa-${seq}`,
    status,
    summary_title: `Ran ${role}`,
    detail: {
      kind: "subagent",
      role,
      prompt_preview: promptPreview,
      child_agent_id: `child-${seq}`,
    },
    diff_files: [],
  };
}

describe("turn-delegation-split", () => {
  it("keeps step groups without delegation calls untouched on the fast path", () => {
    const items = [makeReadTool(1, "/repo/a.ts"), makeShellTool(2, "cargo test")];
    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, false);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0]?.kind, "work");
    assert.equal(res.chunks[0]?.items, items);
  });

  it("extracts trailing send_message out of reads+commands and removes 'used a tool' from summary", () => {
    const items: TimelineItemView[] = [];
    for (let i = 1; i <= 26; i += 1) {
      items.push(makeReadTool(i, `/repo/file_${i}.rs`));
    }
    for (let i = 27; i <= 45; i += 1) {
      items.push(makeShellTool(i, `git log -n ${i}`));
    }
    items.push(makeSendMessageTool(46, "{}"));

    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, true);
    assert.equal(res.chunks.length, 2);

    const [workChunk, delegationChunk] = res.chunks;
    assert.equal(workChunk.kind, "work");
    assert.equal(workChunk.items.length, 45);
    assert.equal(delegationChunk.kind, "delegation");
    assert.equal(delegationChunk.items.length, 1);
    assert.equal(delegationChunk.items[0].call_id, "msg-46");

    const summary = summarizeWorkChunk(workChunk.items);
    assert.equal(summary.label, "Read 26 files, ran 19 commands");
    assert.equal(summary.counts.other_tool_count, 0);
    assert.equal(summary.counts.read_file_count, 26);
    assert.equal(summary.counts.command_count, 19);
  });

  it("splits interleaved work -> parallel subagents -> follow-up work in strict chronological order", () => {
    const items: TimelineItemView[] = [
      makeReadTool(1, "/repo/AGENTS.md"),
      makeReadTool(2, "/repo/Cargo.toml"),
      makeSubagentTool(3, "Codebase Researcher"),
      makeSubagentTool(4, "Security Reviewer"),
      makeShellTool(5, "cargo test --workspace"),
    ];

    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, true);
    assert.equal(res.chunks.length, 3);
    assert.equal(res.chunks[0].kind, "work");
    assert.equal(res.chunks[0].items.length, 2);
    assert.equal(res.chunks[1].kind, "delegation");
    assert.equal(res.chunks[1].items.length, 2);
    assert.equal(res.chunks[2].kind, "work");
    assert.equal(res.chunks[2].items.length, 1);

    assert.equal(summarizeWorkChunk(res.chunks[0].items).label, "Read 2 files");
    assert.equal(summarizeWorkChunk(res.chunks[2].items).label, "Ran a command");
  });

  it("renders pure-delegation step groups with zero empty work chunks", () => {
    const items: TimelineItemView[] = [
      makeSubagentTool(10, "Researcher A"),
      makeSubagentTool(11, "Researcher B"),
    ];
    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, true);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0].kind, "delegation");
    assert.equal(res.chunks[0].items.length, 2);
  });

  it("preserves failed generic invoke_subagent calls and extracts fallback dispatch metadata", () => {
    const failedDispatch: TimelineItemView = {
      item_type: "tool_call",
      seq: 20,
      call_id: "inv-20",
      status: "failed",
      summary_title: "Invoke subagent",
      detail: {
        kind: "generic",
        tool_name: "invoke_subagent",
        arguments_json: JSON.stringify({
          Role: "Rust Reviewer",
          Prompt: "Review crates/orbit-cli",
        }),
      },
      diff_files: [],
    };

    assert.equal(isDelegationMilestoneToolCall(failedDispatch), true);
    const res = splitStepGroupChronologically([failedDispatch]);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0].kind, "delegation");

    const info = extractFallbackDispatchCall(res.chunks[0].items[0]);
    assert.ok(info);
    assert.equal(info.label, "Rust Reviewer");
    assert.equal(info.preview, "Review crates/orbit-cli");
    assert.equal(info.status, "failed");
  });

  it("ignores legacy empty Subagent placeholder items", () => {
    const legacy: TimelineItemView = {
      item_type: "tool_call",
      seq: 30,
      call_id: "legacy-30",
      status: "completed",
      summary_title: "Subagent",
      detail: {
        kind: "subagent",
        role: "Subagent",
        prompt_preview: "",
        child_agent_id: null,
      },
      diff_files: [],
    };
    assert.equal(isDelegationMilestoneToolCall(legacy), false);
    const res = splitStepGroupChronologically([legacy]);
    assert.equal(res.hasDelegations, false);
  });

  it("computes work chunk duration from item timestamps when spanning >= 1s", () => {
    const items = [
      makeReadTool(1, "/repo/a.ts", 10_000),
      makeReadTool(2, "/repo/b.ts", 14_500),
    ];
    assert.equal(computeWorkChunkDurationMs(items), 4500);
    assert.equal(computeWorkChunkDurationMs([makeReadTool(1, "/repo/a.ts", 10_000)]), null);
  });

  it("promotes session-level timer and worktree handoff calls into external milestones", () => {
    const timerCall: TimelineItemView = {
      item_type: "tool_call",
      seq: 50,
      call_id: "timer-50",
      status: "completed",
      summary_title: "Timer",
      detail: {
        kind: "generic",
        tool_name: "mcp_orbit_orbit_timer",
        arguments_json: JSON.stringify({ action: "create", duration_seconds: 120, prompt: "Check build" }),
      },
      diff_files: [],
    };
    const handoffCall: TimelineItemView = {
      item_type: "tool_call",
      seq: 51,
      call_id: "handoff-51",
      status: "completed",
      summary_title: "Handoff",
      detail: {
        kind: "generic",
        tool_name: "mcp_orbit_handoff_worktree",
        arguments_json: JSON.stringify({ direction: "worktree_to_local", dry_run: false }),
      },
      diff_files: [],
    };

    assert.equal(isDelegationMilestoneToolCall(timerCall), true);
    assert.equal(isDelegationMilestoneToolCall(handoffCall), true);

    const timerInfo = extractStateChangeMilestoneCall(timerCall);
    assert.equal(timerInfo?.kind, "timer");
    assert.equal(timerInfo?.label, "Timer (2m)");
    assert.equal(timerInfo?.preview, "Check build");

    const handoffInfo = extractStateChangeMilestoneCall(handoffCall);
    assert.equal(handoffInfo?.kind, "handoff");
    assert.equal(handoffInfo?.label, "Handoff (worktree → local)");
    assert.match(handoffInfo?.preview ?? "", /worktree_to_local/);
  });
});

it("promotes a space-normalized dispatch before its managed child arrives", () => {
  const item: TimelineItemView = {
    item_type: "tool_call", seq: 1, call_id: "new", status: "running", diff_files: [],
    summary_title: "Run mcp orbit create agent",
    detail: { kind: "generic", tool_name: "Run mcp orbit create agent",
      arguments_json: '{"toolSummary":"Build reviewer","initial_prompt":"Check build"}' },
  };
  assert.equal(splitStepGroupChronologically([item]).chunks[0]?.kind, "delegation");
  assert.deepEqual(extractFallbackDispatchCall(item), {
    callId: "new", label: "Build reviewer", preview: "Check build", status: "running",
  });
  assert.equal(item.detail.kind, "generic");
  item.detail.arguments_json = "";
  assert.equal(extractFallbackDispatchCall(item)?.label, "Subagent");
});

it("localizes live and settled summaries while preserving the file name", () => {
  const read = makeReadTool(1, "/repo/Build.md");
  for (const [locale, settled, live] of [
    ["en", "Read Build.md", "Reading Build.md…"],
    ["zh-CN", "已读取 Build.md", "正在读取 Build.md…"],
    ["ja", "Build.md を読み取り", "Build.md を読み取り中…"],
  ] as const) {
    const summary = summarizeWorkChunk([read], null, locale);
    assert.equal(summary.label, settled);
    assert.equal(summary.live_label, live);
    assert.equal(summary.counts.read_file_count, 1);
  }
});

it("keeps timer maintenance and plan bookkeeping in one folded group between ordinary steps", () => {
  const tools = [
    genericTool(2, "mcp__orbit__orbit_timer", { action: "cancel", duration_seconds: 30 }),
    genericTool(3, "Run mcp orbit orbit timer", { action: "list" }),
    genericTool(4, "schedule", { Action: "CANCEL", CronExpression: "0 9 * * *" }),
    genericTool(5, "schedule", { action: "list" }),
    genericTool(6, "orbit_timer", '{"action":'),
    ...["update_steps", "update_plan", "TodoWrite", "TaskCreate", "TaskUpdate", "TaskList"]
      .map((name, index) => genericTool(7 + index, name, { action: "create" })),
  ];
  const items = [makeShellTool(1, "pwd"), ...tools, makeReadTool(13, "/a")];
  const split = splitStepGroupChronologically(items);
  assert.equal(split.hasDelegations, false);
  assert.equal(split.chunks[0].items, items);
  for (const tool of tools) assert.equal(extractStateChangeMilestoneCall(tool), null);
});

it("keeps scheduled cron, duration and conditional wakeups in their chronological positions", () => {
  const cron = genericTool(2, "schedule", { CronExpression: "0 9 * * *", Prompt: "Review" });
  const timer = genericTool(3, "Run mcp orbit orbit timer", { action: "create", duration_seconds: 300 });
  const condition = genericTool(5, "schedule", { TimerCondition: "all_peers" });
  const items = [makeReadTool(1, "a"), cron, timer, genericTool(4, "orbit_timer", { action: "list" }), condition];
  const split = splitStepGroupChronologically(items);
  assert.deepEqual(split.chunks.map(chunk => [chunk.kind, chunk.items.map(item => item.seq)]), [
    ["work", [1]], ["delegation", [2, 3]], ["work", [4]], ["delegation", [5]],
  ]);
  assert.equal(extractStateChangeMilestoneCall(cron)?.preview, "Review");
  assert.equal(extractStateChangeMilestoneCall(timer)?.label, "Timer (5m)");
});

type AgentMessage = Extract<TimelineItemView, { item_type: "agent_message" }>;

function message(seq: number, kind: AgentMessage["kind"], fields: Partial<AgentMessage> = {}): AgentMessage {
  return {
    item_type: "agent_message", seq, message_id: `m-${seq}`, kind, source_agent_id: "ag_reporter",
    text: "Report ready", blocks: [], ...fields,
  };
}

function segment(item: AgentMessage): TurnLiveSegmentView {
  return { kind: "agent_message", key: item.message_id, item };
}

function work(seq: number): Extract<TurnLiveSegmentView, { kind: "step_group" }> {
  const items = [makeShellTool(seq, "pwd")];
  return {
    kind: "step_group", key: `steps-${seq}`, first_seq: seq, items, summary: summarizeWorkChunk(items),
    has_following_content: false, has_running_step: false,
  };
}

it("attaches a finish to the turn-opening report and rejoins only the work split by that finish", () => {
  const report = message(1, "direct_message");
  const first = { ...work(2), has_following_content: true };
  const last = work(4);
  const finish = message(3, "finish_notification", { event: "finished", source_host_id: "local" });
  const { segments, finishStatuses } = coalescePeerFinishes(report, [first, segment(finish), last], "local");
  assert.equal(finishStatuses.get(report), "completed");
  assert.equal(segments.length, 1);
  const [steps] = segments;
  assert.equal(steps.kind, "step_group");
  assert.equal(steps.key, first.key);
  assert.deepEqual(steps.items.map(it => it.seq), [2, 4]);
  assert.equal(steps.summary.counts.command_count, 2);
  assert.equal(steps.has_following_content, false);
  assert.equal(first.items.length, 1);
});

it("attaches canceled and completed finishes to each peer's latest preceding direct report", () => {
  const old = message(1, "direct_message");
  const latest = message(2, "direct_message");
  const remote = message(3, "direct_message", { source_host_id: "remote" });
  const other = message(4, "direct_message", { source_agent_id: "other" });
  const finishes = [
    message(5, "finish_notification", { event: "cancelled" }),
    message(6, "finish_notification", { event: "closed", source_host_id: "remote" }),
    message(7, "finish_notification", { event: "finished", source_agent_id: "other" }),
  ];
  const { segments, finishStatuses } = coalescePeerFinishes(old, [latest, remote, other, ...finishes].map(segment), "local");
  assert.deepEqual(segments.map(seg => seg.key), [latest.message_id, remote.message_id, other.message_id]);
  assert.deepEqual([...finishStatuses], [[latest, "canceled"], [remote, "canceled"], [other, "completed"]]);
});

it("retains unmatched, failed, and permission notifications even when a report has the same display name", () => {
  const report = message(1, "direct_message", { source_title: "Reviewer" });
  const permission = { child_agent_id: "ag_reporter", permission_request_id: "req", title: "Read file", allowed_decisions: [] };
  for (const fields of [
    { event: "errored" }, { event: "error" }, { event: "failed" },
    { text: "Agent ag_reporter (Reviewer) failed.\n\n<agent-response>Details</agent-response>" },
    { event: "finished", text: "Agent ag_reporter (Reviewer) finished.\n\nError: disconnected\n<agent-response>Details</agent-response>" },
    { event: "finished", text: "Report ready\nError: build lost its worker" },
    { permission_context: permission }, { event: "permission_required" },
    { source_agent_id: null }, { source_agent_id: "other" }, { source_host_id: "remote" },
  ]) {
    const finish = segment(message(2, "finish_notification", { source_title: "Reviewer", ...fields }));
    const result = coalescePeerFinishes(report, [finish], "local");
    assert.deepEqual(result.segments, [finish]);
    assert.equal(result.finishStatuses.size, 0);
  }
  const finish = segment(message(2, "finish_notification"));
  for (const opening of [null, message(1, "heartbeat"), { ...report, permission_context: permission }]) {
    assert.deepEqual(coalescePeerFinishes(opening, [finish], "local").segments, [finish]);
  }
  const laterReport = segment(message(3, "direct_message"));
  assert.deepEqual(coalescePeerFinishes(null, [finish, laterReport], "local").segments, [finish, laterReport]);
});
