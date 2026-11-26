import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { GoalRunRecord } from "../../lib/orbit-ipc.ts";
import {
  GOAL_DISMISSED_STORAGE_KEY,
  GOAL_SUCCEEDED_FRESH_HANDOFF_MS,
  createDismissedGoalStore,
  createGoalShelfCache,
  reconcileGoalShelf,
} from "./composer-goal-shelf-model.ts";

function makeGoal(overrides: Partial<GoalRunRecord> = {}): GoalRunRecord {
  return {
    goal_id: "goal_1",
    agent_id: "ag_1",
    workspace_id: "",
    objective: "Ship release",
    criteria: [],
    verify_command: null,
    max_iterations: 25,
    current_iteration: 1,
    max_duration_secs: 0,
    status: "running",
    consecutive_identical_failures: 0,
    active_duration_secs: 14,
    started_at_ms: 1_000,
    updated_at_ms: 15_000,
    finished_at_ms: null,
    ...overrides,
  };
}

function makeMemoryStorage() {
  const map = new Map<string, string>();
  return {
    getItem(key: string) {
      return map.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      map.set(key, value);
    },
  };
}

describe("composer-goal-shelf lifecycle parity", () => {
  it("hides stale historical succeeded goals on fresh mount and never resurrects dismissed goals across reloads", () => {
    const storage = makeMemoryStorage();
    const store1 = createDismissedGoalStore(storage);
    const oldSucceeded = makeGoal({
      status: "succeeded",
      finished_at_ms: 10_000,
      updated_at_ms: 10_000,
    });
    const stale = reconcileGoalShelf(
      null,
      oldSucceeded,
      42,
      (id) => store1.has(id),
      10_000 + GOAL_SUCCEEDED_FRESH_HANDOFF_MS + 1,
    );
    assert.equal(stale?.dismissed, true);

    // Dismissing a freshly-succeeded goal persists across a brand-new store instance (reload).
    const freshSucceeded = makeGoal({
      goal_id: "goal_fresh",
      status: "succeeded",
      finished_at_ms: 20_000,
      updated_at_ms: 20_000,
    });
    const shown = reconcileGoalShelf(null, freshSucceeded, 42, (id) => store1.has(id), 22_000);
    assert.equal(shown?.dismissed, false);
    assert.equal(shown?.settledLive, true);

    store1.add("goal_fresh");
    assert.ok(storage.getItem(GOAL_DISMISSED_STORAGE_KEY)?.includes("goal_fresh"));

    const store2 = createDismissedGoalStore(storage);
    const afterReload = reconcileGoalShelf(null, freshSucceeded, 42, (id) => store2.has(id), 23_000);
    assert.equal(afterReload?.dismissed, true);
  });

  it("auto-hides live succeeded goal when a subsequent user turn begins, without false-hiding optimistic commit", () => {
    const neverDismissed = () => false;
    const running = makeGoal({ status: "running" });
    const p1 = reconcileGoalShelf(null, running, 10, neverDismissed, 15_000);
    assert.equal(p1?.observedActive, true);
    assert.equal(p1?.dismissed, false);

    // Transitions live to succeeded while optimistic turn (0) is settling -> commits to seq 12.
    const succeeded = makeGoal({ status: "succeeded", finished_at_ms: 16_000, updated_at_ms: 16_000 });
    const p2 = reconcileGoalShelf(p1, succeeded, 0, neverDismissed, 16_500);
    assert.equal(p2?.dismissed, false);
    assert.equal(p2?.settledLive, true);

    const p3 = reconcileGoalShelf(p2, succeeded, 12, neverDismissed, 16_800);
    assert.equal(p3?.dismissed, false);
    assert.equal(p3?.settledTurnAnchorSeq, 12);

    // User sends a new message -> optimistic turn 0 or newer seq 15 immediately hides completed banner.
    const p4 = reconcileGoalShelf(p3, succeeded, 15, neverDismissed, 17_000);
    assert.equal(p4?.dismissed, true);
    assert.equal(p4?.settledLive, false);
  });

  it("reappears cleanly when a brand-new goal starts on the same session and bounds cache size", () => {
    const cache = createGoalShelfCache(2);
    const g1Done = reconcileGoalShelf(
      { goalId: "goal_1", open: false, dismissed: true, observedActive: true, settledLive: false, settledTurnAnchorSeq: 12 },
      makeGoal({ goal_id: "goal_2", status: "running" }),
      15,
      (id) => id === "goal_1",
      20_000,
    );
    assert.equal(g1Done?.goalId, "goal_2");
    assert.equal(g1Done?.dismissed, false);
    assert.equal(g1Done?.observedActive, true);

    cache.save("a", g1Done);
    cache.save("b", g1Done);
    cache.save("c", g1Done);
    assert.equal(cache.get("a"), null);
    assert.ok(cache.get("b"));
    assert.ok(cache.get("c"));
  });
});
