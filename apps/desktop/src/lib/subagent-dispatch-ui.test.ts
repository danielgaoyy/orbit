import assert from "node:assert/strict";
import { after, before, it } from "node:test";
import { Children, createElement, isValidElement, useState, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let server: ViteDevServer;
before(async () => {
  server = await createServer({
    server: { middlewareMode: true, hmr: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [{
      name: "test-subagent-panel-actions",
      enforce: "pre",
      load(id) {
        if (id.endsWith("/state/orbit-store.ts")) return `
          export const opened = [];
          export const orbitActions = { openSubagentTab: (record) => opened.push(record) };
          export const state = {
            hostDetails: {remote: {
              host: {host_id: "remote"},
              agents: ["ag_review", "ag_Reviewer", "ag_Auditor"].map(agent_id => ({agent_id})),
            }},
            stages: {},
          };
          export const useOrbitSelector = (selector) => selector(state);
          export const getOrbitState = () => state;
        `;
      },
    }],
  });
});
after(async () => { await server?.close(); });

// Render the actual memo component with React's hook dispatcher, and exercise
// its action and controlled disclosure callbacks without a Tauri runtime.
function renderRow(
  component: { type: (props: object) => ReactNode },
  props: object,
  clickSequence: number[] = [],
) {
  const clicks: Array<() => void> = [];
  function collectButtons(node: ReactNode) {
    Children.forEach(node, (child) => {
      if (!isValidElement<{
        onClick?: () => void; open?: boolean; onOpenChange?: (open: boolean) => void;
        action?: ReactNode; children?: ReactNode;
      }>(child)) return;
      const { open, onOpenChange } = child.props;
      if (onOpenChange) clicks.push(() => onOpenChange(!open));
      if (child.props.onClick) clicks.push(child.props.onClick);
      collectButtons(child.props.action);
      collectButtons(child.props.children);
    });
  }
  let step = 0;
  function Row() {
    clicks.length = 0;
    const tree = component.type(props);
    collectButtons(tree);
    // React applies render-phase updates before producing the final markup.
    if (step < clickSequence.length) clicks[clickSequence[step++]]();
    return tree;
  }
  return { html: renderToStaticMarkup(createElement(Row)), clicks };
}

function assertIndependentControls(html: string) {
  for (const [, , content] of html.matchAll(/<(button|a)\b[^>]*>([\s\S]*?)<\/\1>/g)) {
    assert.doesNotMatch(content, /<(?:button|a)\b/);
  }
}

it("opens the subagent panel directly from overview and full-width dispatch rows", async () => {
  const { SubagentCrewList } = await server.ssrLoadModule("/src/components/stage/subagent-crew-dock.tsx");
  const { DelegationMilestoneRow } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  const { opened } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const peer = { key: "managed:remote:ag_review", callId: "ag_review", title: "Reviewer",
    childHostId: "remote", childAgentId: "ag_review", providerSubagentId: null, isManaged: true,
    promptPreview: "Inspect changes", status: "awaiting_permission" };
  for (const status of ["awaiting_permission", "running", "completed"]) {
    const overview = renderRow(SubagentCrewList, { agentId: "parent", subagents: [{ ...peer, status }] });
    assert.equal(overview.clicks.length, 1);
    if (status === "awaiting_permission") assert.match(overview.html, /Approval required/);
    overview.clicks[0]();
    assert.equal(opened.at(-1).childAgentId, "ag_review");
  }

  const item = { seq: 1, item_type: "tool_call", call_id: "launch", status: "running",
    detail: { kind: "generic", tool_name: "Run mcp orbit create agent",
      arguments_json: '{"title":"Reviewer"}' } };
  for (const [rpcStatus, childStatus, statusLabel] of [
    ["running", null, "Starting…"],
    ["completed", null, "Starting…"],
    ["completed", "running", "Working…"],
    ["completed", "awaiting_permission", "Approval required"],
    ["running", "completed", "Done"],
    ["completed", "failed", "Failed"],
    ["completed", "canceled", "Stopped"],
    ["denied", null, "Failed"],
  ] as const) {
    const row = renderRow(DelegationMilestoneRow, {
      items: [{ ...item, status: rpcStatus }], hostId: "local", agentId: "parent",
      sessionSubagents: childStatus ? [{ ...peer, status: childStatus }] : [],
    });
    assert.equal(row.clicks.length, 1);
    assert.equal(row.html.replace(/<[^>]+>/g, "").trim(), "Reviewer" + statusLabel);
    assert.equal(row.html.match(/<button\b/g)?.length, 1);
    assert.match(row.html, /min-h-\[28px\]/);
    assert.match(row.html, /w-\[calc\(100%\+1[24]px\)\]/);
    assert.match(row.html, /shrink-0 max-w-\[65%\] truncate/);
    assert.match(row.html, /group-focus-visible\/sachip:opacity-100/);
    assert.doesNotMatch(row.html, /aria-expanded|bg-surface-raised|border-border-subtle/);
    assert.match(row.html, /data-slot="subagent-avatar"/);
    assert.ok(row.html.includes(`data-status="${childStatus ?? (rpcStatus === "denied" ? "failed" : "running")}"`));
    const status = childStatus ?? (rpcStatus === "denied" ? "failed" : "running");
    assert.equal(/lucide-(check|x)\b/.test(row.html), ["completed", "failed", "canceled"].includes(status));
    assert.equal(row.html.includes("border-signal-warning-border"), status === "awaiting_permission");
    assert.equal(row.html.includes("border-signal-danger-border"), status === "failed");
    row.clicks[0]();
    assert.equal(opened.at(-1).callId, "launch");
    assert.equal(opened.at(-1).childAgentId, childStatus ? "ag_review" : null);
    assert.equal(opened.at(-1).parentAgentId, "parent");
  }
});

it("toggles outgoing messages on the main row and opens the recipient separately", async () => {
  const { DelegationMilestoneRow } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  const { opened } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const props = {
    agentId: "parent",
    sessionSubagents: [{ key: "review", callId: "launch", title: "Reviewer", childHostId: "remote",
      childAgentId: "ag_review", providerSubagentId: null, status: "completed" }],
    items: [{ item_type: "tool_call", seq: 1, call_id: "send", status: "completed",
      detail: { kind: "generic", tool_name: "send_message",
        arguments_json: JSON.stringify({ recipient: "ag_review", message: "Review ready" }) } }],
  };
  const before = opened.length;
  const expanded = renderRow(DelegationMilestoneRow, props, [0]);
  assert.match(expanded.html, /aria-expanded="true"/);
  assert.match(expanded.html, /<p[^>]*>Review ready<\/p>/);
  const folded = renderRow(DelegationMilestoneRow, props, [0, 0]);
  assert.match(folded.html, /aria-expanded="false"/);
  assert.doesNotMatch(folded.html, /<p[ >]|border-border-subtle|bg-surface-recessed/);
  assert.equal(opened.length, before);
  assert.match(folded.html, /data-slot="collapsible-trigger"/);
  assert.match(folded.html, /aria-label="Open Reviewer"/);
  assert.doesNotMatch(folded.html, /lucide-arrow-up-right/);
  assertIndependentControls(folded.html);
  assert.match(expanded.html, /data-slot="collapsible-content"/);
  folded.clicks[1]();
  assert.equal(opened.length, before + 1);
  assert.equal(opened.at(-1).childAgentId, "ag_review");
  assert.equal(opened.at(-1).childHostId, "remote");
});

it("renders historical launches with the overview portraits and opens their respective child tabs", async () => {
  const { SubagentCrewList } = await server.ssrLoadModule("/src/components/stage/subagent-crew-dock.tsx");
  const { DelegationMilestoneRow } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  const { assignSubagentCharacters } = await server.ssrLoadModule("/src/lib/subagent-avatar.ts");
  const { opened } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const children = ["Auditor", "Reviewer"].map((title, i) => ({
    key: "managed:" + title, callId: "ag_" + title, childAgentId: "ag_" + title,
    title, childHostId: "remote", createdAtMs: 50000 + i * 1000, isManaged: true,
    status: i === 0 ? "completed" : "awaiting_permission",
  }));
  const characters = assignSubagentCharacters(children, (sa: typeof children[number]) => sa.childAgentId);
  const sessionSubagents = children.map((sa) => ({ ...sa, character: characters.get(sa.key) }));
  const items = children.map((sa, seq) => ({
    item_type: "tool_call", seq, call_id: "launch-" + seq, status: "completed",
    timestamp_ms: sa.createdAtMs - 20, detail: { kind: "generic", tool_name: "create_agent", arguments_json: "{}" },
  }));
  const timeline = renderRow(DelegationMilestoneRow, { items, sessionSubagents: [...sessionSubagents].reverse(), agentId: "parent" });
  const overview = renderRow(SubagentCrewList, { agentId: "parent", subagents: sessionSubagents });
  const portraits = (html: string) => Array.from(html.matchAll(/data-character="([^"]+)"/g), (m) => m[1]);
  assert.deepEqual(portraits(timeline.html), portraits(overview.html));
  assert.match(timeline.html, /flex-col gap-0\.5/);
  assert.match(timeline.html.replace(/<[^>]+>/g, ""), /^AuditorDoneReviewerApproval required· /);
  assert.doesNotMatch(timeline.html, /Spawned subagent|waiting for response/);
  timeline.clicks.forEach((click, i) => {
    click();
    assert.equal(opened.at(-1).childAgentId, children[i].childAgentId);
  });
  assert.equal(timeline.clicks.length, 2);
});

it("uses the current run for dispatch elapsed time and removes the clock in every settled state", async () => {
  const { DelegationMilestoneRow } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  const { useLiveSecondClock } = await server.ssrLoadModule("/src/lib/live-clock.ts");
  const now = Number(renderToStaticMarkup(createElement(() => String(useLiveSecondClock(true)))));
  const launch = { item_type: "tool_call", seq: 1, call_id: "launch", status: "completed",
    timestamp_ms: now - 25 * 60_000, summary_title: "mcp_orbit_create_agent",
    detail: { kind: "subagent", role: "Reviewer", prompt_preview: "", child_agent_id: "ag_review" } };
  const child = { key: "managed:remote:ag_review", callId: "launch", title: "Reviewer",
    childHostId: "remote", childAgentId: "ag_review", isManaged: true,
    createdAtMs: launch.timestamp_ms, activeStartedAtMs: now - 10_000 };
  for (const status of ["running", "awaiting_permission", "completed", "failed", "canceled"]) {
    const { html } = renderRow(DelegationMilestoneRow, {
      items: [launch], sessionSubagents: [{ ...child, status }], hostId: "local", agentId: "parent",
    });
    assert.doesNotMatch(html, /· 2[45]m/);
    if (status === "running" || status === "awaiting_permission") assert.match(html, /· 10s</);
    else assert.doesNotMatch(html, /data-slot="trace-description"/);
  }
});

it("uses one inline step rail and offers a bottom collapse action from ten steps", async () => {
  const { TurnActivityDisclosure } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  for (const count of [1, 9, 10, 50]) {
    const props = {
      items: Array.from({ length: count }, (_, seq) => ({
        item_type: "assistant_message", seq, text: "Step " + (seq + 1), blocks: [], streaming: false,
      })),
      agentId: "parent", cwd: "/", isWorkActive: false,
      settledLabel: "Work summary", summaryCounts: {}, defaultCollapsed: true, autoOpenForError: false,
      getThinkingDurationMs: () => null,
      holdViewport: (_anchor: unknown, apply: () => void) => apply(),
    };
    const expanded = renderRow(TurnActivityDisclosure, props, [0]);
    assert.match(expanded.html, /aria-expanded="true"/);
    assert.equal(expanded.html.match(/data-slot="trace"/g)?.length, 2);
    assert.equal(expanded.html.match(/data-slot="trace-line"/g)?.length, count + 1);
    assert.equal(expanded.html.match(/data-slot="trace-collapsible-content-inner"/g)?.length, 1);
    assert.match(expanded.html, /data-slot="trace-collapsible-trigger"/);
    assertIndependentControls(expanded.html);
    assert.ok(expanded.html.includes("Step " + count + "</span>"));
    assert.doesNotMatch(expanded.html, /orbit-disclosure-(track|scroll)|orbit-sticky-disclosure-bar|lucide-(maximize|minimize)|max-h-\[340px\]|overflow-y-auto/);
    assert.equal(expanded.html.includes("Collapse " + count + " steps"), count >= 10);
    if (count >= 10) {
      const folded = renderRow(TurnActivityDisclosure, props, [0, 1]);
      assert.match(folded.html, /aria-expanded="false"/);
      assert.doesNotMatch(folded.html, /Step 1|Collapse \d+ steps/);
    }
  }
});

it("keeps hook order when a compaction-only turn gains ordinary activity", async () => {
  const { TurnStageSection } = await server.ssrLoadModule("/src/components/stage/stage-turn-card.tsx");
  function TurnUpdate() {
    const [hasActivity, setHasActivity] = useState(false);
    const item = { item_type: "notice", level: "info", message: "Compaction finished", seq: 2 };
    const tree = TurnStageSection.type({
      hostId: "local", agentId: "parent", cwd: "/", hostArtifacts: [], isTurnActive: false,
      turn: {
        is_compaction_command: true, folded_steps: [], terminal_reply: null, compactions: [], turn_diffs: [],
        notices: hasActivity ? [item] : [], live_segments: hasActivity ? [{ kind: "notice", key: "notice", item }] : [],
      },
    });
    if (!hasActivity) setHasActivity(true);
    return tree;
  }
  const html = renderToStaticMarkup(createElement(TurnUpdate));
  assert.match(html, /Compaction finished/);
  assert.match(html, /contain:layout style/);
});

it("keeps incoming message expansion separate from opening a provider-native sender", async () => {
  const { AgentMessageCard } = await server.ssrLoadModule("/src/components/stage/subagents-track.tsx");
  const { opened } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const props = { hostId: "local", agentId: "parent", sessionSubagents: [{
    callId: "launch", title: "Reviewer", childAgentId: null,
    providerSubagentId: "native-review", childHostId: "local",
  }], item: { kind: "direct_message", source_agent_id: "native-review", source_title: "Reviewer",
    text: "Review ready", blocks: [], seq: 2 } };
  const before = opened.length;
  const expanded = renderRow(AgentMessageCard, props, [0]);
  assert.match(expanded.html, /aria-expanded="true"/);
  assert.match(expanded.html, /<p[^>]*>Review ready<\/p>/);
  assert.equal(opened.length, before);
  expanded.clicks[1]();
  assert.equal(opened.at(-1).providerSubagentId, "native-review");
  assert.equal(opened.at(-1).callId, "launch");
});

it("tracks the exact child's pending approval across reloads and exposes only Inspect", async () => {
  const { AgentMessageCard } = await server.ssrLoadModule("/src/components/stage/subagents-track.tsx");
  const { opened, state } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const props = { hostId: "local", agentId: "parent", item: {
    kind: "direct_message", source_title: "Reviewer", text: "Permission requested", blocks: [], seq: 2,
    permission_context: { child_agent_id: "ag_review", child_host_id: "remote",
      permission_request_id: "req", title: "Read report" },
  } };
  const host = state.hostDetails.remote;
  const stageKey = "remote::ag_review";
  try {
    state.stages[stageKey] = { pending_permissions: [
      { item_type: "permission_request", request_id: "req", resolved_decision: null },
    ] };
    assert.match(renderRow(AgentMessageCard, props).html, /Awaiting decision/);
    host.pending_permissions = [{ agent_id: "ag_review", request_id: "req" }];
    const pending = renderRow(AgentMessageCard, props);
    assert.match(pending.html, /data-status="awaiting_permission"/);
    assert.match(pending.html, /Reviewer requested approval/);
    assert.match(pending.html, /Read report/);
    assert.doesNotMatch(pending.html, />Allow<|>Deny</);
    assert.equal(pending.clicks.length, 1);
    pending.clicks[0]();
    assert.equal(opened.at(-1).childAgentId, "ag_review");
    assert.equal(opened.at(-1).childHostId, "remote");

    host.pending_permissions = [{ agent_id: "ag_other", request_id: "req" },
      { agent_id: "ag_review", request_id: "other" }];
    const settled = renderRow(AgentMessageCard, props);
    assert.match(settled.html, /data-status="completed"/);
    assert.match(settled.html, />Resolved</);
    assert.doesNotMatch(settled.html, /requested approval|Awaiting decision|border-signal-warning/);
    assert.match(settled.html, /data-slot="collapsible-trigger"/);
    assert.doesNotMatch(settled.html, /border-border-subtle|bg-surface-recessed/);
    assert.equal(settled.clicks.length, 2);
  } finally {
    delete host.pending_permissions;
    delete state.stages[stageKey];
  }
});

it("reuses dispatch maps until a resolved entry or timeline key changes", async () => {
  const { useDelegationDispatches } = await server.ssrLoadModule("/src/components/stage/use-delegation-dispatches.ts");
  const launch = { item_type: "tool_call", seq: 1, call_id: "launch", status: "completed",
    detail: { kind: "generic", tool_name: "create_agent", arguments_json: '{"agent_id":"ag_review"}' } };
  const peer = { callId: "launch", isManaged: true, childAgentId: "ag_review", title: "Reviewer", status: "running" };
  const results: Map<unknown, unknown>[] = [];
  function Probe() {
    const [step, advance] = useState(0);
    const items = step < 2 ? [] : [step === 5 ? { ...launch } : launch];
    const peers = [{ ...peer, status: step < 4 ? "running" : "completed" }, { title: "other-" + step }];
    results.push(useDelegationDispatches([{ live_segments: [{ kind: "step_group", items }] }], peers, "local"));
    if (step < 5) advance(step + 1);
    return null;
  }
  renderToStaticMarkup(createElement(Probe));
  assert.equal(results[0].size, 0);
  assert.equal(results[0], results[1]);
  assert.equal(results[2].size, 1);
  assert.equal(results[2], results[3]);
  assert.notEqual(results[3], results[4]);
  assert.notEqual(results[4], results[5]);
});

it("shows one frameless peer report with the finish badge and status, preserving separate failure alerts", async () => {
  const { TurnStageSection } = await server.ssrLoadModule("/src/components/stage/stage-turn-card.tsx");
  const report = {
    item_type: "agent_message", kind: "direct_message", seq: 1, message_id: "report",
    source_agent_id: "ag_review", source_title: "Reviewer", text: "Review ready", blocks: [],
  };
  for (const [event, expectedStatus, badge] of [
    ["finished", "finished", "bg-signal-success"],
    ["canceled", "stopped", "bg-ink-muted"],
    ["errored", "failed", "bg-signal-danger"],
  ]) {
    const finish = { ...report, seq: 5, message_id: "finish", kind: "finish_notification", event, text: "Final notification" };
    const turn = {
      agent_message: report, folded_steps: [], notices: [], compactions: [], turn_diffs: [],
      live_segments: [{ kind: "agent_message", key: "finish", item: finish }],
    };
    const html = renderRow(TurnStageSection, {
      hostId: "local", agentId: "parent", cwd: "/", hostArtifacts: [], turn, isTurnActive: false,
    }).html;
    assert.equal(html.match(/data-slot="subagent-avatar"/g)?.length, event === "errored" ? 2 : 1);
    assert.match(html, new RegExp(badge));
    assert.match(html, new RegExp(">" + expectedStatus + "<", "i"));
    assert.match(html, /Review ready/);
    if (event === "errored") {
      assert.match(html, /border-signal-danger-border bg-signal-danger-bg\/45/);
      assert.match(html, /Final notification/);
    } else {
      assert.doesNotMatch(html, /Final notification|bg-surface-recessed\/55|border-signal-danger-border/);
      const rowClasses = html.match(/class="(group\/msgrow [^"]+)"/)?.[1] ?? "";
      assert.match(rowClasses, /hover:bg-accent\/55/);
      assert.doesNotMatch(rowClasses, /\bborder\b|bg-surface-raised/);
    }
  }
});

it("expands heartbeat and timer details on a left rail, with a copy action for the timer", async () => {
  const { AgentMessageCard } = await server.ssrLoadModule("/src/components/stage/subagents-track.tsx");
  const { DelegationMilestoneRow } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  const heartbeat = renderRow(AgentMessageCard, {
    hostId: "local", item: { kind: "heartbeat", seq: 1, text: "Still checking", blocks: [] },
  }, [0]);
  assert.match(heartbeat.html, /data-slot="collapsible-content"/);
  assert.match(heartbeat.html, /<p[^>]*>Still checking<\/p>/);

  const timer = {
    item_type: "tool_call", seq: 2, call_id: "timer", summary_title: "Timer",
    detail: { kind: "generic", tool_name: "orbit_timer", arguments_json: '{"action":"create","prompt":"Check progress","duration_seconds":300}' },
  };
  for (const status of ["completed", "failed", "denied"]) {
    const row = renderRow(DelegationMilestoneRow, { items: [{ ...timer, status }] }, [0]);
    assert.match(row.html, /Timer \(5m\)/);
    assert.match(row.html, /data-slot="trace-collapsible-content"/);
    assert.match(row.html, /Check progress/);
    assert.match(row.html, /aria-label="Copy"/);
    assert.doesNotMatch(row.html, /bg-surface-recessed\/65/);
    assert.equal(row.html.includes('data-status="error"'), status !== "completed");
    assertIndependentControls(row.html);
  }
});

it("keeps an empty live status static and remembers a manual override on an auto-opened failure", async () => {
  const { TurnActivityDisclosure } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  let holds = 0;
  const props = {
    items: [], isWorkActive: true, liveLabel: "Thinking…", isNetworkOffline: true,
    summaryCounts: {}, defaultCollapsed: true, autoOpenForError: true, cwd: "/",
    agentId: "trace-persistence", turnIndex: 1, turnAnchorSeq: 42, workOrdinal: 0,
    getThinkingDurationMs: () => null,
    holdViewport: (_anchor: unknown, apply: () => void) => { holds++; apply(); },
  };
  const live = renderRow(TurnActivityDisclosure, props);
  assert.match(live.html, /data-status="running"/);
  assert.match(live.html, /Thinking…/);
  assert.match(live.html, /Offline/i);
  assert.doesNotMatch(live.html, /<button\b|data-slot="trace-collapsible"/);
  assert.equal(renderRow(TurnActivityDisclosure, { ...props, isWorkActive: false }).html, "");

  const failed = {
    ...props, isWorkActive: false, settledLabel: "Worked", settledDurationMs: 3200,
    summaryCounts: { failed_count: 1, unrecovered_failure_count: 1 },
    items: [{ item_type: "tool_call", seq: 1, status: "failed", diff_files: [],
      detail: { kind: "shell", command: "pwd", output_preview: "failure output", exit_code: 1 } }],
  };
  const initial = renderRow(TurnActivityDisclosure, failed);
  assert.equal(initial.html.match(/aria-expanded="true"/g)?.length, 2);
  assert.match(initial.html, /failure output/);
  assert.match(initial.html, /data-slot="trace-description"[^>]*>· 3s</);
  assert.match(initial.html, /1 failed/);
  assertIndependentControls(initial.html);
  renderRow(TurnActivityDisclosure, failed, [0]);
  assert.equal(holds, 1);
  assert.match(renderRow(TurnActivityDisclosure, failed).html, /aria-expanded="false"/);
  assert.match(renderRow(TurnActivityDisclosure, { ...failed, turnAnchorSeq: 43 }).html, /aria-expanded="true"/);
});

it("exposes tool details only when present and keeps navigation and Copy outside the disclosure button", async () => {
  const { StepItemRow } = await server.ssrLoadModule("/src/components/stage/stage-tool-renderers.tsx");
  const props = { cwd: "/", holdViewport: (_anchor: unknown, apply: () => void) => apply() };
  const tool = { item_type: "tool_call", call_id: "launch", seq: 1, status: "running", summary_title: "", diff_files: [] };
  const generic = { kind: "generic", tool_name: "lookup", arguments_json: "{}" };
  const pending = renderRow(StepItemRow, { ...props, item: { ...tool, detail: generic } });
  assert.match(pending.html, /data-status="running"/);
  assert.doesNotMatch(pending.html, /<button\b/);

  for (const detail of [
    { ...generic, arguments_json: '{"project":"orbit"}' },
    { kind: "subagent", role: "Reviewer", prompt_preview: "Review changes" },
    { kind: "file_edit", path: "/example.ts", unified_diff: "@@ -1 +1 @@\n-old\n+new" },
  ]) {
    const row = renderRow(StepItemRow, { ...props, item: { ...tool, status: "denied", detail } }, [0]);
    assert.match(row.html, /data-status="error"/);
    assert.match(row.html, /aria-expanded="true"/);
    if (detail.kind === "file_edit") assert.match(row.html, /title="Open in Diff Review"/);
    else assert.match(row.html, /aria-label="Copy"/);
    if (detail.kind === "subagent") assert.match(row.html, /title="Open subagent details"/i);
    assertIndependentControls(row.html);
  }
});

it("renders system notes as markers and a detail-free worktree handoff as a static trace", async () => {
  const { CompactionMilestoneRow, WakeupEventRow, DelegationMilestoneRow } =
    await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  for (const [component, variant] of [[CompactionMilestoneRow, "separator"], [WakeupEventRow, "default"]]) {
    const { html } = renderRow(component, { label: "Checkpoint" });
    assert.ok(html.includes(`data-variant="${variant}"`));
    assert.match(html, /data-slot="marker-icon"/);
    assert.match(html, /data-slot="marker-content"[^>]*>Checkpoint</);
  }
  const handoff = renderRow(DelegationMilestoneRow, { items: [{
    item_type: "tool_call", seq: 1, status: "running", summary_title: "Handoff",
    detail: { kind: "generic", tool_name: "handoff_worktree", arguments_json: "{}" },
  }] });
  assert.match(handoff.html, /data-slot="trace-line"/);
  assert.match(handoff.html, /data-status="running"/);
  assert.doesNotMatch(handoff.html, /<button\b/);
});
