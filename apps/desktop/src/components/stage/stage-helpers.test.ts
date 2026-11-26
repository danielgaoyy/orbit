import assert from "node:assert/strict";
import { test } from "node:test";
import { Virtualizer } from "@tanstack/react-virtual";
import type {
  AgentCardView, HostCardView, HostDetailView, StageTurnView, TimelineItemView, ToolCallDetail,
} from "../../lib/orbit-ipc.ts";
import { buildSubagentNameMatcher } from "../../lib/subagents.ts";
import { computePrependRestoredScrollTop } from "../../lib/stage-pagination.ts";
import type { OrbitUiState } from "../../state/store-types.ts";
import {
  DEFAULT_TURN_SIZE_METRICS,
  EMPTY_WORK_COUNTS,
  buildTurnHeightCachePrefix,
  createHostAgentSelector,
  createRelativeClock,
  createStableHostArtifactsSelector,
  createStageSessionSelector,
  createTurnProseMatcherLookup,
  estimateTurnSizeFallback,
  findTurnIndexForSeq,
  formatFullTimestampTooltip,
  formatRelativeTimeLabel,
  getCachedHistoricalThinkingSignature,
  getTurnContentStart,
  getTurnFooter,
  getTurnItemHeight,
  getTurnListRows,
  getTurnTopGapPx,
  isLiveCompactionSegment,
  mayUserPromptOverflow,
  resolveTurnForkSeq,
  turnHasSubagentReferences,
  updateStageTimingHistory,
  type StageTimingHistory,
  type TurnTimingRecord,
} from "./stage-helpers.ts";

function turn(anchor: number, live_segments: StageTurnView["live_segments"] = []): StageTurnView {
  return {
    key: `turn-${anchor}`, turn_index: 0, anchor_seq: anchor, user_prompt: null,
    is_compaction_command: false, folded_steps: [], work_counts: EMPTY_WORK_COUNTS,
    work_summary_label: "", work_summary_live_label: "", work_summary_includes_failure: false,
    thought_duration_ms: null, turn_duration_ms: null, terminal_reply: null, notices: [],
    compactions: [], live_segments, turn_diffs: [], has_error: false, has_unrecovered_error: false,
    has_active_item: false, started_at_ms: null, completed_at_ms: null,
  };
}

function group(...items: TimelineItemView[]): StageTurnView["live_segments"][number] {
  return {
    kind: "step_group", key: "steps", first_seq: items[0].seq, items,
    has_following_content: false, has_running_step: false,
    summary: { counts: EMPTY_WORK_COUNTS, label: "", live_label: "", live_detail: null, label_includes_failure: false },
  };
}

function tool(detail: ToolCallDetail): TimelineItemView {
  return { item_type: "tool_call", seq: 2, call_id: "call", status: "completed", summary_title: "", detail, diff_files: [] };
}

function thought(seq: number): TimelineItemView {
  return { item_type: "assistant_thinking", seq, item_id: `item-${seq}`, text: "thinking" };
}

const message: TimelineItemView = {
  item_type: "agent_message", seq: 14, kind: "finish_notification", event: "failed", message_id: "message", text: "Error: failed", blocks: [],
};
const inlineMessage = { kind: "agent_message", key: "message", item: message } as const;
const hiddenDirectMessage: TimelineItemView = {
  item_type: "agent_message", seq: 15, kind: "direct_message", message_id: "dm", text: "Hello", blocks: [],
};
const inlineHiddenDirect = { kind: "agent_message", key: "dm", item: hiddenDirectMessage } as const;

const prompt: TimelineItemView = {
  item_type: "user_prompt", seq: 10, message_id: "prompt", text: "Continue", blocks: [],
};

test("turn spacing counts the historical footer slot once and preserves a distinct user break", () => {
  const peer = turn(1);
  const user = { ...turn(2), user_prompt: prompt };
  assert.equal(getTurnTopGapPx(user, undefined), 0);
  assert.equal(getTurnTopGapPx(peer, undefined), 0);
  assert.equal(getTurnTopGapPx(peer, true), 8);
  assert.equal(getTurnTopGapPx(peer, false), 12);
  assert.equal(getTurnTopGapPx(user, true), 14);
  assert.equal(getTurnTopGapPx(user, false), 32);
});

test("only rendered footer content contributes to the following turn's spacing", () => {
  const historical = { canFork: true };
  const report = { ...turn(1, [inlineMessage]), agent_message: message };
  assert.equal(getTurnFooter(report, historical)?.forkUpToSeq, 14);
  assert.equal(getTurnFooter(report, { ...historical, isActive: true }), null);
  for (const footerless of [
    { ...turn(1), agent_message: message, completed_at_ms: 123 },
    { ...turn(1, [inlineHiddenDirect]), agent_message: hiddenDirectMessage, completed_at_ms: 456 },
    { ...report, is_compaction_command: true },
    turn(1, [{ kind: "wakeup", key: "wait", label: "Waiting for a peer" }]),
  ]) {
    assert.equal(getTurnFooter(footerless, historical), null);
    assert.equal(getTurnListRows([footerless, report], "host::agent", new Map(), false)[1].topGapPx, 12);
  }

  const unsequenced = turn(0, [{ kind: "wakeup", key: "resume", label: "Peer resumed" }]);
  assert.equal(getTurnFooter(unsequenced, historical), null);
  assert.equal(getTurnFooter(unsequenced, { ...historical, isLastTurn: true })?.showFork, true);
  const timing = { startedAtMs: 1, completedAtMs: 2, durationMs: 1, workDurationMs: null };
  assert.equal(getTurnFooter(unsequenced, { canFork: false, timing })?.completedAtMs, 2);
  const reply: TimelineItemView = {
    item_type: "assistant_message", seq: 3, item_id: "reply", text: "  Finished  ", blocks: [], streaming: false,
  };
  const replyTurn = { ...report, last_assistant_reply: reply };
  assert.equal(getTurnFooter(replyTurn, { canFork: false })?.replyText, "Finished");
});

test("isLiveCompactionSegment distinguishes live compacting from completed dividers", () => {
  assert.equal(isLiveCompactionSegment({ key: "compact-live-12", label: "Compacting context…" }), true);
  assert.equal(isLiveCompactionSegment({ key: "compact-12", label: "Compacting context..." }), true);
  assert.equal(isLiveCompactionSegment({ key: "compact-12", label: "Compaction requested" }), true);
  assert.equal(isLiveCompactionSegment({ key: "compact-13", label: "Context compacted" }), false);
  assert.equal(
    isLiveCompactionSegment({
      key: "compact-14",
      label: "Context compacted automatically (from 142k tokens)",
    }),
    false,
  );
});

test("prepending rebases the first cached row's padding and preserves its content anchor", () => {
  const older = { ...turn(1, [inlineMessage]), agent_message: message };
  const marker = { ...turn(2), is_compaction_command: true };
  const first = { ...turn(3, [inlineMessage]), user_prompt: prompt };
  const peer = turn(4, [inlineMessage]);
  const nextPrompt = { ...turn(5), user_prompt: prompt };
  const oldTurns = [first, peer, nextPrompt];
  const turns = [older, marker, ...oldTurns];
  const timings = new Map();
  const before = getTurnListRows(oldTurns, "host::agent", timings, false);
  const after = getTurnListRows(turns, "host::agent", timings, false, before);
  assert.deepEqual(before.map(row => row.topGapPx), [0, 8, 14]);
  assert.deepEqual(after.map(row => row.topGapPx), [0, 8, 32, 8, 14]);
  assert.equal(getTurnListRows([...turns], "host::agent", timings, false, after), after);

  // One cold estimate and four known content heights. TanStack measures the
  // whole padded wrapper; restoring the session must not pad a cached height twice.
  const contentHeights = new Map([[marker.key, 30], [first.key, 202], [peer.key, 90], [nextPrompt.key, 68]]);
  const layout = (list: StageTurnView[], rows: typeof after) => new Virtualizer({
    count: list.length, getScrollElement: () => null, gap: 0,
    getItemKey: i => rows[i].key,
    estimateSize: i => getTurnItemHeight(list[i], rows[i], contentHeights.get(rows[i].key)),
    scrollToFn: () => {}, observeElementOffset: () => {}, observeElementRect: () => {},
  });
  const oldList = layout(oldTurns, before);
  const newList = layout(turns, after);
  const oldTotal = oldList.getTotalSize();
  const newTotal = newList.getTotalSize();
  const addedHeight = estimateTurnSizeFallback(older) + 26 + 8 + 30 + 32;
  assert.equal(newTotal - oldTotal, addedHeight);
  assert.equal(newList.measurementsCache[2].size, 202 + 26 + 32);
  const oldAnchor = getTurnContentStart(before[0], oldList.measurementsCache[0].start);
  const newAnchor = getTurnContentStart(after[2], newList.measurementsCache[2].start);
  assert.equal(newAnchor - oldAnchor, addedHeight);
  assert.equal(computePrependRestoredScrollTop(37, oldTotal, newTotal), newAnchor + 37);
  assert.equal(newList.getVirtualItemForOffset(newAnchor + 37)?.key, first.key);
  assert.equal(newList.measurementsCache[4].end, newTotal);

  newList.resizeItem(4, newList.measurementsCache[4].size + 70);
  assert.equal(newList.getTotalSize(), newTotal + 70);
  assert.equal(getTurnContentStart(after[2], newList.measurementsCache[2].start), newAnchor);
});

test("a cached reply can settle, become historical, or lose its predecessor without retaining old spacing", () => {
  const reply = turn(1, [inlineMessage]);
  const timings = new Map();
  const content = 102.375;
  const active = getTurnListRows([reply], "host::agent", timings, true);
  const settled = getTurnListRows([reply], "host::agent", timings, false, active);
  const appended = getTurnListRows([reply, turn(2)], "host::agent", timings, true, settled);
  const rewound = getTurnListRows([reply], "host::agent", timings, false, appended);
  assert.deepEqual([active, settled, appended, rewound].map(rows => getTurnItemHeight(reply, rows[0], content)), [
    content, content + 30, content + 26, content + 30,
  ]);
});

test("an inline peer message is an exact jump target and the inclusive fork boundary", () => {
  const turns = [turn(1), turn(10, [inlineMessage]), turn(20)];
  assert.deepEqual(findTurnIndexForSeq(turns, 14), { index: 1, exact: true });
  assert.deepEqual(findTurnIndexForSeq(turns, 15), { index: 1, exact: false });
  assert.equal(resolveTurnForkSeq(turns[1]), 14);
  assert.equal(resolveTurnForkSeq({ ...turns[1], terminal_reply: { ...message, seq: 17 } }), 17);
});

test("only peer messages and subagent tools need the session dispatch map", () => {
  const generic = (name: string) => tool({ kind: "generic", tool_name: name, arguments_json: "{}" });
  const normal = turn(1, [group(thought(2), generic("orbit_timer"), generic("read_file"))]);
  assert.equal(turnHasSubagentReferences(normal), false);
  assert.equal(turnHasSubagentReferences({ ...normal, agent_message: message }), true);
  assert.equal(turnHasSubagentReferences(turn(1, [inlineMessage])), true);
  for (const item of [
    generic("mcp__orbit__spawn_workspace_peer"), generic("message_peer"),
    tool({ kind: "subagent", role: "Reviewer", child_agent_id: null, prompt_preview: "" }),
  ]) {
    assert.equal(turnHasSubagentReferences(turn(1, [group(item)])), true);
  }
});

test("historical signatures reuse reads until their own version changes and active turns read live timing", () => {
  const history = turn(1, [group(thought(2))]);
  let reads = 0;
  let duration = 17;
  const timings = {
    get "agent::think::item-2"(): TurnTimingRecord {
      reads++;
      return { startedAtMs: 10, completedAtMs: 10 + duration, durationMs: duration, workDurationMs: null };
    },
  };
  const signature = (version: number, active = false) =>
    getCachedHistoricalThinkingSignature(history, "agent", timings, version, active);
  assert.equal(signature(1), "2:17;");
  duration = 23;
  assert.equal(signature(1), "2:17;");
  assert.equal(reads, 1);
  assert.equal(signature(1, true), "2:23;");
  assert.equal(signature(2), "2:23;");
  assert.equal(signature(2), "2:23;");
  assert.equal(reads, 3);
  assert.equal(getCachedHistoricalThinkingSignature(history, "agent", {}, 2, false), "");
});

test("timings stay isolated by turn and host and both history and single-turn block counts are bounded", () => {
  let history: StageTimingHistory = new Map();
  const first = turn(1, [group(thought(2))]);
  history = updateStageTimingHistory(history, "local", "agent", first, true, 100);
  assert.equal(updateStageTimingHistory(history, "local", "agent", first, true, 120), history);
  history = updateStageTimingHistory(history, "local", "agent", first, false, 150);
  const completed = history.get("local::agent::turn-1")!;
  assert.equal(completed.thinkingVersion, 2);
  assert.equal(completed.getThinkingDurationMs("agent::think::item-2"), 50);
  for (let i = 1; i <= 201; i++) {
    history = updateStageTimingHistory(history, "remote", "agent", turn(1, [group(thought(i))]), true, 200 + i);
  }
  assert.equal(history.get("local::agent::turn-1"), completed);
  const remote = history.get("remote::agent::turn-1")!;
  assert.equal(Object.keys(remote.thinking).length, 200);
  assert.equal(remote.getThinkingDurationMs("agent::think::item-1"), null);
  assert.equal(remote.getThinkingDurationMs("agent::think::item-2"), 1);
  for (let i = 2; i <= 201; i++) {
    history = updateStageTimingHistory(history, "remote", "agent", turn(i), true, 450 + i);
  }
  assert.equal(history.size, 200);
  assert.equal(history.has("local::agent::turn-1"), false);
  assert.equal(history.has("remote::agent::turn-1"), false);
});

test("unrelated heartbeats reuse the session snapshot while cross-host peer decisions and renames propagate", () => {
  const select = createStageSessionSelector("local", "parent");
  const remote = { host_id: "remote", display_name: "Build Mac" } as HostCardView;
  const child = { agent_id: "ag_peer", title: "Reviewer", status_label: "Idle", latest_seq: 3 } as AgentCardView;
  const permission = {
    agent_id: "ag_peer", request_id: "approval", title: "Read", agent_title: "Reviewer",
    detail: { kind: "file_read", path: "README.md", start_line: null, end_line: null } as const,
    allowed_decisions: ["allow"], created_at: "",
  };
  const host = { host: remote, agents: [child], pending_permissions: [permission] } as HostDetailView;
  // A dispatched workspace peer can lack a parent pointer on its remote host.
  const stage = {
    insights: { subagents: [{ id: "spawn", child_agent_id: "ag_peer", role: "Reviewer", last_seq: 3 }] },
    pending_questions: [], pending_permissions: [],
  } as unknown as OrbitUiState["stages"][string];
  const state = {
    hostDetails: { remote: host }, stages: { "local::parent": stage }, archivedSubagents: {},
    fleet: { hosts: [remote], revision: 1, projects: [], total_pending_approvals: 1 },
  };
  const snapshot = select(state);
  assert.equal(snapshot.decisions.subagentPermissions[0].permission, permission);
  assert.equal(snapshot.subagents[0].remoteHostName, "Build Mac");
  assert.equal(select(state), snapshot);
  const other = { host: { ...remote, host_id: "other" }, agents: [] } as unknown as HostDetailView;
  assert.equal(select({ ...state, hostDetails: { ...state.hostDetails, other } }), snapshot);
  assert.equal(select({ ...state, fleet: { ...state.fleet, hosts: [other.host, remote] } }), snapshot);
  assert.equal(select({ ...state, stages: { "local::parent": { ...stage, revision: 99 } } }), snapshot);
  const renamed = select({ ...state, fleet: { ...state.fleet, hosts: [{ ...remote, display_name: "Remote Mac" }] } });
  assert.equal(renamed.subagents[0].remoteHostName, "Remote Mac");
  const resolved = select({ ...state, hostDetails: { remote: { ...host, pending_permissions: [] } } });
  assert.equal(resolved.decisions.totalCount, 0);

  const matcher = buildSubagentNameMatcher(snapshot.subagents);
  assert.ok(matcher);
  const forTurn = createTurnProseMatcherLookup(matcher);
  const prose = (text: string) => turn(1, [{
    kind: "assistant_message", key: "reply", item: {
      item_type: "assistant_message", seq: 2, item_id: "reply", text, streaming: false,
      blocks: [{ kind: "paragraph", spans: [{ text, bold: false, italic: false, strikethrough: false, inline_code: false, link_url: null }] }],
    },
  }]);
  assert.equal(forTurn(prose("See Reviewer for details")), matcher);
  assert.equal(forTurn(prose("Work complete")), null);
});

test("both shared relative clocks stop while hidden, resync on return and release their last listener", (t) => {
  const doc = Object.assign(new EventTarget(), { hidden: true });
  const timers = new Map<number, number>();
  let id = 0;
  const win = {
    setInterval(_tick: () => void, delay: number) { timers.set(++id, delay); return id; },
    clearInterval(timer: number) { timers.delete(timer); },
  };
  Object.defineProperty(globalThis, "window", { configurable: true, value: win });
  Object.defineProperty(globalThis, "document", { configurable: true, value: doc });
  t.after(() => {
    Reflect.deleteProperty(globalThis, "window");
    Reflect.deleteProperty(globalThis, "document");
  });
  let now = 1000;
  t.mock.method(Date, "now", () => now);
  const clocks = [createRelativeClock(15_000), createRelativeClock(60_000)];
  let calls = 0;
  const unsubscribe = clocks.map((clock) => clock.subscribe(() => calls++));
  const extra = clocks[0].subscribe(() => {});
  const visibility = (hidden: boolean) => {
    doc.hidden = hidden;
    doc.dispatchEvent(new Event("visibilitychange"));
  };
  assert.equal(timers.size, 0);
  visibility(false);
  assert.deepEqual([...timers.values()], [15_000, 60_000]);
  assert.equal(calls, 2);
  visibility(true);
  assert.equal(timers.size, 0);
  now += 300_000;
  visibility(false);
  assert.equal(calls, 4);
  assert.deepEqual(clocks.map((clock) => clock.getSnapshot()), [now, now]);
  unsubscribe.forEach((off) => off());
  assert.equal(timers.size, 1);
  extra();
  visibility(true);
  visibility(false);
  assert.equal(timers.size, 0);
});

test("relative labels and cached calendar dates follow the Stage locale", () => {
  const stamp = new Date(2024, 1, 10, 14, 35).getTime();
  const minute = 60_000;
  const ages = [0, 2 * minute, 2 * 60 * minute, 2 * 24 * 60 * minute];
  assert.deepEqual(ages.map((age) => formatRelativeTimeLabel(stamp, stamp + age)), [
    "just now", "2m ago", "2h ago", "2d ago",
  ]);
  for (const [locale, expected] of [
    ["zh-CN", ["刚刚", "2 分钟前", "2 小时前", "2 天前"]],
    ["ja", ["たった今", "2分前", "2時間前", "2日前"]],
  ] as const) {
    assert.deepEqual(ages.map((age) => formatRelativeTimeLabel(stamp, stamp + age, locale)), expected);
  }

  // Revisit the same timestamp after switching locales to exercise the tooltip cache.
  for (const locale of ["en", "zh-CN", "ja", "en"] as const) {
    assert.equal(formatFullTimestampTooltip(stamp, locale), new Intl.DateTimeFormat(locale, {
      year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false,
    }).format(stamp));
    for (const year of [2024, 2025]) {
      const now = new Date(year, 6, 1).getTime();
      const date = new Intl.DateTimeFormat(locale, {
        month: "short", day: "numeric", year: year === 2024 ? undefined : "numeric",
      }).format(stamp);
      assert.equal(formatRelativeTimeLabel(stamp, now, locale), date);
    }
  }
});

test("narrow host selectors preserve identity across unrelated sibling host updates and short prompts skip overflow measurement", () => {
  assert.equal(mayUserPromptOverflow("A"), false);
  assert.equal(mayUserPromptOverflow("short single line prompt"), false);
  assert.equal(mayUserPromptOverflow("line1\nline2"), true);
  assert.equal(mayUserPromptOverflow("x".repeat(73)), true);
  assert.equal(mayUserPromptOverflow("short", 1), true);

  const selectAgent = createHostAgentSelector("h1", "a1");
  const selectArtifacts = createStableHostArtifactsSelector("h1");
  const makeAgent = (id: string, status: string): AgentCardView => ({
    agent_id: id,
    workspace_id: "ws1",
    provider: "claude",
    title: id,
    task_summary: "",
    cwd: "/repo",
    status_label: status,
    latest_seq: 1,
    is_running: false,
    is_awaiting_permission: false,
    model: "sonnet",
    effort: "medium",
    updated_at_rfc3339: "2026-01-01T00:00:00Z",
    mode: { kind: "default" },
    queued_prompts: [],
  } as unknown as AgentCardView);

  const state1 = {
    hostDetails: {
      h1: {
        agents: [makeAgent("a1", "Idle"), makeAgent("a2", "Idle")],
        artifacts: [{ artifact_id: "art1", title: "Spec", version: 1, kind: "doc", resource_uri: "uri://1" }],
      } as unknown as HostDetailView,
    },
  };
  const firstAgent = selectAgent(state1);
  const firstArtifacts = selectArtifacts(state1);

  // Sibling agent `a2` mutates and `hostDetails.h1` is cloned with structurally equivalent `a1` & `artifacts`
  const state2 = {
    hostDetails: {
      h1: {
        agents: [makeAgent("a1", "Idle"), makeAgent("a2", "Running")],
        artifacts: [{ artifact_id: "art1", title: "Spec", version: 1, kind: "doc", resource_uri: "uri://1" }],
      } as unknown as HostDetailView,
    },
  };
  assert.equal(selectAgent(state2), firstAgent);
  assert.equal(selectArtifacts(state2), firstArtifacts);

  // Genuine status mutation on `a1` propagates immediately
  const state3 = {
    hostDetails: {
      h1: {
        agents: [makeAgent("a1", "Running"), makeAgent("a2", "Running")],
        artifacts: [{ artifact_id: "art1", title: "Spec", version: 2, kind: "doc", resource_uri: "uri://1" }],
      } as unknown as HostDetailView,
    },
  };
  assert.notEqual(selectAgent(state3), firstAgent);
  assert.equal(selectAgent(state3)?.status_label, "Running");
  assert.notEqual(selectArtifacts(state3), firstArtifacts);
  assert.equal(selectArtifacts(state3)[0]?.version, 2);
});

test("F9 turn height cache prefix includes typographyKey and fallback estimate matches default exactly", () => {
  assert.equal(
    buildTurnHeightCachePrefix("host1", "agent1", 24, "r15:c13:default:match-ui:default"),
    "host1::agent1::24::r15:c13:default:match-ui:default::",
  );
  assert.equal(
    buildTurnHeightCachePrefix("side", "scope-1", "panel", "r18:c16:inter:system-serif:menlo"),
    "side::scope-1::panel::r18:c16:inter:system-serif:menlo::",
  );

  const plainSpan = (text: string) => ({
    text,
    bold: false,
    italic: false,
    strikethrough: false,
    inline_code: false,
    link_url: null,
  });

  const richTurn: StageTurnView = {
    ...turn(42, [
      {
        kind: "assistant_message",
        key: "msg-42",
        item: {
          item_type: "assistant_message",
          seq: 43,
          item_id: "m-43",
          text: "",
          streaming: false,
          blocks: [
            { kind: "heading", level: 2, spans: [plainSpan("Summary")] },
            {
              kind: "paragraph",
              spans: [plainSpan("A".repeat(144))],
            },
            {
              kind: "list_item",
              ordered: false,
              index: 0,
              checked: null,
              depth: 0,
              spans: [plainSpan("B".repeat(136))],
            },
            {
              kind: "table",
              alignments: ["left", "left"],
              headers: [
                { spans: [plainSpan("Col A")] },
                { spans: [plainSpan("Col B")] },
              ],
              rows: [
                { cells: [{ spans: [plainSpan("1")] }, { spans: [plainSpan("2")] }] },
                { cells: [{ spans: [plainSpan("3")] }, { spans: [plainSpan("4")] }] },
              ],
            },
            {
              kind: "code_block",
              language: "ts",
              raw_code: "1\n2\n3\n4\n5",
              lines: [1, 2, 3, 4, 5].map((n) => ({
                line_number: n,
                tokens: [{ text: `const x${n} = ${n};`, fg_hex: "#d4d4d8", bold: false, italic: false }],
              })),
            },
          ],
        },
      },
    ]),
    user_prompt: {
      item_type: "user_prompt",
      seq: 42,
      message_id: "u-42",
      text: "C".repeat(112),
      blocks: [],
    },
  };

  // Legacy default calculation at readScale=1, codeLinePx=20:
  // prompt: 56 + min(124, ceil(112/56)*24=48) = 104
  // blocks: 16 + heading(42) + para(max(34, 2*25+14)=64) + list(max(28, 2*24+8)=56)
  //       + table(max(96, 3*38+24)=138) + code(max(84, 5*21+52)=157) = 473
  // total = 104 + 473 = 577
  const defaultEst = estimateTurnSizeFallback(richTurn);
  assert.equal(defaultEst, 577);
  assert.equal(estimateTurnSizeFallback(richTurn, DEFAULT_TURN_SIZE_METRICS), 577);

  const largerEst = estimateTurnSizeFallback(richTurn, { readScale: 22 / 15, codeLinePx: 28 });
  assert.ok(largerEst > defaultEst, `Expected larger estimate (${largerEst}) > default (${defaultEst})`);

  const row = { key: richTurn.key, topGapPx: 14, spacingPx: 40 };
  assert.equal(getTurnItemHeight(richTurn, row, undefined, DEFAULT_TURN_SIZE_METRICS), 40 + 577);
  assert.equal(getTurnItemHeight(richTurn, row, undefined, { readScale: 22 / 15, codeLinePx: 28 }), 40 + largerEst);
});
