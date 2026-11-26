import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { TimelineItemView, TurnLiveSegmentView } from "./orbit-ipc.ts";
import {
  coalescePeerFinishes,
  computeWorkChunkDurationMs,
  extractFallbackDispatchCall,
  extractStateChangeMilestoneCall,
  isDelegationMilestoneToolCall,
  isDirectAgentPromptBubble,
  isTimelineAgentMessageHidden,
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

  it("keeps outgoing send_message inside the folded work group without fragmenting steps", () => {
    const items: TimelineItemView[] = [];
    for (let i = 1; i <= 26; i += 1) {
      items.push(makeReadTool(i, `/repo/file_${i}.rs`));
    }
    for (let i = 27; i <= 45; i += 1) {
      items.push(makeShellTool(i, `git log -n ${i}`));
    }
    items.push(makeSendMessageTool(46, "{}"));

    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, false);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0].kind, "work");
    assert.equal(res.chunks[0].items.length, 46);
  });

  it("keeps subagent dispatches folded inside work groups alongside reads and commands", () => {
    const items: TimelineItemView[] = [
      makeReadTool(1, "/repo/AGENTS.md"),
      makeReadTool(2, "/repo/Cargo.toml"),
      makeSubagentTool(3, "Codebase Researcher"),
      makeSubagentTool(4, "Security Reviewer"),
      makeShellTool(5, "cargo test --workspace"),
    ];

    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, false);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0].kind, "work");
    assert.equal(res.chunks[0].items.length, 5);
    assert.equal(summarizeWorkChunk(res.chunks[0].items).label, "Ran agents, read files, ran a command");
  });

  it("keeps pure-subagent step groups folded inside one work chunk", () => {
    const items: TimelineItemView[] = [
      makeSubagentTool(10, "Researcher A"),
      makeSubagentTool(11, "Researcher B"),
    ];
    const res = splitStepGroupChronologically(items);
    assert.equal(res.hasDelegations, false);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0].kind, "work");
    assert.equal(res.chunks[0].items.length, 2);
  });

  it("keeps failed generic invoke_subagent inside the folded work group while still extracting fallback metadata", () => {
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

    assert.equal(isDelegationMilestoneToolCall(failedDispatch), false);
    const res = splitStepGroupChronologically([failedDispatch]);
    assert.equal(res.chunks.length, 1);
    assert.equal(res.chunks[0].kind, "work");

    const info = extractFallbackDispatchCall(failedDispatch);
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

it("keeps space-normalized create_agent folded inside work while still extracting fallback metadata", () => {
  const item: TimelineItemView = {
    item_type: "tool_call", seq: 1, call_id: "new", status: "running", diff_files: [],
    summary_title: "Run mcp orbit create agent",
    detail: { kind: "generic", tool_name: "Run mcp orbit create agent",
      arguments_json: '{"toolSummary":"Build reviewer","initial_prompt":"Check build"}' },
  };
  assert.equal(splitStepGroupChronologically([item]).chunks[0]?.kind, "work");
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

it("suppresses hidden finish notifications and pure protocol noise while rejoining split step groups", () => {
  const report = message(1, "direct_message");
  const first = { ...work(2), has_following_content: true };
  const last = work(4);
  for (const hidden of [
    message(3, "finish_notification", { event: "finished", source_host_id: "local" }),
    message(3, "direct_message", {
      source_title: "Personal Router",
      text: '<orbit-fleet-delta rev="2">\n- worker1\n</orbit-fleet-delta>',
    }),
  ]) {
    const { segments } = coalescePeerFinishes(report, [first, segment(hidden), last], "local");
    assert.equal(segments.length, 1);
    const [steps] = segments;
    assert.equal(steps.kind, "step_group");
    assert.equal(steps.key, first.key);
    assert.deepEqual(steps.items.map(it => it.seq), [2, 4]);
    assert.equal(steps.summary.counts.command_count, 2);
    assert.equal(steps.has_following_content, false);
    assert.equal(first.items.length, 1);
  }
});

it("renders readable direct messages as sourced user bubbles while hiding routine finish notifications and protocol noise", () => {
  const report = message(1, "direct_message", { source_title: "Reviewer", text: "Please inspect stage.rs" });
  const permission = { child_agent_id: "ag_reporter", permission_request_id: "req", title: "Read file", allowed_decisions: [] };

  // Failed finish notifications and permission checkpoints stay visible via AgentMessageCard (not user bubbles)
  for (const fields of [
    { event: "errored" }, { event: "error" }, { event: "failed" },
    { text: "Agent ag_reporter (Reviewer) failed.\n\n<agent-response>Details</agent-response>" },
    { event: "finished", text: "Agent ag_reporter (Reviewer) finished.\n\nError: disconnected\n<agent-response>Details</agent-response>" },
    { event: "finished", text: "Report ready\nError: build lost its worker" },
    { permission_context: permission },
  ]) {
    const finishItem = message(2, "finish_notification", { source_title: "Reviewer", ...fields });
    assert.equal(isTimelineAgentMessageHidden(finishItem), false);
    assert.equal(isDirectAgentPromptBubble(finishItem), false);
    const finish = segment(finishItem);
    const result = coalescePeerFinishes(report, [finish], "local");
    assert.deepEqual(result.segments, [finish]);
  }

  // Real direct messages (including <orbit-system> wrapped) render via shared UserPromptBubble
  for (const directItem of [
    report,
    message(5, "direct_message", { source_title: "Personal Router", text: "Fix the folding logic" }),
    message(6, "direct_message", {
      source_title: "@backend",
      text: '<orbit-system version="1" kind="agent-message" source-agent-id="ag_be">\nSchema updated\n</orbit-system>',
    }),
  ]) {
    assert.equal(isDirectAgentPromptBubble(directItem), true);
    assert.equal(isTimelineAgentMessageHidden(directItem), false);
    const seg = segment(directItem);
    const result = coalescePeerFinishes(null, [work(2), seg, work(4)], "local");
    assert.equal(result.segments.length, 3);
  }

  // Routine/queued finish notifications and pure protocol noise remain suppressed
  for (const hiddenItem of [
    message(3, "finish_notification", { source_title: "Reviewer", event: "finished", source_agent_id: null }),
    message(3, "finish_notification", { source_title: "Reviewer", event: "finished", source_agent_id: "other" }),
    message(3, "finish_notification", { source_title: "Reviewer", event: "canceled", source_host_id: "remote" }),
    message(3, "finish_notification", { source_title: "Reviewer", event: "finished" }),
    message(3, "finish_notification", { source_title: "Reviewer", event: "finished", delivery_status: "queued" }),
    message(3, "direct_message", { source_title: "Router", text: '<orbit-fleet-state rev="1">\n- a\n</orbit-fleet-state>' }),
    message(3, "direct_message", { source_title: "Router", text: '<orbit-fleet-delta rev="2">\n- a\n</orbit-fleet-delta>' }),
    message(3, "direct_message", {
      source_title: "Router",
      text: '<orbit-system version="1" kind="agent-notification" event="finished">\nDone\n</orbit-system>',
    }),
  ]) {
    assert.equal(isDirectAgentPromptBubble(hiddenItem), false);
    assert.equal(isTimelineAgentMessageHidden(hiddenItem), true);
    const result = coalescePeerFinishes(null, [work(2), segment(hiddenItem), work(4)], "local");
    assert.equal(result.segments.length, 1);
    assert.equal(result.segments[0]?.kind, "step_group");
    if (result.segments[0]?.kind === "step_group") {
      assert.deepEqual(result.segments[0].items.map((it) => it.seq), [2, 4]);
    }
  }

  // Heartbeats and permission-context direct messages remain visible as system cards (not user bubbles)
  assert.equal(isTimelineAgentMessageHidden(message(4, "heartbeat")), false);
  assert.equal(isDirectAgentPromptBubble(message(4, "heartbeat")), false);
  assert.equal(isTimelineAgentMessageHidden(message(6, "direct_message", { permission_context: permission })), false);
  assert.equal(isDirectAgentPromptBubble(message(6, "direct_message", { permission_context: permission })), false);
  const heartbeatSeg = segment(message(4, "heartbeat"));
  const finish = segment(message(2, "finish_notification"));
  assert.deepEqual(coalescePeerFinishes(null, [finish, heartbeatSeg], "local").segments, [heartbeatSeg]);
});

it("rejoins adjacent step groups even when an intervening wakeup or plan notice was stripped earlier", () => {
  const first = { ...work(2), has_following_content: true };
  const second = work(5);
  const { segments } = coalescePeerFinishes(null, [first, second], "local");
  assert.equal(segments.length, 1);
  assert.equal(segments[0]?.kind, "step_group");
  if (segments[0]?.kind === "step_group") {
    assert.deepEqual(segments[0].items.map((it) => it.seq), [2, 5]);
    assert.equal(segments[0].summary.counts.command_count, 2);
  }
});

it("resolveCompletedTurnWorkFold wraps only pure pre-final tool work and never swallows important segments", async () => {
  const { resolveCompletedTurnWorkFold } = await import("./turn-delegation-split.ts");

  const assistant = (seq: number, text: string, streaming = false): TurnLiveSegmentView => ({
    kind: "assistant_message",
    key: `msg-${seq}`,
    item: {
      item_type: "assistant_message",
      seq,
      item_id: `m${seq}`,
      text,
      streaming,
      blocks: [],
    },
  });

  const thoughtOnlyGroup = (seq: number): TurnLiveSegmentView => {
    const items: TimelineItemView[] = [
      { item_type: "assistant_thinking", seq, item_id: `th-${seq}`, text: "Reasoning..." },
    ];
    return {
      kind: "step_group",
      key: `th-group-${seq}`,
      first_seq: seq,
      items,
      summary: summarizeWorkChunk(items, 5_000),
      thought_duration_ms: 5_000,
      has_following_content: true,
      has_running_step: false,
    };
  };

  // 1. Pure pre-final tool work + final reply => wraps cleanly with natural fallback label
  const pure = resolveCompletedTurnWorkFold({
    segments: [{ ...work(1), work_duration_ms: 4_000, has_following_content: true }, assistant(2, "Done.")],
    isTurnActive: false,
    turnHasUnrecoveredError: false,
    locale: "zh-CN",
  });
  assert.equal(pure.shouldWrap, true);
  assert.equal(pure.items.length, 1);
  assert.equal(pure.sumWorkDurationMs, 4_000);
  assert.ok(pure.fallbackLabel.length > 0);

  // 2. Active turn or streaming final reply => never wraps
  assert.equal(
    resolveCompletedTurnWorkFold({
      segments: [work(1), assistant(2, "Done.")],
      isTurnActive: true,
      turnHasUnrecoveredError: false,
    }).shouldWrap,
    false,
  );
  assert.equal(
    resolveCompletedTurnWorkFold({
      segments: [work(1), assistant(2, "Partial...", true)],
      isTurnActive: false,
      turnHasUnrecoveredError: false,
    }).shouldWrap,
    false,
  );

  // 3. Pure thinking-only pre-final work => stays as dedicated [Brain] Thought disclosure
  assert.equal(
    resolveCompletedTurnWorkFold({
      segments: [thoughtOnlyGroup(1), assistant(2, "Answer.")],
      isTurnActive: false,
      turnHasUnrecoveredError: false,
    }).shouldWrap,
    false,
  );

  // 4. Multi-message turn (Antigravity mid-turn analysis + final closing) => never swallows earlier reply
  assert.equal(
    resolveCompletedTurnWorkFold({
      segments: [work(1), assistant(2, "Detailed findings"), work(3), assistant(4, "Verified.")],
      isTurnActive: false,
      turnHasUnrecoveredError: false,
    }).shouldWrap,
    false,
  );
  assert.equal(
    resolveCompletedTurnWorkFold({
      segments: [assistant(1, "Part 1"), thoughtOnlyGroup(2), assistant(3, "Part 2")],
      isTurnActive: false,
      turnHasUnrecoveredError: false,
    }).shouldWrap,
    false,
  );

  // 5. Notice, compaction, wakeup, visible agent_message, or promoted timer milestone => never swallowed
  const noticeSeg: TurnLiveSegmentView = {
    kind: "notice",
    key: "notice-2",
    item: { item_type: "notice", seq: 2, level: "warning", message: "API retry" },
  };
  const compactionSeg: TurnLiveSegmentView = {
    kind: "compaction",
    key: "compact-2",
    label: "Context compacted",
  };
  const wakeupSeg: TurnLiveSegmentView = {
    kind: "wakeup",
    key: "wakeup-2",
    label: "Background task finished",
  };
  const failedAlertSeg = segment(message(2, "finish_notification", { event: "failed" }));
  const timerItems: TimelineItemView[] = [
    makeShellTool(1, "pwd"),
    {
      item_type: "tool_call",
      seq: 2,
      call_id: "sched-2",
      summary_title: "Schedule timer",
      status: "completed",
      detail: {
        kind: "generic",
        tool_name: "schedule",
        arguments_json: JSON.stringify({ DurationSeconds: 60, Prompt: "Check build" }),
      },
      diff_files: [],
    },
  ];
  const timerGroup: TurnLiveSegmentView = {
    kind: "step_group",
    key: "timer-group",
    first_seq: 1,
    items: timerItems,
    summary: summarizeWorkChunk(timerItems),
    has_following_content: true,
    has_running_step: false,
  };

  for (const segs of [
    [work(1), noticeSeg, assistant(3, "Final")],
    [work(1), compactionSeg, assistant(3, "Final")],
    [work(1), wakeupSeg, assistant(3, "Final")],
    [work(1), failedAlertSeg, assistant(3, "Final")],
    [timerGroup, assistant(3, "Timer armed.")],
  ]) {
    assert.equal(
      resolveCompletedTurnWorkFold({
        segments: segs,
        isTurnActive: false,
        turnHasUnrecoveredError: false,
      }).shouldWrap,
      false,
    );
  }
});
