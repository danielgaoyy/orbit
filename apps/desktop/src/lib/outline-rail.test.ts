import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  isActiveInEarlier,
  outlinePromptsEqual,
  partitionOutlinePrompts,
  resolveOutlinePrompts,
  type OutlinePromptItem,
} from "./outline-rail.ts";

function makePrompts(count: number): OutlinePromptItem[] {
  return Array.from({ length: count }, (_, i) => ({
    seq: (i + 1) * 10,
    preview: `Prompt ${i + 1}`,
  }));
}

describe("partitionOutlinePrompts", () => {
  for (const count of [0, 1, 17, 18, 50]) {
    it(`renders the latest individual anchors in a ${count}-prompt session`, () => {
      const prompts = makePrompts(count);
      const { earlier, recent, isActiveInEarlier } = partitionOutlinePrompts(prompts, count * 10);
      assert.deepEqual(earlier, prompts.slice(0, Math.max(0, count - 17)));
      assert.deepEqual(recent, prompts.slice(-17));
      assert.equal(isActiveInEarlier, false);
    });
  }

  it("recognizes earlier sequence numbers at both boundaries and skips gaps", () => {
    const prompts = makePrompts(50);
    const { earlier } = partitionOutlinePrompts(prompts);
    for (const seq of [10, 170, 330]) {
      assert.equal(partitionOutlinePrompts(prompts, seq).isActiveInEarlier, true);
    }
    for (const seq of [null, 0, 165, 340, 999]) {
      assert.equal(isActiveInEarlier(earlier, seq), false);
    }
    assert.equal(partitionOutlinePrompts(makePrompts(18), 10).isActiveInEarlier, true);
  });
});

describe("outlinePromptsEqual", () => {
  it("accepts identical references and copies, and detects middle edits and prepends", () => {
    const prompts = makePrompts(50);
    const copy = prompts.map((p) => ({ ...p }));
    assert.ok(outlinePromptsEqual(prompts, prompts));
    assert.ok(outlinePromptsEqual(prompts, copy));
    copy[20].preview = "edited";
    assert.equal(outlinePromptsEqual(prompts, copy), false);
    copy[20] = { ...prompts[20], seq: 211 };
    assert.equal(outlinePromptsEqual(prompts, copy), false);
    assert.equal(outlinePromptsEqual(prompts, [{ seq: 1, preview: "older" }, ...prompts]), false);
  });
});

describe("resolveOutlinePrompts", () => {
  it("preserves coreOutlinePrompts reference stability on the fast path", () => {
    const core = makePrompts(20);
    const loaded = core.slice(-5);
    const resolved = resolveOutlinePrompts(core, loaded, "Prompt");
    assert.strictEqual(resolved, core);
  });

  it("falls back to loaded turns when core outline_prompts is undefined or empty", () => {
    const loaded = makePrompts(4);
    assert.strictEqual(resolveOutlinePrompts(undefined, loaded, "Prompt"), loaded);
    assert.strictEqual(resolveOutlinePrompts([], loaded, "Prompt"), loaded);
  });

  it("merges confirmed loaded turns missing from coreOutlinePrompts without dropping earlier anchors", () => {
    const core = [
      { seq: 10, preview: "Prompt 1" },
      { seq: 20, preview: "Prompt 2" },
    ];
    const loaded = [
      { seq: 20, preview: "Prompt 2" },
      { seq: 30, preview: "Prompt 3" },
      { seq: 0, preview: "Optimistic draft" },
    ];
    assert.deepEqual(resolveOutlinePrompts(core, loaded, "Prompt"), [
      { seq: 10, preview: "Prompt 1" },
      { seq: 20, preview: "Prompt 2" },
      { seq: 30, preview: "Prompt 3" },
    ]);
  });

  it("fills blank previews and deduplicates optimistic seq=0 fallback anchors", () => {
    const core = [{ seq: 10, preview: "" }];
    assert.deepEqual(resolveOutlinePrompts(core, [], "Prompt"), [{ seq: 10, preview: "Prompt" }]);
    const optimistic = [
      { seq: 0, preview: "First optimistic" },
      { seq: 0, preview: "Duplicate optimistic" },
    ];
    assert.deepEqual(resolveOutlinePrompts(undefined, optimistic, "Prompt"), [
      { seq: 0, preview: "First optimistic" },
    ]);
  });
});
