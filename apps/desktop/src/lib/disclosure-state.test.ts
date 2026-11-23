import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { DisclosureStateStore } from "./disclosure-state.ts";

describe("DisclosureStateStore", () => {
  it("preserves manual expand across optimistic anchorSeq=0 -> confirmed anchorSeq transition", () => {
    const store = new DisclosureStateStore();
    store.set("ag_1", 2, 0, 0, true);
    assert.equal(store.get("ag_1", 2, 0, 0), true);

    // Daemon confirms turn with anchor_seq = 42 -> should still be expanded and promote anchorSeq
    assert.equal(store.get("ag_1", 2, 0, 42), true);
    assert.equal(store.get("ag_1", 2, 0, 42), true);

    // Later rewind creates a brand-new turn at index 2 with anchor_seq = 55 -> must NOT inherit old state
    assert.equal(store.get("ag_1", 2, 0, 55), null);
  });

  it("migrates overrides seamlessly when a new conversation transitions from opt_* to real agentId", () => {
    const store = new DisclosureStateStore();
    store.set("opt_req1", 0, 0, 0, true);
    store.migrateAgent("opt_req1", "ag_real_99");

    assert.equal(store.get("opt_req1", 0, 0, 0), null);
    assert.equal(store.get("ag_real_99", 0, 0, 12), true);
  });

  it("isolates multiple work chunks within the same turn and enforces bounded LRU eviction", () => {
    const store = new DisclosureStateStore();
    store.set("ag_1", 0, 0, 10, true);
    store.set("ag_1", 0, 1, 10, false);

    assert.equal(store.get("ag_1", 0, 0, 10), true);
    assert.equal(store.get("ag_1", 0, 1, 10), false);

    for (let i = 0; i < 450; i += 1) {
      store.set("ag_bulk", i, 0, i + 1, true);
    }
    assert.equal(store.size, 400);
    assert.equal(store.get("ag_1", 0, 0, 10), null);
    assert.equal(store.get("ag_bulk", 449, 0, 450), true);
  });
});
