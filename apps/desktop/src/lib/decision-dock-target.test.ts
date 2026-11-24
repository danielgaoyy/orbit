import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  getFocusedDecisionTarget,
  setDecisionQueueStepper,
  setFocusedDecisionTarget,
  stepFocusedDecisionQueue,
} from "./timeline-coalesce.ts";

describe("DecisionDock focused target & queue stepper coordination", () => {
  it("preserves exact hostId, agentId, requestId, allowedDecisions, and onResolve for subagent permissions", () => {
    const resolved: string[] = [];
    setFocusedDecisionTarget({
      kind: "permission",
      queueKey: "subperm:remote-1::child-42::req-99",
      hostId: "remote-1",
      agentId: "child-42",
      requestId: "req-99",
      allowedDecisions: ["allow", "allow_always", "deny"],
      onResolve: (choice) => resolved.push(choice),
    });

    const target = getFocusedDecisionTarget();
    assert.ok(target && target.kind === "permission");
    assert.equal(target.hostId, "remote-1");
    assert.equal(target.agentId, "child-42");
    assert.equal(target.requestId, "req-99");
    assert.deepEqual(target.allowedDecisions, ["allow", "allow_always", "deny"]);

    target.onResolve?.("allow");
    assert.deepEqual(resolved, ["allow"]);

    setFocusedDecisionTarget(null);
    assert.equal(getFocusedDecisionTarget(), null);
  });

  it("routes queue stepper deltas only when a multi-item queue handler is registered", () => {
    setDecisionQueueStepper(null);
    assert.equal(stepFocusedDecisionQueue(1), false);

    const deltas: Array<-1 | 1> = [];
    setDecisionQueueStepper((d) => deltas.push(d));
    assert.equal(stepFocusedDecisionQueue(-1), true);
    assert.equal(stepFocusedDecisionQueue(1), true);
    assert.deepEqual(deltas, [-1, 1]);

    setDecisionQueueStepper(null);
    assert.equal(stepFocusedDecisionQueue(1), false);
  });
});
