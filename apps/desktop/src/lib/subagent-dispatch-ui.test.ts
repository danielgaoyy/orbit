import assert from "node:assert/strict";
import { after, before, it } from "node:test";
import { Children, createElement, isValidElement, useState, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";

let server: ViteDevServer;
before(async () => {
  server = await createServer({
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [{
      name: "test-subagent-panel-actions",
      enforce: "pre",
      load(id) {
        if (id.endsWith("/state/orbit-store.ts")) return `
          export const opened = [];
          export const selected = [];
          export const orbitActions = {
            openSubagentTab: (record) => opened.push(record),
            selectAgent: (hostId, agentId) => selected.push({ hostId, agentId }),
          };
          export const state = {
            hostDetails: {
              local: {
                host: { host_id: "local" },
                agents: [
                  {
                    agent_id: "ag_router",
                    title: "Personal Router",
                    handle: "router",
                    labels: { "orbit.role": "personal_router" },
                  },
                  {
                    agent_id: "ag_backend",
                    title: "Backend Worker",
                    handle: "backend",
                    labels: {},
                  },
                  {
                    agent_id: "ag_worker_legacy",
                    title: "Legacy Worker",
                    parent_agent_id: "ag_router",
                    parent_host_id: "local",
                    labels: {},
                  },
                ],
              },
              remote: {
                host: {host_id: "remote"},
                agents: ["ag_review", "ag_Reviewer", "ag_Auditor"].map(agent_id => ({agent_id})),
              },
            },
            archivedThreads: {},
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

it("opens the subagent panel directly from overview portraits and quiet single-dispatch trace rows", async () => {
  const { SubagentCrewList } = await server.ssrLoadModule("/src/components/stage/subagent-crew-dock.tsx");
  const { DelegationMilestoneRow } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  const { opened } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const peer = { key: "managed:remote:ag_review", callId: "ag_review", title: "Reviewer",
    childHostId: "remote", childAgentId: "ag_review", providerSubagentId: null, isManaged: true,
    promptPreview: "Inspect changes", status: "awaiting_permission" };
  for (const status of ["awaiting_permission", "running", "completed"]) {
    const overview = renderRow(SubagentCrewList, { agentId: "parent", subagents: [{ ...peer, status }] });
    assert.equal(overview.clicks.length, 1);
    assert.match(overview.html, /data-slot="subagent-avatar"/);
    if (status === "awaiting_permission") assert.match(overview.html, /Approval required/);
    overview.clicks[0]();
    assert.equal(opened.at(-1).childAgentId, "ag_review");
  }

  const item = { seq: 1, item_type: "tool_call", call_id: "launch", status: "running",
    detail: { kind: "generic", tool_name: "Run mcp orbit create agent",
      arguments_json: '{"title":"Reviewer"}' } };
  for (const [rpcStatus, childStatus, expectedSuffix] of [
    ["running", null, ""],
    ["completed", null, ""],
    ["completed", "running", ""],
    ["completed", "awaiting_permission", "· Approval required"],
    ["running", "completed", ""],
    ["completed", "failed", "· Failed"],
    ["completed", "canceled", ""],
    ["denied", null, "· Failed"],
  ] as const) {
    const row = renderRow(DelegationMilestoneRow, {
      items: [{ ...item, status: rpcStatus }], hostId: "local", agentId: "parent",
      sessionSubagents: childStatus ? [{ ...peer, status: childStatus }] : [],
    });
    assert.equal(row.clicks.length, 1);
    assert.equal(row.html.replace(/<[^>]+>/g, "").trim(), "Dispatched Reviewer" + expectedSuffix);
    assert.equal(row.html.match(/<button\b/g)?.length, 1);
    assert.match(row.html, /data-slot="trace"/);
    assert.match(row.html, /data-slot="trace-line"/);
    assert.match(row.html, /data-slot="subagent-dispatch-trigger"/);
    assert.match(row.html, /min-h-\[28px\]/);
    assert.match(row.html, /group-focus-visible\/dispatch:opacity-100/);
    assert.doesNotMatch(row.html, /data-slot="subagent-avatar"|aria-expanded|bg-surface-raised|border-border-subtle/);
    const status = childStatus ?? (rpcStatus === "denied" ? "failed" : "running");
    assert.equal(row.html.includes("text-signal-warning"), status === "awaiting_permission");
    assert.equal(row.html.includes("text-signal-danger"), status === "failed");
    assert.equal(row.html.includes('data-slot="trace-description"'), status === "awaiting_permission" || status === "failed");
    assertIndependentControls(row.html);
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

it("coalesces consecutive dispatches into a quiet collapsible trace while keeping portraits in overview", async () => {
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
  const props = { items, sessionSubagents: [...sessionSubagents].reverse(), agentId: "parent" };

  // Overview HUD remains the sole source of animal portraits.
  const overview = renderRow(SubagentCrewList, { agentId: "parent", subagents: sessionSubagents });
  const overviewPortraits = Array.from(overview.html.matchAll(/data-character="([^"]+)"/g), (m) => m[1]);
  assert.equal(overviewPortraits.length, 2);

  // Collapsed timeline batch coalesces both dispatches with semantic-only awaiting suffix.
  const collapsed = renderRow(DelegationMilestoneRow, props);
  assert.doesNotMatch(collapsed.html, /data-slot="subagent-avatar"/);
  assert.match(collapsed.html, /data-slot="trace-collapsible-trigger"/);
  assert.match(collapsed.html, /aria-expanded="false"/);
  assert.equal(collapsed.html.replace(/<[^>]+>/g, "").trim(), "Dispatched Auditor, Reviewer· 1 awaiting approval");
  assert.doesNotMatch(collapsed.html, /Done|Working|Spawned subagent|waiting for response/);
  assertIndependentControls(collapsed.html);

  // Expanding reveals quiet clickable items that open each subagent directly.
  const expanded = renderRow(DelegationMilestoneRow, props, [0]);
  assert.match(expanded.html, /aria-expanded="true"/);
  assert.match(expanded.html, /data-slot="trace-collapsible-content"/);
  assert.doesNotMatch(expanded.html, /data-slot="subagent-avatar"/);
  assert.equal(expanded.clicks.length, 3); // 1 collapsible toggle + 2 child open buttons
  expanded.clicks[1]();
  assert.equal(opened.at(-1).childAgentId, "ag_Auditor");
  expanded.clicks[2]();
  assert.equal(opened.at(-1).childAgentId, "ag_Reviewer");

  // 3+ consecutive dispatches summarize by count and surface only semantic failure/awaiting cues.
  const trio = ["Auditor", "Reviewer", "Planner"].map((title, i) => ({
    key: "managed:" + title, callId: "ag_" + title, childAgentId: "ag_" + title,
    title, childHostId: "remote", createdAtMs: 60000 + i * 1000, isManaged: true,
    status: i === 0 ? "completed" : i === 1 ? "failed" : "canceled",
  }));
  const trioItems = trio.map((sa, seq) => ({
    item_type: "tool_call", seq: seq + 10, call_id: "trio-" + seq, status: "completed",
    timestamp_ms: sa.createdAtMs - 20, detail: { kind: "generic", tool_name: "create_agent", arguments_json: "{}" },
  }));
  const trioCollapsed = renderRow(DelegationMilestoneRow, { items: trioItems, sessionSubagents: trio, agentId: "parent" });
  assert.equal(trioCollapsed.html.replace(/<[^>]+>/g, "").trim(), "Dispatched 3 subagents· 1 failed");
  const trioExpanded = renderRow(DelegationMilestoneRow, { items: trioItems, sessionSubagents: trio, agentId: "parent" }, [0]);
  assert.match(trioExpanded.html, /Auditor/);
  assert.match(trioExpanded.html, /Reviewer.*· Failed/);
  assert.match(trioExpanded.html, /Planner.*· Stopped/);
  assert.doesNotMatch(trioExpanded.html, /data-slot="subagent-avatar"/);
});

it("never renders live elapsed clocks or duplicated nested grandchild trees in timeline dispatches", async () => {
  const { DelegationMilestoneRow } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  const { SubagentCrewList } = await server.ssrLoadModule("/src/components/stage/subagent-crew-dock.tsx");
  const { useLiveSecondClock } = await server.ssrLoadModule("/src/lib/live-clock.ts");
  const now = Number(renderToStaticMarkup(createElement(() => String(useLiveSecondClock(true)))));
  const launch = { item_type: "tool_call", seq: 1, call_id: "launch", status: "completed",
    timestamp_ms: now - 25 * 60_000, summary_title: "mcp_orbit_create_agent",
    detail: { kind: "subagent", role: "Reviewer", prompt_preview: "", child_agent_id: "ag_review" } };
  const child = { key: "managed:remote:ag_review", callId: "launch", title: "Reviewer",
    childHostId: "remote", childAgentId: "ag_review", isManaged: true, depth: 1,
    createdAtMs: launch.timestamp_ms, activeStartedAtMs: now - 10_000 };
  const grandchild = { key: "managed:remote:ag_nested", callId: "nested", title: "Deep Auditor",
    childHostId: "remote", childAgentId: "ag_nested", parentAgentId: "ag_review", isManaged: true,
    depth: 2, status: "running",
    lineage: [
      { host_id: "local", agent_id: "parent", title: "Parent" },
      { host_id: "remote", agent_id: "ag_review", title: "Reviewer" },
      { host_id: "remote", agent_id: "ag_nested", title: "Deep Auditor" },
    ] };
  for (const status of ["running", "awaiting_permission", "completed", "failed", "canceled"]) {
    const { html } = renderRow(DelegationMilestoneRow, {
      items: [launch], sessionSubagents: [{ ...child, status }, grandchild], hostId: "local", agentId: "parent",
    });
    assert.doesNotMatch(html, /· 2[45]m|· 10s/);
    assert.doesNotMatch(html, /Deep Auditor/);
    assert.doesNotMatch(html, /data-slot="subagent-avatar"/);
  }
  // Overview HUD still shows both the direct child and nested grandchild portraits.
  const overview = renderRow(SubagentCrewList, {
    agentId: "parent",
    subagents: [{ ...child, status: "running" }, grandchild],
  });
  assert.match(overview.html, /Reviewer/);
  assert.match(overview.html, /Deep Auditor/);
  assert.equal(overview.html.match(/data-slot="subagent-avatar"/g)?.length, 2);
});

it("uses one inline step rail and folds earlier steps above twelve without a bottom collapse button", async () => {
  const { TurnActivityDisclosure } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  for (const count of [1, 9, 12, 13, 50]) {
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
    const visibleSteps = count > 12 ? 8 : count;
    assert.equal(expanded.html.match(/data-slot="trace-line"/g)?.length, visibleSteps + 1);
    assert.equal(expanded.html.match(/data-slot="trace-collapsible-content-inner"/g)?.length, 1);
    assert.match(expanded.html, /data-slot="trace-collapsible-trigger"/);
    assertIndependentControls(expanded.html);
    assert.ok(expanded.html.includes("Step " + count + "</span>"));
    assert.doesNotMatch(expanded.html, /orbit-disclosure-(track|scroll)|orbit-sticky-disclosure-bar|lucide-(maximize|minimize)|max-h-\[340px\]|overflow-y-auto|Collapse \d+ steps/);
    if (count > 12) {
      const hidden = count - 8;
      assert.ok(expanded.html.includes(`Show ${hidden} earlier steps`));
      assert.ok(!expanded.html.includes("Step 1</span>"));
    } else {
      assert.doesNotMatch(expanded.html, /Show \d+ earlier step/);
      assert.ok(expanded.html.includes("Step 1</span>"));
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
  assert.match(html, /contain:layout[_ ]style/);
});

it("keeps incoming failure expansion separate from opening a provider-native sender and hides direct messages", async () => {
  const { AgentMessageCard } = await server.ssrLoadModule("/src/components/stage/subagents-track.tsx");
  const { opened } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const subagents = [{
    callId: "launch", title: "Reviewer", childAgentId: null,
    providerSubagentId: "native-review", childHostId: "local",
  }];
  // Direct inter-agent messages stay completely silent
  const directHtml = renderRow(AgentMessageCard, {
    hostId: "local", agentId: "parent", sessionSubagents: subagents,
    item: { kind: "direct_message", source_agent_id: "native-review", source_title: "Reviewer",
      text: "Review ready", blocks: [], seq: 2 },
  }).html;
  assert.equal(directHtml, "");

  // Failed finish alerts stay expandable with a separate open-sender control
  const props = { hostId: "local", agentId: "parent", sessionSubagents: subagents,
    item: { kind: "finish_notification", event: "failed", source_agent_id: "native-review", source_title: "Reviewer",
      text: "Review failed", blocks: [], seq: 2 } };
  const before = opened.length;
  const expanded = renderRow(AgentMessageCard, props, [0]);
  assert.match(expanded.html, /aria-expanded="true"/);
  assert.match(expanded.html, /<p[^>]*>Review failed<\/p>/);
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
    // Once resolved, permission callbacks stay completely silent in the main stream
    const settled = renderRow(AgentMessageCard, props);
    assert.equal(settled.html, "");
    assert.equal(settled.clicks.length, 0);
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

it("renders direct agent dispatches via shared UserPromptBubble with source provenance and hides routine finish/protocol noise", async () => {
  const { TurnStageSection } = await server.ssrLoadModule("/src/components/stage/stage-turn-card.tsx");
  const { UserPromptBubble } = await server.ssrLoadModule("/src/components/stage/stage-prompt-controls.tsx");
  const { selected } = await server.ssrLoadModule("/src/state/orbit-store.ts");
  const { translate } = await server.ssrLoadModule("/src/lib/i18n/index.ts");

  // 1. Routine finish notifications and pure fleet/protocol noise stay silent (no ghost footer or bubble)
  for (const opening of [
    {
      item_type: "agent_message", kind: "finish_notification", seq: 10, message_id: "fin-10",
      source_agent_id: "ag_review", source_title: "Reviewer", event: "finished", text: "Done", blocks: [],
    },
    {
      item_type: "agent_message", kind: "direct_message", seq: 11, message_id: "noise-11",
      source_agent_id: "ag_router", source_title: "Personal Router",
      text: '<orbit-fleet-delta rev="3">\n- worker1 idle\n</orbit-fleet-delta>', blocks: [],
    },
    {
      item_type: "agent_message", kind: "direct_message", seq: 12, message_id: "noise-12",
      source_agent_id: "ag_router", source_title: "Personal Router",
      text: '<orbit-system version="1" kind="agent-notification" event="finished">\nAgent finished.\n</orbit-system>',
      blocks: [],
    },
  ]) {
    const emptyTurn = {
      agent_message: opening,
      folded_steps: [], notices: [], compactions: [], turn_diffs: [],
      live_segments: [],
      completed_at_ms: 1_700_000_000_000,
    };
    const emptyHtml = renderRow(TurnStageSection, {
      hostId: "local", agentId: "worker", cwd: "/", hostArtifacts: [], turn: emptyTurn, isTurnActive: false,
    }).html;
    assert.equal(emptyHtml, "");
  }

  // 2. Failed finish alerts still render via AgentMessageCard danger banner (not UserPromptBubble)
  const failedFinish = {
    item_type: "agent_message", kind: "finish_notification", seq: 5, message_id: "finish-err",
    source_agent_id: "ag_review", source_title: "Reviewer", event: "errored", text: "Final notification", blocks: [],
  };
  const failedHtml = renderRow(TurnStageSection, {
    hostId: "local", agentId: "parent", cwd: "/", hostArtifacts: [], isTurnActive: false,
    turn: {
      folded_steps: [], notices: [], compactions: [], turn_diffs: [],
      live_segments: [{ kind: "agent_message", key: "finish", item: failedFinish }],
    },
  }).html;
  assert.doesNotMatch(failedHtml, /data-slot="subagent-avatar"|prompt-bubble-surface/);
  assert.match(failedHtml, />failed</i);
  assert.match(failedHtml, /border-signal-danger-border bg-signal-danger-bg\/45/);
  assert.match(failedHtml, /Final notification/);

  // 3. Direct message from Personal Agent (even wrapped in <orbit-system>) renders in global UserPromptBubble
  //    with localized source label and clickable jump to Personal Agent
  const personalDispatch = {
    item_type: "agent_message", kind: "direct_message", seq: 1, message_id: "dm-personal",
    source_agent_id: "ag_router", source_host_id: "local", source_title: "Personal Router",
    text: '<orbit-system version="1" kind="agent-message" source-agent-id="ag_router" source-host-id="local" source-agent-title="Personal Router" message-id="dm-personal">\nInvestigate &lt;Trace&gt; icon alignment.\n</orbit-system>',
    blocks: [],
  };
  const personalTurn = {
    turn_index: 0, anchor_seq: 1, agent_message: personalDispatch,
    folded_steps: [], notices: [], compactions: [], turn_diffs: [],
    live_segments: [],
  };
  const personalEn = renderRow(TurnStageSection, {
    hostId: "local", agentId: "ag_worker", cwd: "/", hostArtifacts: [], turn: personalTurn, isTurnActive: false,
  });
  assert.match(personalEn.html, /prompt-bubble-surface/);
  assert.match(personalEn.html, /data-slot="prompt-source"/);
  assert.match(personalEn.html, /From Personal Agent/);
  assert.match(personalEn.html, /Investigate &lt;Trace&gt; icon alignment\./);
  assert.doesNotMatch(personalEn.html, /orbit-system|Undo to this point/);

  // Clicking the source button navigates directly to the source agent session
  const beforeJump = selected.length;
  const bubbleRender = renderRow(UserPromptBubble, {
    hostId: "local", agentId: "ag_worker", cwd: "/",
    item: personalDispatch,
    holdViewport: (_a: unknown, apply: () => void) => apply(),
    onPreviewImage: () => {},
  });
  bubbleRender.clicks[0]();
  assert.equal(selected.length, beforeJump + 1);
  assert.deepEqual(selected.at(-1), { hostId: "local", agentId: "ag_router" });

  // Chinese & Japanese locales format localized Personal Agent source labels via i18n
  assert.equal(
    translate("zh-CN", "stage.fromSource", { source: translate("zh-CN", "sidebar.personalRouter") }),
    "来自 个人代理",
  );
  assert.equal(
    translate("en", "stage.fromSource", { source: translate("en", "sidebar.personalRouter") }),
    "From Personal Agent",
  );
  assert.equal(
    translate("ja", "stage.fromSource", { source: translate("ja", "sidebar.personalRouter") }),
    "パーソナルエージェント から",
  );

  // 4. Inline peer message (e.g. message_peer from @backend) renders via UserPromptBubble with @handle
  const peerMsg = {
    item_type: "agent_message", kind: "direct_message", seq: 20, message_id: "pm-1",
    source_agent_id: "ag_backend", source_host_id: "local", source_title: "@backend", event: "peer_message",
    text: "Updated OpenAPI schema for login.", blocks: [],
  };
  const peerTurn = {
    turn_index: 1, anchor_seq: 18,
    folded_steps: [], notices: [], compactions: [], turn_diffs: [],
    live_segments: [{ kind: "agent_message", key: "pm-1", item: peerMsg }],
  };
  const peerHtml = renderRow(TurnStageSection, {
    hostId: "local", agentId: "ag_frontend", cwd: "/", hostArtifacts: [], turn: peerTurn, isTurnActive: false,
  }).html;
  assert.match(peerHtml, /prompt-bubble-surface/);
  assert.match(peerHtml, /From @backend/);
  assert.match(peerHtml, /Updated OpenAPI schema for login\./);

  // 5. Historical create_agent worker (turn 0 persisted as UserPrompt with parent_agent_id=ag_router)
  //    automatically attributes turn 0 bubble to Personal Agent
  const legacyTurn = {
    turn_index: 0, anchor_seq: 1,
    user_prompt: {
      item_type: "user_prompt", seq: 1, message_id: "u-legacy",
      text: "Audit stage folding", badges: [], blocks: [],
    },
    folded_steps: [], notices: [], compactions: [], turn_diffs: [],
    live_segments: [],
  };
  const legacyHtml = renderRow(TurnStageSection, {
    hostId: "local", agentId: "ag_worker_legacy", cwd: "/", hostArtifacts: [], turn: legacyTurn, isTurnActive: false,
  }).html;
  assert.match(legacyHtml, /prompt-bubble-surface/);
  assert.match(legacyHtml, /From Personal Agent/);
  assert.match(legacyHtml, /Audit stage folding/);
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
  assert.match(live.html, /lucide-brain/);
  assert.match(live.html, /Thinking…/);
  assert.doesNotMatch(live.html, /Offline|queued|waiting for response|steps/i);
  assert.doesNotMatch(live.html, /<button\b|data-slot="trace-collapsible"/);
  assert.equal(renderRow(TurnActivityDisclosure, { ...props, isWorkActive: false }).html, "");

  const failed = {
    ...props, isWorkActive: false, settledLabel: "Ran a command", settledDurationMs: 3200,
    summaryCounts: { failed_count: 1, unrecovered_failure_count: 1 },
    items: [{ item_type: "tool_call", seq: 1, status: "failed", diff_files: [],
      detail: { kind: "shell", command: "pwd", output_preview: "failure output", exit_code: 1 } }],
  };
  const initial = renderRow(TurnActivityDisclosure, failed);
  assert.equal(initial.html.match(/aria-expanded="true"/g)?.length, 2);
  assert.match(initial.html, /failure output/);
  // Settled step groups omit per-group duration (moved to turn-level Worked for) and leading icons
  assert.doesNotMatch(initial.html, /data-slot="trace-description"[^>]*>· 3s</);
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

  // Long clickable file paths must truncate only the path button, never squeezing the action verb (`Read` / `已读取`)
  const longRead = renderRow(StepItemRow, {
    ...props,
    item: {
      ...tool,
      status: "completed",
      detail: {
        kind: "file_read",
        path: "/Users/jess/.gemini/antigravity-cli/brain/a46fa900-2ce2-438f-9c5a-0c7812b510d5/.user_uploaded/uploaded_media_0_1795624988351.jpg",
        start_line: null,
        end_line: null,
      },
    },
  });
  assert.match(longRead.html, /data-slot="trace-line"[^>]*class="[^"]*\bshrink-0\b/);
  assert.match(longRead.html, /data-slot="trace-title"[^>]*class="[^"]*\bshrink-0\b/);
  assert.match(longRead.html, /<button[^>]*class="[^"]*\bmin-w-0\b[^"]*\btruncate\b[^"]*"[^>]*>\/Users\/jess\/\.gemini\//);

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

it("renders thinking and failure icons consistently, wraps pure tool turns behind Worked for without double-nesting, and preserves important segments", async () => {
  const { TurnStageSection } = await server.ssrLoadModule("/src/components/stage/stage-turn-card.tsx");
  const { TurnActivityDisclosure } = await server.ssrLoadModule("/src/components/stage/stage-step-group.tsx");
  const { StepItemRow } = await server.ssrLoadModule("/src/components/stage/stage-tool-renderers.tsx");

  // 1. Live step group: dynamic tool icon (FilePenLine for running file_edit) + active label + dotless elapsed,
  //    drops step count / diff / stall / queued / thinking subtitle
  const liveGroup = renderRow(TurnActivityDisclosure, {
    items: [
      { item_type: "assistant_thinking", seq: 1, text: "Checking hidden headline" },
      { item_type: "tool_call", seq: 2, status: "running", summary_title: "Edit a.ts", diff_files: [], detail: { kind: "file_edit", path: "/a.ts" } },
    ],
    isWorkActive: true,
    liveLabel: "Editing a.ts…",
    liveDetail: "Checking hidden headline",
    settledLabel: "",
    summaryCounts: { action_steps: 1, thought_count: 1, additions: 12, deletions: 4, unrecovered_failure_count: 0 },
    labelIncludesFailure: false,
    defaultCollapsed: true,
    autoOpenForError: false,
    liveStartedAtMs: Date.now() - 5000,
    cwd: "/",
    agentId: "ag",
    thinkingSignature: "",
    getThinkingDurationMs: () => null,
    holdViewport: (_a: unknown, apply: () => void) => apply(),
  });
  assert.match(liveGroup.html, /lucide-file-pen-line/);
  assert.doesNotMatch(liveGroup.html, /lucide-list-checks|lucide-sparkles/);
  assert.match(liveGroup.html, /Editing a\.ts…/);
  // Elapsed renders without middle-dot separator
  assert.match(liveGroup.html, /data-slot="trace-description"[^>]*>5s</);
  assert.doesNotMatch(liveGroup.html, /· 5s|Checking hidden headline|2 steps|\+12|>-4<|Offline|queued/i);

  // 1b. Thought-only TurnActivityDisclosure: pulsing Brain while active, static Brain when settled ("Thought for 5s")
  const activeThought = renderRow(TurnActivityDisclosure, {
    items: [{ item_type: "assistant_thinking", seq: 1, item_id: "th1", text: "Reasoning..." }],
    isWorkActive: true,
    liveLabel: "Thinking…",
    settledLabel: "Thought about it",
    thoughtDurationMs: 5_000,
    summaryCounts: { action_steps: 0, thought_count: 1, unrecovered_failure_count: 0 },
    labelIncludesFailure: false,
    defaultCollapsed: true,
    autoOpenForError: false,
    liveStartedAtMs: Date.now() - 3000,
    cwd: "/",
    agentId: "ag-think-live",
    thinkingSignature: "",
    getThinkingDurationMs: () => 5_000,
    holdViewport: (_a: unknown, apply: () => void) => apply(),
  });
  assert.match(activeThought.html, /lucide-brain/);
  assert.match(activeThought.html, /animate-pulse/);
  assert.match(activeThought.html, /Thinking…/);

  const settledThought = renderRow(TurnActivityDisclosure, {
    items: [{ item_type: "assistant_thinking", seq: 1, item_id: "th1", text: "Reasoning..." }],
    isWorkActive: false,
    liveLabel: "Thinking…",
    settledLabel: "Thought about it",
    thoughtDurationMs: 5_000,
    summaryCounts: { action_steps: 0, thought_count: 1, unrecovered_failure_count: 0 },
    labelIncludesFailure: false,
    defaultCollapsed: true,
    autoOpenForError: false,
    liveStartedAtMs: null,
    cwd: "/",
    agentId: "ag-think-done",
    thinkingSignature: "",
    getThinkingDurationMs: () => 5_000,
    holdViewport: (_a: unknown, apply: () => void) => apply(),
  });
  assert.match(settledThought.html, /lucide-brain/);
  assert.doesNotMatch(settledThought.html, /animate-pulse/);
  assert.match(settledThought.html, /Thought for 5s/);
  assert.equal(settledThought.html.match(/data-slot="trace-icon"/g)?.length, 2);

  // 1c. Settled tool disclosure with unrecovered failure surfaces non-color AlertTriangle icon
  const settledFailed = renderRow(TurnActivityDisclosure, {
    items: [
      { item_type: "tool_call", seq: 1, status: "failed", summary_title: "Run test", diff_files: [], detail: { kind: "shell", command: "npm test" } },
    ],
    isWorkActive: false,
    liveLabel: "Running…",
    settledLabel: "Ran command",
    summaryCounts: { action_steps: 1, thought_count: 0, failed_count: 1, unrecovered_failure_count: 1 },
    labelIncludesFailure: false,
    defaultCollapsed: true,
    autoOpenForError: false,
    liveStartedAtMs: null,
    cwd: "/",
    agentId: "ag-failed",
    thinkingSignature: "",
    getThinkingDurationMs: () => null,
    holdViewport: (_a: unknown, apply: () => void) => apply(),
  });
  assert.match(settledFailed.html, /lucide-triangle-alert/);
  assert.match(settledFailed.html, /data-status="error"/);

  // 2. Inner thinking row: keeps restrained Brain icon (static when settled, pulsing when active)
  //    while still omitting truncated headline preview while collapsed
  const thinkingRow = renderRow(StepItemRow, {
    cwd: "/",
    thoughtDurationMs: 5000,
    holdViewport: (_a: unknown, apply: () => void) => apply(),
    item: { item_type: "assistant_thinking", seq: 1, item_id: "th1", text: "Secret preview sentence\nDetails" },
  });
  assert.match(thinkingRow.html, /Thought for 5s/);
  assert.match(thinkingRow.html, /lucide-brain/);
  assert.doesNotMatch(thinkingRow.html, /animate-pulse/);
  assert.doesNotMatch(thinkingRow.html, /Secret preview sentence|lucide-sparkles/);
  assert.equal(thinkingRow.html.match(/data-slot="trace-icon"/g)?.length, 2);

  const activeThinkingRow = renderRow(StepItemRow, {
    cwd: "/",
    isActivelyThinking: true,
    holdViewport: (_a: unknown, apply: () => void) => apply(),
    item: { item_type: "assistant_thinking", seq: 1, item_id: "th1", text: "Streaming thought" },
  });
  assert.match(activeThinkingRow.html, /Thinking…/);
  assert.match(activeThinkingRow.html, /lucide-brain/);
  assert.match(activeThinkingRow.html, /animate-pulse/);

  const mkMsg = (seq: number, text: string) => ({
    kind: "assistant_message",
    key: `msg-${seq}`,
    item: {
      item_type: "assistant_message",
      seq,
      item_id: `m${seq}`,
      text,
      streaming: false,
      blocks: [{ kind: "paragraph", spans: [{ text, bold: false, italic: false, strikethrough: false, inline_code: false, link_url: null }] }],
    },
  });

  // 3. Completed turn with pure pre-final tool work + final reply: folds behind "Worked for 18s",
  //    and expanding opens StepTimelineRail directly without a double-nested "Read files" disclosure
  const turn = {
    key: "turn-1",
    turn_index: 0,
    anchor_seq: 10,
    is_compaction_command: false,
    folded_steps: [],
    notices: [],
    compactions: [],
    turn_diffs: [],
    work_duration_ms: 18_000,
    turn_duration_ms: 18_000,
    has_unrecovered_error: false,
    live_segments: [
      {
        kind: "step_group",
        key: "sg-1",
        first_seq: 11,
        items: [
          { item_type: "tool_call", seq: 11, status: "completed", summary_title: "Read a", diff_files: [], detail: { kind: "file_read", path: "/repo/a.ts" } },
          { item_type: "tool_call", seq: 12, status: "completed", summary_title: "Read b", diff_files: [], detail: { kind: "file_read", path: "/repo/b.ts" } },
        ],
        summary: {
          counts: { action_steps: 2, read_file_count: 2, unrecovered_failure_count: 0, failed_count: 0, denied_count: 0, additions: 0, deletions: 0 },
          label: "Read files",
          live_label: "Reading b.ts…",
          live_detail: null,
          label_includes_failure: false,
        },
        has_following_content: true,
        has_running_step: false,
        work_duration_ms: 18_000,
      },
      mkMsg(13, "All done."),
    ],
  };
  const baseProps = {
    hostId: "local",
    cwd: "/repo",
    hostArtifacts: [],
    isTurnActive: false,
    autoFoldCompleted: true,
    isNetworkOffline: false,
    thinkingSignature: "",
    getThinkingDurationMs: () => null,
    holdViewport: (_a: unknown, apply: () => void) => apply(),
    onRequestRewind: () => {},
    onPreviewImage: () => {},
  };
  const collapsedTurn = renderRow(TurnStageSection, { ...baseProps, agentId: "ag-worked", turn });
  assert.match(collapsedTurn.html, /Worked for 18s/);
  assert.match(collapsedTurn.html, /All done\./);
  assert.doesNotMatch(collapsedTurn.html, /Read files/);
  // Trigger has trailing chevron trace-icon only (no leading prefix icon inside trace-line)
  assert.equal(collapsedTurn.html.match(/data-slot="trace-icon"/g)?.length, 1);

  // Expanding "Worked for 18s" reveals inner steps directly (single click, no nested "Read files" wrapper)
  const expandedTurn = renderRow(TurnStageSection, {
    ...baseProps,
    agentId: "ag-worked-open",
    turn,
    autoFoldCompleted: false,
  });
  assert.match(expandedTurn.html, /Worked for 18s/);
  assert.match(expandedTurn.html, /a\.ts/);
  assert.match(expandedTurn.html, /b\.ts/);
  assert.doesNotMatch(expandedTurn.html, /Read files|· 18s/);

  // Fast pure-work turn (< 1s / missing duration) uses natural fallback label instead of bare "Worked"
  const fastTurn = {
    ...turn,
    key: "turn-fast",
    work_duration_ms: null,
    turn_duration_ms: null,
    live_segments: [
      { ...turn.live_segments[0], work_duration_ms: null },
      turn.live_segments[1],
    ],
  };
  const fastCollapsed = renderRow(TurnStageSection, { ...baseProps, agentId: "ag-fast", turn: fastTurn });
  assert.match(fastCollapsed.html, /Read files/);
  assert.match(fastCollapsed.html, /All done\./);

  // 4. Antigravity multi-segment completed turn: intermediate assistant reply MUST stay visible
  //    top-level after turn ends (never swallowed behind "Worked for"), while tool groups stay folded
  const multiReplyTurn = {
    ...turn,
    key: "turn-multi-reply",
    live_segments: [
      turn.live_segments[0],
      mkMsg(13, "Detailed root-cause analysis stays visible."),
      {
        kind: "step_group",
        key: "sg-2",
        first_seq: 14,
        items: [
          { item_type: "tool_call", seq: 14, status: "completed", summary_title: "Run tests", diff_files: [], detail: { kind: "shell", command: "npm test" } },
        ],
        summary: {
          counts: { action_steps: 1, command_count: 1, unrecovered_failure_count: 0, failed_count: 0, denied_count: 0, additions: 0, deletions: 0 },
          label: "Ran command",
          live_label: "Running command…",
          live_detail: null,
          label_includes_failure: false,
        },
        has_following_content: true,
        has_running_step: false,
        work_duration_ms: 4_000,
      },
      mkMsg(15, "Verification passed."),
    ],
  };
  const multiReplyHtml = renderRow(TurnStageSection, {
    ...baseProps,
    agentId: "ag-multi-reply",
    turn: multiReplyTurn,
  }).html;
  assert.match(multiReplyHtml, /Detailed root-cause analysis stays visible\./);
  assert.match(multiReplyHtml, /Verification passed\./);
  assert.match(multiReplyHtml, /Read files/);
  assert.match(multiReplyHtml, /Ran command/);
  assert.doesNotMatch(multiReplyHtml, /Worked for 18s/);

  // 5. Antigravity post-turn wakeup, runtime warning notice, and promoted timer milestone stay visible
  const wakeupTurn = {
    ...turn,
    key: "turn-wakeup",
    live_segments: [
      turn.live_segments[0],
      mkMsg(13, "Primary task completed."),
      { kind: "wakeup", key: "wk-14", label: "Subagent finished" },
      mkMsg(15, "Follow-up summary."),
    ],
  };
  const wakeupHtml = renderRow(TurnStageSection, { ...baseProps, agentId: "ag-wakeup", turn: wakeupTurn }).html;
  assert.match(wakeupHtml, /Primary task completed\./);
  assert.match(wakeupHtml, /Subagent finished/);
  assert.match(wakeupHtml, /Follow-up summary\./);
  assert.doesNotMatch(wakeupHtml, /Worked for 18s/);

  const noticeTurn = {
    ...turn,
    key: "turn-notice",
    live_segments: [
      turn.live_segments[0],
      { kind: "notice", key: "n-13", item: { item_type: "notice", seq: 13, level: "warning", message: "API stream retried" } },
      mkMsg(14, "Recovered cleanly."),
    ],
  };
  const noticeHtml = renderRow(TurnStageSection, { ...baseProps, agentId: "ag-notice", turn: noticeTurn }).html;
  assert.match(noticeHtml, /API stream retried/);
  assert.match(noticeHtml, /Recovered cleanly\./);
  assert.doesNotMatch(noticeHtml, /Worked for 18s/);
});
