import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  OUTLINE_AGGREGATION_THRESHOLD,
  OUTLINE_RECENT_INDIVIDUAL_COUNT,
  partitionOutlinePrompts,
  type OutlinePromptItem,
} from "./outline-rail.ts";

function makePrompts(count: number): OutlinePromptItem[] {
  return Array.from({ length: count }, (_, i) => ({
    seq: (i + 1) * 10,
    preview: `Prompt ${i + 1}`,
  }));
}

describe("partitionOutlinePrompts", () => {
  it("keeps up to 12 prompts as individual ticks with zero aggregation", () => {
    const prompts = makePrompts(OUTLINE_AGGREGATION_THRESHOLD);
    const res = partitionOutlinePrompts(prompts, 120);
    assert.equal(res.earlier.length, 0);
    assert.equal(res.recent.length, 12);
    assert.equal(res.isActiveInEarlier, false);
  });

  it("aggregates earlier prompts once count exceeds 12 and keeps latest 10 individual", () => {
    const prompts = makePrompts(48);
    const res = partitionOutlinePrompts(prompts, 480);
    assert.equal(res.earlier.length, 38);
    assert.equal(res.recent.length, OUTLINE_RECENT_INDIVIDUAL_COUNT);
    assert.equal(res.earlier[0]?.seq, 10);
    assert.equal(res.earlier[res.earlier.length - 1]?.seq, 380);
    assert.equal(res.recent[0]?.seq, 390);
    assert.equal(res.recent[res.recent.length - 1]?.seq, 480);
    assert.equal(res.isActiveInEarlier, false);
  });

  it("marks isActiveInEarlier true when activeSeq belongs to an aggregated prompt", () => {
    const prompts = makePrompts(25);
    const res = partitionOutlinePrompts(prompts, 50);
    assert.equal(res.earlier.length, 15);
    assert.equal(res.recent.length, 10);
    assert.equal(res.isActiveInEarlier, true);
  });
});
