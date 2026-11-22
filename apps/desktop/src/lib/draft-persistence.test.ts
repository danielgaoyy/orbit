import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  DRAFTS_STORAGE_KEY,
  DRAFT_SCHEMA_VERSION,
  DRAFT_TTL_MS,
  MAX_TOTAL_DRAFT_BYTES,
  buildPersistedDraftPayload,
  createDraftPersistenceController,
  hasNonEmptyScopeDraft,
  parseAndValidatePersistedDrafts,
  truncateUtf8,
  utf8ByteLength,
  type DocumentEventTargetLike,
  type DraftStateSlices,
  type StorageLike,
  type WindowEventTargetLike,
} from "./draft-persistence.ts";

class MemoryStorage implements StorageLike {
  readonly data = new Map<string, string>();
  setCalls = 0;
  removeCalls = 0;
  throwOnSetBytesLimit: number | null = null;
  throwOnGet = false;

  getItem(key: string): string | null {
    if (this.throwOnGet) throw new Error("SecurityError: storage unavailable");
    return this.data.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.setCalls += 1;
    if (
      this.throwOnSetBytesLimit !== null &&
      utf8ByteLength(value) > this.throwOnSetBytesLimit
    ) {
      throw new Error("QuotaExceededError");
    }
    this.data.set(key, value);
  }

  removeItem(key: string): void {
    this.removeCalls += 1;
    this.data.delete(key);
  }
}

class FakeEventTarget implements WindowEventTargetLike, DocumentEventTargetLike {
  visibilityState: DocumentVisibilityState = "visible";
  readonly listeners = new Map<string, Set<EventListenerOrEventListenerObject>>();

  addEventListener(type: string, listener: EventListenerOrEventListenerObject): void {
    let set = this.listeners.get(type);
    if (!set) {
      set = new Set();
      this.listeners.set(type, set);
    }
    set.add(listener);
  }

  removeEventListener(type: string, listener: EventListenerOrEventListenerObject): void {
    this.listeners.get(type)?.delete(listener);
  }

  dispatch(type: string): void {
    const set = this.listeners.get(type);
    if (!set) return;
    for (const listener of Array.from(set)) {
      if (typeof listener === "function") {
        listener({ type } as Event);
      } else {
        listener.handleEvent({ type } as Event);
      }
    }
  }

  listenerCount(type?: string): number {
    if (type) return this.listeners.get(type)?.size ?? 0;
    let total = 0;
    for (const set of this.listeners.values()) total += set.size;
    return total;
  }
}

function emptySlices(): DraftStateSlices {
  return {
    composerDrafts: {},
    pastedAttachments: {},
    diffQuotes: {},
    slashDirectives: {},
    sideQuestionDrafts: {},
  };
}

describe("draft-persistence", () => {
  it("roundtrips composer text, pastes, diff quotes, slash directives, and side-question drafts across restarts", async () => {
    const storage = new MemoryStorage();
    const fakeWindow = new FakeEventTarget();
    const fakeDoc = new FakeEventTarget();
    let clock = 1_700_000_000_000;

    const slices = emptySlices();
    slices.composerDrafts["host-1::agent-a"] = "  keep leading & trailing spaces ";
    slices.pastedAttachments["host-1::agent-a"] = [
      {
        id: "paste_1",
        label: "snippet.ts · 3 lines",
        filename: "snippet.ts",
        lineCount: 3,
        content: "const a = 1;\nconst b = 2;\nexport { a, b };",
      },
    ];
    slices.diffQuotes["host-1::agent-a"] = [
      { id: "quote_1", path: "src/app.ts", lineNumber: 42, code: "return ok();" },
      { id: "quote_2", path: "src/lib.ts", lineNumber: null, code: "export const x = 1;" },
    ];
    slices.slashDirectives["host-1::agent-a"] = "review";
    slices.sideQuestionDrafts["host-1::agent-a"] = "Why does this retry twice?";
    slices.composerDrafts["draft::host-1"] = "Start a brand new task";
    slices.slashDirectives["draft::host-1"] = "plan";

    const writer = createDraftPersistenceController(() => slices, {
      storage,
      debounceMs: 25,
      now: () => clock,
      windowTarget: fakeWindow,
      documentTarget: fakeDoc,
    });

    // Debounce coalesces rapid mutations without immediate synchronous writes
    writer.markDirty("host-1::agent-a");
    clock += 10;
    writer.markDirty("draft::host-1");
    assert.equal(storage.setCalls, 0);

    await new Promise((resolve) => setTimeout(resolve, 60));
    assert.equal(storage.setCalls, 1);
    assert.ok(storage.data.has(DRAFTS_STORAGE_KEY));
    writer.dispose();

    // Simulate full Desktop restart with a brand new controller
    const freshSlices = emptySlices();
    const reader = createDraftPersistenceController(() => freshSlices, {
      storage,
      now: () => clock + 5_000,
      windowTarget: fakeWindow,
      documentTarget: fakeDoc,
    });
    const hydrated = reader.hydrate();
    reader.dispose();

    assert.equal(hydrated.prunedCount, 0);
    assert.equal(
      hydrated.composerDrafts["host-1::agent-a"],
      "  keep leading & trailing spaces ",
    );
    assert.deepEqual(hydrated.pastedAttachments["host-1::agent-a"], [
      {
        id: "paste_1",
        label: "snippet.ts · 3 lines",
        filename: "snippet.ts",
        lineCount: 3,
        content: "const a = 1;\nconst b = 2;\nexport { a, b };",
      },
    ]);
    assert.deepEqual(hydrated.diffQuotes["host-1::agent-a"], [
      { id: "quote_1", path: "src/app.ts", lineNumber: 42, code: "return ok();" },
      { id: "quote_2", path: "src/lib.ts", lineNumber: null, code: "export const x = 1;" },
    ]);
    assert.equal(hydrated.slashDirectives["host-1::agent-a"], "review");
    assert.equal(
      hydrated.sideQuestionDrafts["host-1::agent-a"],
      "Why does this retry twice?",
    );
    assert.equal(hydrated.composerDrafts["draft::host-1"], "Start a brand new task");
    assert.equal(hydrated.slashDirectives["draft::host-1"], "plan");
  });

  it("flushes synchronously on beforeunload, pagehide, and hidden visibilitychange and removes listeners on dispose", () => {
    const storage = new MemoryStorage();
    const fakeWindow = new FakeEventTarget();
    const fakeDoc = new FakeEventTarget();
    const slices = emptySlices();

    const controller = createDraftPersistenceController(() => slices, {
      storage,
      debounceMs: 60_000,
      now: () => 1_700_000_000_000,
      windowTarget: fakeWindow,
      documentTarget: fakeDoc,
    });

    assert.equal(fakeWindow.listenerCount("beforeunload"), 1);
    assert.equal(fakeWindow.listenerCount("pagehide"), 1);
    assert.equal(fakeDoc.listenerCount("visibilitychange"), 1);

    // 1. beforeunload flushes immediately
    slices.composerDrafts["h::1"] = "unsent on unload";
    controller.markDirty("h::1");
    assert.equal(storage.setCalls, 0);
    fakeWindow.dispatch("beforeunload");
    assert.equal(storage.setCalls, 1);

    // 2. pagehide flushes immediately
    slices.composerDrafts["h::1"] = "unsent on pagehide";
    controller.markDirty("h::1");
    fakeWindow.dispatch("pagehide");
    assert.equal(storage.setCalls, 2);

    // 3. visibilitychange only flushes when hidden
    slices.composerDrafts["h::1"] = "unsent on hide";
    controller.markDirty("h::1");
    fakeDoc.visibilityState = "visible";
    fakeDoc.dispatch("visibilitychange");
    assert.equal(storage.setCalls, 2);
    fakeDoc.visibilityState = "hidden";
    fakeDoc.dispatch("visibilitychange");
    assert.equal(storage.setCalls, 3);

    // 4. dispose removes every listener and prevents future writes
    slices.composerDrafts["h::1"] = "after dispose";
    controller.markDirty("h::1");
    controller.dispose();
    assert.equal(fakeWindow.listenerCount(), 0);
    assert.equal(fakeDoc.listenerCount(), 0);
    fakeWindow.dispatch("beforeunload");
    fakeDoc.dispatch("visibilitychange");
    assert.equal(storage.setCalls, 3);
  });

  it("prunes drafts older than 30 days and cleans storage", () => {
    const nowMs = 1_750_000_000_000;
    const storage = new MemoryStorage();
    storage.setItem(
      DRAFTS_STORAGE_KEY,
      JSON.stringify({
        version: DRAFT_SCHEMA_VERSION,
        drafts: {
          "host::fresh": { updatedAt: nowMs - (DRAFT_TTL_MS - 60_000), text: "still valid" },
          "host::expired": { updatedAt: nowMs - (DRAFT_TTL_MS + 1), text: "too old" },
        },
      }),
    );

    const controller = createDraftPersistenceController(emptySlices, {
      storage,
      now: () => nowMs,
      windowTarget: null,
      documentTarget: null,
    });
    const hydrated = controller.hydrate();
    controller.dispose();

    assert.equal(hydrated.composerDrafts["host::fresh"], "still valid");
    assert.equal(hydrated.composerDrafts["host::expired"], undefined);
    assert.equal(hydrated.prunedCount, 1);

    const rewritten = JSON.parse(storage.getItem(DRAFTS_STORAGE_KEY) ?? "{}");
    assert.deepEqual(Object.keys(rewritten.drafts), ["host::fresh"]);
  });

  it("prunes empty drafts, removes storage key when empty, and cleans archived/deleted scopes immediately", () => {
    const storage = new MemoryStorage();
    const slices = emptySlices();
    const controller = createDraftPersistenceController(() => slices, {
      storage,
      debounceMs: 1000,
      now: () => 1_700_000_000_000,
      windowTarget: null,
      documentTarget: null,
    });

    slices.composerDrafts["host::whitespace"] = "   \n\t  ";
    slices.composerDrafts["host::keep"] = "active draft";
    slices.composerDrafts["host::to-delete"] = "doomed draft";
    controller.markDirty(["host::whitespace", "host::keep", "host::to-delete"]);
    controller.flushNow();

    const parsed1 = JSON.parse(storage.getItem(DRAFTS_STORAGE_KEY) ?? "{}");
    assert.deepEqual(Object.keys(parsed1.drafts).sort(), ["host::keep", "host::to-delete"]);

    // Simulate archiving / deleting `host::to-delete`
    delete slices.composerDrafts["host::to-delete"];
    controller.removeScopes(["host::to-delete"], true);
    const parsed2 = JSON.parse(storage.getItem(DRAFTS_STORAGE_KEY) ?? "{}");
    assert.deepEqual(Object.keys(parsed2.drafts), ["host::keep"]);

    // Simulate sending `host::keep` -> removes storage key completely
    slices.composerDrafts["host::keep"] = "";
    controller.removeScopes(["host::keep"], true);
    assert.equal(storage.getItem(DRAFTS_STORAGE_KEY), null);
    controller.dispose();

    // Hydrate also excludes scopes already marked archived
    storage.setItem(
      DRAFTS_STORAGE_KEY,
      JSON.stringify({
        version: DRAFT_SCHEMA_VERSION,
        drafts: {
          "host::archived": { updatedAt: 1_700_000_000_000, text: "stale archived draft" },
          "host::live": { updatedAt: 1_700_000_000_000, text: "live draft" },
        },
      }),
    );
    const reader = createDraftPersistenceController(emptySlices, {
      storage,
      now: () => 1_700_000_001_000,
      windowTarget: null,
      documentTarget: null,
    });
    const hydrated = reader.hydrate({ "host::archived": { archivedAtMs: 1 } });
    reader.dispose();
    assert.equal(hydrated.composerDrafts["host::archived"], undefined);
    assert.equal(hydrated.composerDrafts["host::live"], "live draft");
    assert.equal(hasNonEmptyScopeDraft(hydrated, "host::live"), true);
    assert.equal(hasNonEmptyScopeDraft(hydrated, "host::archived"), false);
  });

  it("enforces per-paste & total byte budgets with LRU eviction by updatedAt", () => {
    const nowMs = 1_700_000_000_000;
    const slices = emptySlices();
    const updatedAt = new Map<string, number>();

    // Oversized paste (> 64 KiB) is dropped while text and small paste survive
    slices.composerDrafts["host::bigpaste"] = "Keep my prompt text";
    slices.pastedAttachments["host::bigpaste"] = [
      {
        id: "huge",
        label: "huge.log",
        lineCount: 1000,
        content: "x".repeat(80 * 1024),
      },
      {
        id: "small",
        label: "small.log",
        lineCount: 1,
        content: "compact log line",
      },
    ];
    updatedAt.set("host::bigpaste", nowMs);

    const singlePayload = buildPersistedDraftPayload(slices, updatedAt, { nowMs });
    assert.equal(singlePayload.drafts["host::bigpaste"]?.text, "Keep my prompt text");
    assert.deepEqual(
      singlePayload.drafts["host::bigpaste"]?.pastedAttachments?.map((p) => p.id),
      ["small"],
    );

    // LRU eviction across 12 scopes of ~60 KiB each (~720 KiB total > 512 KiB budget)
    const manySlices = emptySlices();
    const manyTimes = new Map<string, number>();
    for (let i = 0; i < 12; i++) {
      const key = `host::thread-${String(i).padStart(2, "0")}`;
      manySlices.composerDrafts[key] = `Thread ${i}`;
      manySlices.pastedAttachments[key] = [
        {
          id: `p_${i}`,
          label: `paste ${i}`,
          lineCount: 10,
          content: String.fromCharCode(65 + i).repeat(60 * 1024),
        },
      ];
      manyTimes.set(key, nowMs + i * 1_000);
    }

    const bounded = buildPersistedDraftPayload(manySlices, manyTimes, {
      nowMs: nowMs + 20_000,
    });
    const serializedBytes = utf8ByteLength(JSON.stringify(bounded));
    assert.ok(
      serializedBytes <= MAX_TOTAL_DRAFT_BYTES,
      `expected ${serializedBytes} <= ${MAX_TOTAL_DRAFT_BYTES}`,
    );
    // Newest threads survive; oldest threads were evicted first
    assert.ok(bounded.drafts["host::thread-11"]);
    assert.ok(bounded.drafts["host::thread-10"]);
    assert.equal(bounded.drafts["host::thread-00"], undefined);
    assert.equal(bounded.drafts["host::thread-01"], undefined);
  });

  it("handles QuotaExceededError gracefully by shedding oldest drafts and truncates UTF-8 without splitting surrogate pairs", () => {
    const emojiStr = "😀".repeat(20); // 80 UTF-8 bytes, 40 UTF-16 code units
    const truncated = truncateUtf8(emojiStr, 10); // fits 2 emojis = 8 bytes
    assert.equal(truncated, "😀😀");
    assert.equal(utf8ByteLength(truncated), 8);

    const storage = new MemoryStorage();
    storage.throwOnSetBytesLimit = 180;
    const slices = emptySlices();
    let clock = 1_700_000_000_000;
    const controller = createDraftPersistenceController(() => slices, {
      storage,
      now: () => clock,
      windowTarget: null,
      documentTarget: null,
    });

    slices.composerDrafts["host::old"] = "a".repeat(80);
    controller.markDirty("host::old");
    clock += 1_000;
    slices.composerDrafts["host::new"] = "b".repeat(40);
    controller.markDirty("host::new");

    assert.doesNotThrow(() => controller.flushNow());
    controller.dispose();

    const stored = JSON.parse(storage.getItem(DRAFTS_STORAGE_KEY) ?? "{}");
    assert.equal(stored.drafts["host::old"], undefined);
    assert.equal(stored.drafts["host::new"]?.text, "b".repeat(40));
  });

  it("degrades gracefully on corrupt JSON, wrong versions, prototype keys, and invalid field types", () => {
    const nowMs = 1_700_000_000_000;

    for (const bad of [
      "{not-valid-json",
      "null",
      "[]",
      "42",
      '"hello"',
      JSON.stringify({ version: 99, drafts: { "h::1": { updatedAt: nowMs, text: "hi" } } }),
      JSON.stringify({ drafts: { "h::1": { updatedAt: nowMs, text: "hi" } } }),
      JSON.stringify({ version: 1, drafts: null }),
    ]) {
      const res = parseAndValidatePersistedDrafts(bad, { nowMs });
      assert.deepEqual(res.composerDrafts, {});
      assert.ok(res.prunedCount >= 1);
    }

    // Mix of valid + corrupt entries & fields + prototype pollution keys
    const mixed = `{"version":1,"drafts":{
      "__proto__":{"updatedAt":${nowMs},"text":"evil"},
      "constructor":{"updatedAt":${nowMs},"text":"evil2"},
      "host::bad-ts":{"updatedAt":"nope","text":"dropped"},
      "host::partial":{
        "updatedAt":${nowMs},
        "text":"valid text",
        "slashDirective":"unknown-directive",
        "pastedAttachments":[null,{"id":"","content":"bad"},{"id":"ok","label":"ok","lineCount":2,"content":"a\\nb"}],
        "diffQuotes":[{"id":"q1","path":"a.ts","lineNumber":10,"code":"ok"},{"id":123}],
        "sideQuestionDraft":42
      }
    }}`;

    const res = parseAndValidatePersistedDrafts(mixed, { nowMs });
    assert.equal(res.composerDrafts["host::bad-ts"], undefined);
    assert.equal(Object.prototype.hasOwnProperty.call(res.composerDrafts, "__proto__"), false);
    assert.equal(Object.prototype.hasOwnProperty.call(res.composerDrafts, "constructor"), false);
    assert.equal(res.composerDrafts["host::partial"], "valid text");
    assert.equal(res.slashDirectives["host::partial"], undefined);
    assert.equal(res.sideQuestionDrafts["host::partial"], undefined);
    assert.deepEqual(res.pastedAttachments["host::partial"], [
      { id: "ok", label: "ok", lineCount: 2, content: "a\nb" },
    ]);
    assert.deepEqual(res.diffQuotes["host::partial"], [
      { id: "q1", path: "a.ts", lineNumber: 10, code: "ok" },
    ]);

    // Throwing storage never crashes hydrate
    const brokenStorage = new MemoryStorage();
    brokenStorage.throwOnGet = true;
    const ctrl = createDraftPersistenceController(emptySlices, {
      storage: brokenStorage,
      windowTarget: null,
      documentTarget: null,
    });
    assert.deepEqual(ctrl.hydrate().composerDrafts, {});
    ctrl.dispose();
  });
});
