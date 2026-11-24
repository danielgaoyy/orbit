import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  type ResolvedSubagentItem,
  filterVisibleSubagents,
  resolveToolCallSubagent,
  resolveDelegationDispatches,
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
