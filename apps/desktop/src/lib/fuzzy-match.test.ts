import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  DEFAULT_MAX_PALETTE_RECENTS,
  PALETTE_RECENTS_STORAGE_KEY,
  type StorageLike,
  fuzzyMatch,
  getPaletteRecencyBonus,
  loadPaletteRecents,
  recordPaletteRecent,
  splitHighlightSegments,
} from "./fuzzy-match.ts";

function createMemoryStorage(initial: Record<string, string> = {}): StorageLike & {
  store: Map<string, string>;
} {
  const store = new Map<string, string>(Object.entries(initial));
  return {
    store,
    getItem(key) {
      return store.get(key) ?? null;
    },
    setItem(key, value) {
      store.set(key, value);
    },
    removeItem(key) {
      store.delete(key);
    },
  };
}

describe("fuzzyMatch scoring & alignment", () => {
  it("ranks exact > prefix > word boundary > mid-word > scattered subsequence", () => {
    const exact = fuzzyMatch("set", "Set");
    const prefix = fuzzyMatch("set", "Settings");
    const wordBoundary = fuzzyMatch("set", "Open Settings");
    const midWord = fuzzyMatch("set", "Resetting");
    const scattered = fuzzyMatch("set", "s_e_t");

    assert.ok(exact && prefix && wordBoundary && midWord && scattered);
    assert.ok(exact.score > prefix.score, `exact (${exact.score}) > prefix (${prefix.score})`);
    assert.ok(
      prefix.score > wordBoundary.score,
      `prefix (${prefix.score}) > wordBoundary (${wordBoundary.score})`,
    );
    assert.ok(
      wordBoundary.score > midWord.score,
      `wordBoundary (${wordBoundary.score}) > midWord (${midWord.score})`,
    );
    assert.ok(
      midWord.score > scattered.score,
      `midWord (${midWord.score}) > scattered (${scattered.score})`,
    );
  });

  it("rewards camelCase and word/path boundaries and chooses optimal DP alignment over greedy hits", () => {
    const camel = fuzzyMatch("fm", "fuzzyMatch");
    const snake = fuzzyMatch("fm", "file_manager");
    const mid = fuzzyMatch("fm", "aftermath");

    assert.ok(camel && snake && mid);
    assert.deepEqual(camel.indices, [0, 5]);
    assert.deepEqual(snake.indices, [0, 5]);
    assert.ok(camel.score > mid.score, `camel (${camel.score}) > mid (${mid.score})`);
    assert.ok(snake.score > mid.score, `snake (${snake.score}) > mid (${mid.score})`);

    // Optimal alignment picks 'c' and 'p' at command-palette word boundaries rather than 'components'
    const pathHit = fuzzyMatch("cp", "app/components/command-palette.tsx");
    assert.ok(pathHit);
    assert.deepEqual(pathHit.indices, [15, 23]);

    // Tighter span beats wide span with identical boundary types
    const tight = fuzzyMatch("oc", "openChat");
    const wide = fuzzyMatch("oc", "openVeryLongWorkspaceConfig");
    assert.ok(tight && wide);
    assert.ok(tight.score > wide.score, `tight (${tight.score}) > wide (${wide.score})`);
  });

  it("handles CJK queries cleanly across prefix, contiguous substring, scattered, and multi-token inputs", () => {
    const zhPrefix = fuzzyMatch("设置", "设置：主机与配对");
    const zhContig = fuzzyMatch("设置", "打开设置");
    const zhScattered = fuzzyMatch("设置", "设一个置");

    assert.ok(zhPrefix && zhContig && zhScattered);
    assert.deepEqual(zhPrefix.indices, [0, 1]);
    assert.deepEqual(zhContig.indices, [2, 3]);
    assert.deepEqual(zhScattered.indices, [0, 3]);
    assert.ok(
      zhPrefix.score > zhContig.score,
      `CJK prefix (${zhPrefix.score}) > contiguous (${zhContig.score})`,
    );
    assert.ok(
      zhContig.score > zhScattered.score,
      `CJK contiguous (${zhContig.score}) > scattered (${zhScattered.score})`,
    );

    // Multi-token Chinese query across punctuation separator
    const zhMulti = fuzzyMatch("设置 代理", "设置：模型、提供商与 ACP 代理");
    assert.ok(zhMulti);
    assert.deepEqual(
      zhMulti.indices.map((i) => "设置：模型、提供商与 ACP 代理"[i]).join(""),
      "设置代理",
    );

    // Japanese Hiragana / Katakana / Kanji
    const jaHit = fuzzyMatch("設定", "設定を開く");
    assert.ok(jaHit);
    assert.deepEqual(jaHit.indices, [0, 1]);
    assert.equal(fuzzyMatch("存在しない", "設定を開く"), null);
  });

  it("supports multi-token queries in-order and out-of-order with AND semantics", () => {
    const inOrder = fuzzyMatch("open set", "Open Settings");
    const outOfOrder = fuzzyMatch("set open", "Open Settings");
    const missingToken = fuzzyMatch("open missing", "Open Settings");

    assert.ok(inOrder && outOfOrder);
    assert.equal(missingToken, null);
    assert.ok(
      inOrder.score > outOfOrder.score,
      `in-order (${inOrder.score}) > out-of-order (${outOfOrder.score})`,
    );
    assert.deepEqual(inOrder.indices, [0, 1, 2, 3, 5, 6, 7]);
    assert.deepEqual(outOfOrder.indices, [0, 1, 2, 3, 5, 6, 7]);
  });

  it("handles edge cases safely without throwing", () => {
    assert.deepEqual(fuzzyMatch("", "Anything"), { score: 0, indices: [] });
    assert.deepEqual(fuzzyMatch("   ", "Anything"), { score: 0, indices: [] });
    assert.equal(fuzzyMatch("abc", ""), null);
    assert.equal(fuzzyMatch("abcdef", "abc"), null);
    assert.equal(fuzzyMatch("xyz", "abc"), null);

    // Special regex characters must be matched literally
    const regexChars = fuzzyMatch("(/btw)", "Ask Side Question (/btw)");
    assert.ok(regexChars);
    assert.deepEqual(regexChars.indices, [18, 19, 20, 21, 22, 23]);

    // Very long target (> 512 chars) falls back smoothly
    const longTarget = `${"a".repeat(600)}orbit-target`;
    const longHit = fuzzyMatch("orbit", longTarget);
    assert.ok(longHit);
    assert.equal(longHit.indices.length, 5);
  });

  it("merges consecutive match indices into highlight segments accurately", () => {
    const target = "Open Settings";
    const match = fuzzyMatch("oset", target);
    assert.ok(match);
    assert.deepEqual(match.indices, [0, 5, 6, 7]);

    const segments = splitHighlightSegments(target, match.indices);
    assert.deepEqual(segments, [
      { text: "O", matched: true },
      { text: "pen ", matched: false },
      { text: "Set", matched: true },
      { text: "tings", matched: false },
    ]);

    assert.deepEqual(splitHighlightSegments("Plain", []), [{ text: "Plain", matched: false }]);
    assert.deepEqual(splitHighlightSegments("", [0]), []);
  });
});

describe("palette LRU recents persistence", () => {
  it("maintains LRU ordering, deduplicates, and enforces cap", () => {
    const storage = createMemoryStorage();
    assert.deepEqual(loadPaletteRecents(storage), []);

    recordPaletteRecent("cmd-a", storage, 3);
    recordPaletteRecent("cmd-b", storage, 3);
    recordPaletteRecent("cmd-c", storage, 3);
    assert.deepEqual(loadPaletteRecents(storage, 3), ["cmd-c", "cmd-b", "cmd-a"]);

    // Re-executing an existing ID moves it back to the front without duplicating
    recordPaletteRecent("cmd-a", storage, 3);
    assert.deepEqual(loadPaletteRecents(storage, 3), ["cmd-a", "cmd-c", "cmd-b"]);

    // Adding beyond cap evicts the least recently used entry
    recordPaletteRecent("cmd-d", storage, 3);
    assert.deepEqual(loadPaletteRecents(storage, 3), ["cmd-d", "cmd-a", "cmd-c"]);

    // Persisted under the exact canonical storage key
    const raw = storage.store.get(PALETTE_RECENTS_STORAGE_KEY);
    assert.ok(raw);
    assert.deepEqual(JSON.parse(raw), ["cmd-d", "cmd-a", "cmd-c"]);
  });

  it("ignores blank IDs, respects default cap of 24, and survives corrupt or throwing storage", () => {
    const storage = createMemoryStorage({
      [PALETTE_RECENTS_STORAGE_KEY]: "{not-valid-json",
    });
    assert.deepEqual(loadPaletteRecents(storage), []);

    storage.setItem(PALETTE_RECENTS_STORAGE_KEY, JSON.stringify(["ok", "", 42, "ok", "second"]));
    assert.deepEqual(loadPaletteRecents(storage), ["ok", "second"]);

    for (let i = 0; i < 30; i += 1) {
      recordPaletteRecent(`item-${i}`, storage);
    }
    const capped = loadPaletteRecents(storage);
    assert.equal(capped.length, DEFAULT_MAX_PALETTE_RECENTS);
    assert.equal(capped[0], "item-29");

    const throwingStorage: StorageLike = {
      getItem() {
        throw new Error("denied");
      },
      setItem() {
        throw new Error("quota");
      },
    };
    assert.deepEqual(loadPaletteRecents(throwingStorage), []);
    assert.deepEqual(recordPaletteRecent("safe-id", throwingStorage), ["safe-id"]);
  });

  it("computes subtle monotonically decreasing recency bonuses", () => {
    const recents = ["first", "second", "third", "fourth"];
    const b0 = getPaletteRecencyBonus("first", recents);
    const b1 = getPaletteRecencyBonus("second", recents);
    const b2 = getPaletteRecencyBonus("third", recents);
    const b3 = getPaletteRecencyBonus("fourth", recents);
    const missing = getPaletteRecencyBonus("absent", recents);

    assert.ok(b0 > b1 && b1 > b2 && b2 > b3 && b3 > 0);
    assert.equal(missing, 0);
    assert.ok(b0 <= 32, "recency boost should stay subtle relative to fuzzy match scores");
  });
});
