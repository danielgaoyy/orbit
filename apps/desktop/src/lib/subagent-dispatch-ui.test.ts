import assert from "node:assert/strict";
import { after, before, it } from "node:test";
import { Children, createElement, isValidElement, type ReactNode } from "react";
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
          export const useOrbitSelector = () => "light";
          export const getOrbitState = () => ({hostDetails: {
            remote: {host: {host_id: "remote"}, agents: ["ag_review", "ag_Reviewer", "ag_Auditor"].map(agent_id => ({agent_id}))}
          }});
        `;
      },
    }],
  });
});
after(async () => { await server?.close(); });

// Render the actual memo component with React's hook dispatcher, and exercise
// its button handlers without requiring a Tauri runtime or a simulated DOM.
function renderRow(component: { type: (props: object) => ReactNode }, props: object) {
  const clicks: Array<() => void> = [];
  function collectButtons(node: ReactNode) {
    Children.forEach(node, (child) => {
      if (!isValidElement<{ onClick?: () => void; children?: ReactNode }>(child)) return;
      if (child.type === "button" && child.props.onClick) clicks.push(child.props.onClick);
      collectButtons(child.props.children);
    });
  }
  function Row() {
    const tree = component.type(props);
    collectButtons(tree);
    return tree;
  }
  return { html: renderToStaticMarkup(createElement(Row)), clicks };
}

it("opens the subagent panel from overview, launch, and resolved message rows", async () => {
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
  for (const [rpcStatus, childStatus, action, statusLabel] of [
    ["running", null, "Creating subagent", "Starting…"],
    ["completed", null, "Spawned subagent", "Starting…"],
    ["completed", "running", "Spawned subagent", "Working…"],
    ["completed", "awaiting_permission", "Spawned subagent", "Approval required"],
    ["running", "completed", "Spawned subagent", "Done"],
    ["completed", "failed", "Spawned subagent", "Failed"],
    ["completed", "canceled", "Spawned subagent", "Stopped"],
    ["denied", null, "Subagent launch", "Failed"],
  ] as const) {
    const row = renderRow(DelegationMilestoneRow, {
      items: [{ ...item, status: rpcStatus }], hostId: "local", agentId: "parent",
      sessionSubagents: childStatus ? [{ ...peer, status: childStatus }] : [],
    });
    assert.equal(row.clicks.length, 1);
    assert.equal(row.html.replace(/<[^>]+>/g, "").trim(), action + "Reviewer" + statusLabel);
    assert.match(row.html, /data-slot="subagent-avatar"/);
    assert.ok(row.html.includes(`data-status="${childStatus ?? (rpcStatus === "denied" ? "failed" : "running")}"`));
    assert.doesNotMatch(row.html, /lucide-(check|x)\b/);
    row.clicks[0]();
    assert.equal(opened.at(-1).callId, "launch");
    assert.equal(opened.at(-1).childAgentId, childStatus ? "ag_review" : null);
    assert.equal(opened.at(-1).parentAgentId, "parent");
  }

  const message = { ...item, detail: { kind: "generic", tool_name: "send_message",
    arguments_json: JSON.stringify({ recipient: "ag_review", message: "Review ready" }) } };
  const row = renderRow(DelegationMilestoneRow, { items: [message], agentId: "parent", sessionSubagents: [peer] });
  assert.match(row.html, /Review ready/);
  row.clicks[0]();
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
  assert.equal(timeline.html.replace(/<[^>]+>/g, ""), "Spawned subagentAuditorDoneSpawned subagentReviewerApproval required");
  timeline.clicks.forEach((click, i) => {
    click();
    assert.equal(opened.at(-1).childAgentId, children[i].childAgentId);
  });
  assert.equal(timeline.clicks.length, 2);
});
