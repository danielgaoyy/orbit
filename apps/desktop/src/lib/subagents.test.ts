import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  type ResolvedSubagentItem,
  filterVisibleSubagents,
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
