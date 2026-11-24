import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { AgentCardView, HostDetailView, SessionSubagentEntryView } from "./orbit-ipc.ts";
import {
  type ResolvedSubagentItem,
  filterVisibleSubagents,
  resolveAgentAncestorLineage,
  resolveToolCallSubagent,
  resolveDelegationDispatches,
  resolveSessionSubagents,
  isSubagentActiveStatus,
} from "./subagents.ts";

function makeSubagent(overrides: Partial<ResolvedSubagentItem>): ResolvedSubagentItem {
  return {
    key: "provider:sub-1",
    callId: "provider:sub-1",
    title: "Release Manager",
    promptPreview: "Monitor CI",
    status: "completed",
    isManaged: false,
    childHostId: "host-a",
    remoteHostName: null,
    childAgentId: null,
    childProvider: null,
    providerSubagentId: "sub-1",
    createdAtMs: 0,
    sortOrder: 100,
    ...overrides,
  };
}

describe("filterVisibleSubagents", () => {
  it("hides archived settled subagents while keeping unarchived settled subagents visible", () => {
    const archived = makeSubagent({
      key: "provider:sub-archived",
      callId: "call-archived",
      providerSubagentId: "sub-archived",
      status: "completed",
    });
    const visible = makeSubagent({
      key: "provider:sub-visible",
      callId: "call-visible",
      providerSubagentId: "sub-visible",
      status: "completed",
    });
    const archivedMap: Record<string, true> = {
      "host-a::parent-1::sub-archived": true,
    };

    const result = filterVisibleSubagents([archived, visible], archivedMap, "host-a", "parent-1");
    assert.deepEqual(
      result.map((item) => item.key),
      ["provider:sub-visible"],
    );
  });

  it("always shows running and awaiting_permission subagents even when previously archived", () => {
    const revivedRunning = makeSubagent({
      key: "provider:sub-revived",
      callId: "call-revived",
      providerSubagentId: "sub-revived",
      status: "running",
    });
    const revivedAwaiting = makeSubagent({
      key: "managed:host-a:child-9",
      callId: "call-9",
      isManaged: true,
      childAgentId: "child-9",
      providerSubagentId: null,
      status: "awaiting_permission",
    });
    const archivedFailed = makeSubagent({
      key: "provider:sub-failed",
      callId: "call-failed",
      providerSubagentId: "sub-failed",
      status: "failed",
    });

    const archivedMap: Record<string, true> = {
      "host-a::parent-1::sub-revived": true,
      "host-a::parent-1::call-revived": true,
      "host-a::parent-1::child-9": true,
      "host-a::parent-1::sub-failed": true,
    };

    assert.equal(isSubagentActiveStatus("running"), true);
    assert.equal(isSubagentActiveStatus("awaiting_permission"), true);
    assert.equal(isSubagentActiveStatus("completed"), false);
    assert.equal(isSubagentActiveStatus("failed"), false);
    assert.equal(isSubagentActiveStatus("canceled"), false);

    const result = filterVisibleSubagents(
      [revivedRunning, revivedAwaiting, archivedFailed],
      archivedMap,
      "host-a",
      "parent-1",
    );
    assert.deepEqual(
      result.map((item) => item.key),
      ["provider:sub-revived", "managed:host-a:child-9"],
    );
  });
});

describe("managed child lifecycle", () => {
  const created = Date.parse("2026-11-23T12:00:00Z");
  function card(state: string, overrides: Partial<AgentCardView> = {}): AgentCardView {
    return {
      agent_id: "ag_review", parent_agent_id: "parent", title: "Security Reviewer",
      status: { state }, status_label: "", is_running: false, is_awaiting_permission: false,
      created_at_rfc3339: new Date(created).toISOString(), latest_seq: 1,
      updated_at_rfc3339: new Date(created).toISOString(), ...overrides,
    } as AgentCardView;
  }
  const launch: SessionSubagentEntryView = {
    id: "launch", role: " @Security_REVIEWER ", prompt_preview: "Check permissions",
    status: "completed", child_agent_id: null, last_seq: 2,
  };
  function resolve(host: string, agents: AgentCardView[], insights: SessionSubagentEntryView[] = []) {
    return resolveSessionSubagents({
      hostId: host, agentId: "parent", fleetHosts: [], insightsSubagents: insights,
      hostDetails: { [host]: { host: { host_id: host }, agents } as HostDetailView },
    });
  }

  it("reconciles a completed launch and the projected card into one live child in either order", () => {
    const synthetic = { ...launch, id: "ag_review", child_agent_id: "ag_review", prompt_preview: "Working" };
    for (const insights of [[launch], [synthetic, launch], [launch, synthetic]]) {
      const result = resolve("local", [card("initializing")], insights);
      assert.deepEqual(result.map(({ key, callId, status, promptPreview }) => ({ key, callId, status, promptPreview })), [
        { key: "managed:local:ag_review", callId: "launch", status: "running", promptPreview: "Check permissions" },
      ]);
      for (const summary_title of ["Subagent · Security Reviewer", "Run mcp_orbit_create_agent"]) {
        const row = resolveToolCallSubagent({
          item_type: "tool_call", seq: 2, call_id: "launch", summary_title, status: "completed", diff_files: [],
          detail: { kind: "subagent", role: launch.role, prompt_preview: launch.prompt_preview, child_agent_id: null },
        }, result);
        assert.equal(row?.linked, true);
        assert.equal(row.subagent.childAgentId, "ag_review");
        assert.equal(row.subagent.status, "running");
      }
    }
  });

  it("keeps explicit identities reserved and rejects generic or ambiguous title-only matches", () => {
    const reserved = { ...launch, id: "exact", child_agent_id: "ag_review" };
    for (const insights of [[launch, reserved], [reserved, launch]]) {
      const items = resolve("local", [card("running")], insights);
      assert.equal(items.find((sa) => sa.isManaged)?.callId, "exact");
      assert.equal(items.find((sa) => sa.callId === "launch")?.isManaged, false);
    }
    for (const [agents, insights] of [
      [[card("running"), card("idle", { agent_id: "ag_other" })], [launch]],
      [[card("running")], [launch, { ...launch, id: "second-launch" }]],
      [[card("running", { title: "Worker" })], [{ ...launch, role: "worker" }]],
      [[card("running")], [{ ...launch, provider_subagent_id: "native" }]],
      [[card("running")], [{ ...launch, status: "failed" }]],
    ] as const) {
      assert.equal(resolve("local", [...agents], [...insights]).find((sa) => sa.callId === "launch")?.isManaged, false);
    }
  });

  it("releases a title-inferred call ID when a separate timeline launch explicitly identifies that child", () => {
    const roster = resolve("conflict", [card("running"), card("running", { agent_id: "ag_other", title: "Renamed" })], [launch]);
    const inferred = { item_type: "tool_call" as const, seq: 2, call_id: launch.id,
      status: "completed" as const, timestamp_ms: created, summary_title: "create_agent", diff_files: [],
      detail: { kind: "subagent" as const, role: launch.role, child_agent_id: null, prompt_preview: "Review" } };
    const exact = { ...inferred, call_id: "later", seq: 3,
      detail: { kind: "generic" as const, tool_name: "create_agent", arguments_json: '{"agent_id":"ag_review"}' } };
    for (const items of [[inferred, exact], [exact, inferred]]) {
      const matches = resolveDelegationDispatches(items, roster);
      assert.equal(matches.get(exact)?.subagent.childAgentId, "ag_review");
      assert.equal(matches.get(inferred)?.subagent.childAgentId, "ag_other");
    }
  });

  it("uses the structured state and retains lifecycle flags and legacy labels", () => {
    for (const [state, expected] of [
      ["initializing", "running"], ["running", "running"], ["autonomous_wakeup", "running"],
      ["awaiting_permission", "awaiting_permission"], ["error", "failed"], ["closed", "canceled"], ["idle", "completed"],
    ]) assert.equal(resolve("states", [card(state)])[0].status, expected, state);
    for (const [overrides, expected] of [
      [{ is_running: true, is_awaiting_permission: true }, "awaiting_permission"],
      [{ is_running: true, has_background_tasks: true }, "completed"],
      [{ has_background_tasks: true }, "completed"],
      [{ is_running: true }, "running"],
      [{ status_label: "Closed" }, "canceled"], [{ status_label: "Stopped" }, "canceled"],
      [{ status_label: "Interrupted" }, "canceled"], [{ status_label: "Canceled" }, "canceled"],
      [{ status_label: "Launch failed" }, "failed"], [{ status_label: "Error" }, "failed"],
    ] as const) assert.equal(resolve("legacy", [card("", overrides)])[0].status, expected);
    assert.equal(resolve("states", [card("running", { archived_at_rfc3339: new Date(created).toISOString() })])[0].status, "running");
  });

  it("resets each observed restart once and preserves the clock across tokens, permission waits and snapshot replays", (t) => {
    let now = created + 25 * 60_000;
    t.mock.method(Date, "now", () => now);
    const snapshot = (state: string) => [card(state, { updated_at_rfc3339: new Date(now).toISOString() })];
    const first = snapshot("initializing");
    const start = (agents: AgentCardView[]) => resolve("elapsed", agents)[0].activeStartedAtMs;
    assert.equal(start(first), created);
    assert.equal(start(snapshot("running")), created);
    for (const state of ["idle", "closed", "error"]) {
      assert.equal(start(snapshot(state)), null);
      const restarted = now += 5000;
      assert.equal(start(snapshot("running")), restarted);
      now += 15_000;
      assert.equal(start(snapshot("awaiting_permission")), restarted);
      assert.equal(start(first), created);
      assert.equal(start(snapshot("running")), restarted);
      assert.equal(resolve("other-host", snapshot("running"))[0].activeStartedAtMs, created);
    }
  });
});

describe("Orbit managed dispatch resolution", () => {
  const reviewer = makeSubagent({
    title: "Security Reviewer", handle: "security", childAgentId: "ag_review",
    childHostId: "host-b", callId: "ag_review", isManaged: true, providerSubagentId: null,
    sortOrder: 1764000000000,
  });
  const call = (name: string, args = {}, output?: string) => ({
    item_type: "tool_call" as const, seq: 7, call_id: "launch", status: "completed" as const,
    timestamp_ms: reviewer.sortOrder, summary_title: name, output_preview: output, diff_files: [],
    detail: { kind: "generic" as const, tool_name: name, arguments_json: JSON.stringify(args) },
  });

  it("recognizes bare, MCP-prefixed, and space-normalized historical names", () => {
    for (const name of ["create_agent", "Run mcp_orbit_create_agent", "Run mcp orbit create agent",
      "mcp__orbit__create_agent", "spawn_workspace_peer", "Run mcp orbit spawn workspace peer",
      "mcp_orbit_spawn_workspace_peer", "mcp__orbit__spawn_workspace_peer", "orbit/create_agent"]) {
      const result = resolveToolCallSubagent(call(name, { handle: "@security" }), [reviewer]);
      assert.equal(result?.action, "dispatch");
      assert.equal(result.subagent.childAgentId, reviewer.childAgentId);
      assert.equal(result.subagent.callId, "launch");
    }
  });

  it("prefers output identity to conflicting names and never resolves a managed ID as native", () => {
    const native = makeSubagent({ providerSubagentId: "ag_review" });
    const output = JSON.stringify({ content: [{ text: 'Created {"agent_id":"ag_review"}' }] });
    const item = call("mcp_orbit_create_agent", { title: native.title }, output);
    const result = resolveToolCallSubagent(item, [native, reviewer]);
    assert.equal(result?.subagent.childHostId, "host-b");
    assert.equal(result?.subagent.providerSubagentId, null);
    const pending = resolveToolCallSubagent(item, [native]);
    assert.equal(pending?.subagent.childAgentId, "ag_review");
    assert.equal(pending?.subagent.providerSubagentId, null);
  });

  it("uses normalized output titles or a sole unclaimed child with an exact timestamp", () => {
    const item = call("Run mcp orbit create agent", {}, 'Spawned {"title":"Security_Reviewer"}');
    assert.equal(resolveToolCallSubagent(item, [reviewer])?.subagent.childAgentId, "ag_review");
    const bare = call("spawn_workspace_peer");
    assert.equal(resolveToolCallSubagent(bare, [reviewer])?.subagent.childAgentId, "ag_review");
    const other = { ...reviewer, childAgentId: "ag_other", callId: "ag_other" };
    for (const candidates of [[other, reviewer], [{ ...reviewer, sortOrder: reviewer.sortOrder + 1 }],
      [{ ...reviewer, callId: "another-launch" }]]) {
      assert.equal(resolveToolCallSubagent(bare, candidates)?.subagent.childAgentId, null);
    }
    assert.equal(resolveToolCallSubagent(item, [other, reviewer])?.subagent.childAgentId, null);
  });
  it("keeps async launches active until a card arrives, including structured managed calls", () => {
    const generic = call("create_agent");
    const structured = { ...generic, detail: { kind: "subagent" as const,
      child_agent_id: "ag_pending", role: "Researcher", prompt_preview: "" } };
    for (const item of [generic, structured]) {
      for (const status of ["running", "completed", "failed", "denied"] as const) {
        assert.equal(resolveToolCallSubagent({ ...item, status }, [])?.subagent.status,
          status === "failed" || status === "denied" ? "failed" : "running");
      }
    }
  });

  it("bypasses a same-call ghost for a managed launch with no child ID, then links the arriving card by time", () => {
    const ghost = makeSubagent({ key: "insight:launch", callId: "launch", title: "Worker", providerSubagentId: null });
    const live = { ...reviewer, key: "managed:host-b:ag_review", createdAtMs: reviewer.sortOrder + 500, status: "running" as const };
    for (const name of ["Run mcp_orbit_create_agent", "mcp__orbit__spawn_workspace_peer"]) {
      const item = { ...call(name), detail: { kind: "subagent" as const, role: "Worker", prompt_preview: "Review", child_agent_id: null } };
      const pending = resolveToolCallSubagent(item, [ghost]);
      assert.equal(pending?.linked, false);
      assert.equal(pending.subagent.status, "running");
      for (const cards of [[live, ghost], [ghost, live]]) {
        const matched = resolveToolCallSubagent(item, cards);
        assert.equal(matched?.linked, true);
        assert.equal(matched.subagent.childAgentId, live.childAgentId);
        assert.equal(matched.subagent.status, "running");
      }
    }
  });

  it("reserves exact matches first, then pairs anonymous calls by global minimum delta", () => {
    const t = reviewer.sortOrder;
    const early = { ...call("create_agent"), call_id: "early", seq: 1 };
    const later = { ...call("spawn_workspace_peer"), timestamp_ms: t + 2000, seq: 2, call_id: "later" };
    const exact = { ...call("create_agent", { handle: "@security" }), call_id: "exact", seq: 3 };
    const reserved = { ...reviewer, key: "managed:review", createdAtMs: t };
    const makeChild = (id: string, title: string, createdAtMs: number, status: ResolvedSubagentItem["status"]) =>
      ({ ...reviewer, key: "managed:" + id, childAgentId: id, callId: id, handle: null,
        createdAtMs, title, status, sortOrder: t + 100000 });
    const nearest = makeChild("ag_near", "Build watcher", t + 1900, "awaiting_permission");
    const remaining = makeChild("ag_far", "Test runner", t + 8000, "running");
    for (const items of [[early, later, exact], [exact, later, early]]) {
      for (const cards of [[reserved, nearest, remaining], [remaining, nearest, reserved]]) {
        const results = resolveDelegationDispatches(items, cards);
        for (const [item, card] of [[exact, reserved], [early, remaining], [later, nearest]] as const) {
          assert.deepEqual(results.get(item)?.subagent, { ...card, callId: item.call_id });
        }
      }
    }
  });

  it("keeps a hard identity reserved when another launch requests the same name", () => {
    const named = call("create_agent", { title: reviewer.title });
    const exact = { ...call("create_agent", { agent_id: reviewer.childAgentId }), call_id: "with-id" };
    const renamed = { ...reviewer, key: "managed:renamed", childAgentId: "ag_renamed",
      callId: "ag_renamed", createdAtMs: named.timestamp_ms + 50, title: "Final name" };
    const result = resolveDelegationDispatches([named, exact], [reviewer, renamed]);
    assert.equal(result.get(named)?.subagent.key, renamed.key);
    assert.equal(result.get(exact)?.subagent.key, reviewer.key);
  });

  it("honors inclusive time bounds, explicit conflicting IDs, failed attempts and ambiguous legacy clocks", () => {
    const item = call("create_agent");
    for (const delta of [-5001, -5000, 30000, 30001]) {
      const card = { ...reviewer, createdAtMs: item.timestamp_ms + delta };
      assert.equal(resolveToolCallSubagent(item, [card])?.linked, delta >= -5000 && delta <= 30000);
    }
    const card = { ...reviewer, createdAtMs: item.timestamp_ms };
    assert.equal(resolveToolCallSubagent(call("create_agent", {agent_id: "ag_missing"}), [card])?.linked, false);
    assert.equal(resolveToolCallSubagent({ ...item, status: "failed" }, [card])?.linked, false);
    const second = { ...item, call_id: "another", seq: 8 };
    const legacy = resolveDelegationDispatches([item, second], [reviewer]);
    assert.equal(legacy.get(item)?.linked, false);
    assert.equal(legacy.get(second)?.linked, false);
  });

});

describe("multi-level subagent lineage", () => {
  function makeCard(
    id: string,
    title: string,
    parentAgentId: string | null,
    parentHostId: string | null = null,
    running = true,
  ): AgentCardView {
    return {
      agent_id: id,
      parent_agent_id: parentAgentId,
      parent_host_id: parentHostId,
      title,
      status_label: running ? "Running" : "Idle",
      is_running: running,
      is_awaiting_permission: false,
      created_at_rfc3339: "2026-11-24T10:00:00Z",
      updated_at_rfc3339: "2026-11-24T10:00:05Z",
      latest_seq: 10,
      provider: "codex",
    } as AgentCardView;
  }

  it("collects Parent -> Child -> Grandchild across hosts in DFS pre-order and resolves breadcrumbs", () => {
    const hostDetails: Record<string, HostDetailView> = {
      "host-mac": {
        host: { host_id: "host-mac", display_name: "Mac Studio" },
        agents: [
          makeCard("ag_root", "Coordinator", null),
          makeCard("ag_child", "Backend Lead", "ag_root", "host-mac"),
        ],
        provider_subagents: {},
      } as unknown as HostDetailView,
      "host-linux": {
        host: { host_id: "host-linux", display_name: "Linux GPU" },
        agents: [
          makeCard("ag_grand", "Kernel Profiler", "ag_child", "host-mac"),
        ],
        provider_subagents: {},
      } as unknown as HostDetailView,
    };

    const resolved = resolveSessionSubagents({
      hostId: "host-mac",
      agentId: "ag_root",
      insightsSubagents: [],
      hostDetails,
      fleetHosts: [],
    });

    assert.deepEqual(
      resolved.map((sa) => ({ id: sa.childAgentId, depth: sa.depth, remote: sa.remoteHostName })),
      [
        { id: "ag_child", depth: 1, remote: null },
        { id: "ag_grand", depth: 2, remote: "Linux GPU" },
      ],
    );
    assert.deepEqual(
      resolved[1].lineage?.map((step) => step.title),
      ["Coordinator", "Backend Lead", "Kernel Profiler"],
    );

    const crumbs = resolveAgentAncestorLineage({
      hostId: "host-linux",
      agentId: "ag_grand",
      hostDetails,
      fleetHosts: [],
    });
    assert.deepEqual(
      crumbs.map((c) => `${c.host_name}:${c.title}`),
      ["Mac Studio:Coordinator", "Mac Studio:Backend Lead", "Linux GPU:Kernel Profiler"],
    );
  });

  it("breaks cycles safely and caps depth at 8", () => {
    const agents: AgentCardView[] = [makeCard("ag_0", "Root", "ag_10")];
    for (let i = 1; i <= 10; i += 1) {
      agents.push(makeCard(`ag_${i}`, `Node ${i}`, `ag_${i - 1}`));
    }
    const hostDetails: Record<string, HostDetailView> = {
      "host-a": {
        host: { host_id: "host-a", display_name: "Host A" },
        agents,
        provider_subagents: {},
      } as unknown as HostDetailView,
    };

    const resolved = resolveSessionSubagents({
      hostId: "host-a",
      agentId: "ag_0",
      insightsSubagents: [],
      hostDetails,
      fleetHosts: [],
    });
    assert.equal(resolved.length, 8);
    assert.equal(resolved[resolved.length - 1]?.depth, 8);
  });
});
