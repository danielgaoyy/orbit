import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  DRAFTS_STORAGE_KEY,
  DRAFT_SCHEMA_VERSION,
  DRAFT_TTL_MS,
  MAX_FUTURE_TIMESTAMP_SKEW_MS,
  MAX_TOTAL_DRAFT_BYTES,
  buildPersistedDraftPayload,
  buildSerializedDraftPayload,
  createDraftPersistenceController,
  hasNonEmptyScopeDraft,
  mergeMissingDraftSlices,
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

    slices.slashDirectives["host-1::side"] = "btw";
    slices.slashDirectives["host-2::goal"] = "goal";

    const writer = createDraftPersistenceController(() => slices, {
      storage,
      debounceMs: 25,
      now: () => clock,
      windowTarget: fakeWindow,
      documentTarget: fakeDoc,
    });

    // Debounce coalesces rapid mutations without immediate synchronous writes
    writer.markDirty(["host-1::agent-a", "host-1::side", "host-2::goal"]);
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
    assert.equal(hydrated.slashDirectives["host-1::side"], "btw");
    assert.equal(hydrated.slashDirectives["host-2::goal"], "goal");
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

  it("preserves original timestamps of LRU-evicted in-memory scopes to prevent LRU inversion on subsequent flushes", () => {
    const storage = new MemoryStorage();
    const slices = emptySlices();
    let clock = 1_700_000_000_000;

    // Budget big enough for 1 large draft (~190 B), too small for 2 (~350 B)
    const controller = createDraftPersistenceController(() => slices, {
      storage,
      maxTotalBytes: 256,
      maxEntryBytes: 220,
      now: () => clock,
      windowTarget: null,
      documentTarget: null,
    });

    slices.composerDrafts["host::old"] = "o".repeat(120);
    controller.markDirty("host::old");
    clock += 5_000;
    slices.composerDrafts["host::new"] = "n".repeat(120);
    controller.markDirty("host::new");
    controller.flushNow();

    const persisted1 = JSON.parse(storage.getItem(DRAFTS_STORAGE_KEY) ?? "{}");
    assert.equal(persisted1.drafts["host::old"], undefined);
    assert.equal(persisted1.drafts["host::new"]?.text, "n".repeat(120));
    // Original timestamp for still-in-memory evicted scope must remain intact
    assert.equal(controller.exportUpdatedAtByScope()["host::old"], 1_700_000_000_000);

    // Subsequent flush while host::old is still in memory must NOT re-stamp host::old
    // with nowMs and invert LRU order against host::new
    clock += 10_000;
    slices.composerDrafts["host::new"] = "m".repeat(110);
    controller.markDirty("host::new");
    controller.flushNow();

    const persisted2 = JSON.parse(storage.getItem(DRAFTS_STORAGE_KEY) ?? "{}");
    assert.equal(persisted2.drafts["host::old"], undefined);
    assert.equal(persisted2.drafts["host::new"]?.text, "m".repeat(110));
    assert.equal(controller.exportUpdatedAtByScope()["host::old"], 1_700_000_000_000);

    // Once host::old is genuinely cleared in memory, its timestamp is pruned on flush
    delete slices.composerDrafts["host::old"];
    controller.flushNow(true);
    assert.equal(controller.exportUpdatedAtByScope()["host::old"], undefined);
    controller.dispose();
  });

  it("does not destructively remove previously persisted drafts when setItem keeps throwing QuotaExceededError", () => {
    const storage = new MemoryStorage();
    const initialJson = JSON.stringify({
      version: DRAFT_SCHEMA_VERSION,
      drafts: {
        "host::existing": { updatedAt: 1_700_000_000_000, text: "valid saved draft" },
      },
    });
    storage.data.set(DRAFTS_STORAGE_KEY, initialJson);
    // Simulate storage completely full: every setItem throws QuotaExceededError
    storage.throwOnSetBytesLimit = 0;

    const slices = emptySlices();
    slices.composerDrafts["host::existing"] = "valid saved draft";
    slices.composerDrafts["host::unsaved"] = "new draft that triggers quota error";

    const controller = createDraftPersistenceController(() => slices, {
      storage,
      now: () => 1_700_000_005_000,
      windowTarget: null,
      documentTarget: null,
    });
    controller.markDirty(["host::existing", "host::unsaved"]);

    assert.doesNotThrow(() => controller.flushNow());
    assert.equal(storage.removeCalls, 0);
    assert.equal(storage.getItem(DRAFTS_STORAGE_KEY), initialJson);
    controller.dispose();
  });

  it("clamps future updatedAt timestamps beyond 60s skew on parse, build, and flush", () => {
    const nowMs = 1_700_000_000_000;
    const farFuture = nowMs + MAX_FUTURE_TIMESTAMP_SKEW_MS + 5_000_000;
    const slightFuture = nowMs + 30_000;

    // 1. parseAndValidatePersistedDrafts clamps far-future timestamps and flags cleanup
    const raw = JSON.stringify({
      version: DRAFT_SCHEMA_VERSION,
      drafts: {
        "host::skewed": { updatedAt: farFuture, text: "x".repeat(120) },
        "host::slight": { updatedAt: slightFuture, text: "y".repeat(120) },
      },
    });
    const parsedAll = parseAndValidatePersistedDrafts(raw, { nowMs });
    assert.equal(parsedAll.updatedAtByScope["host::skewed"], nowMs);
    assert.equal(parsedAll.updatedAtByScope["host::slight"], slightFuture);
    assert.equal(parsedAll.prunedCount, 1);

    // Under tight budget, clamped far-future draft (nowMs) is evicted before slightFuture
    const parsedTight = parseAndValidatePersistedDrafts(raw, {
      nowMs,
      maxTotalBytes: 256,
      maxEntryBytes: 220,
    });
    assert.equal(parsedTight.composerDrafts["host::skewed"], undefined);
    assert.equal(parsedTight.composerDrafts["host::slight"], "y".repeat(120));

    // 2. buildPersistedDraftPayload clamps far-future timestamps so they cannot pin LRU
    const slices = emptySlices();
    slices.composerDrafts["host::skewed"] = "a".repeat(120);
    slices.composerDrafts["host::recent"] = "b".repeat(120);
    const builtAll = buildPersistedDraftPayload(
      slices,
      { "host::skewed": farFuture, "host::recent": slightFuture },
      { nowMs },
    );
    assert.equal(builtAll.drafts["host::skewed"]?.updatedAt, nowMs);
    assert.equal(builtAll.drafts["host::recent"]?.updatedAt, slightFuture);

    const builtTight = buildPersistedDraftPayload(
      slices,
      { "host::skewed": farFuture, "host::recent": slightFuture },
      { nowMs, maxTotalBytes: 256, maxEntryBytes: 220 },
    );
    assert.equal(builtTight.drafts["host::skewed"], undefined);
    assert.equal(builtTight.drafts["host::recent"]?.text, "b".repeat(120));

    // 3. Controller flush normalizes skewed in-memory timestamps if system clock jumped backward
    const storage = new MemoryStorage();
    let clock = farFuture;
    const ctrlSlices = emptySlices();
    const ctrl = createDraftPersistenceController(() => ctrlSlices, {
      storage,
      now: () => clock,
      windowTarget: null,
      documentTarget: null,
    });
    ctrlSlices.composerDrafts["host::skewed"] = "before clock jump";
    ctrl.markDirty("host::skewed");
    // Clock jumps backward to nowMs before flush
    clock = nowMs;
    ctrl.flushNow();
    assert.equal(ctrl.exportUpdatedAtByScope()["host::skewed"], nowMs);
    ctrl.dispose();
  });

  it("serializes full payload in a single pass on flush and trims only needed excess bytes", () => {
    const storage = new MemoryStorage();
    const slices = emptySlices();
    const nowMs = 1_700_000_000_000;
    slices.composerDrafts["host::a"] = "draft alpha";
    slices.composerDrafts["host::b"] = "draft beta";

    const controller = createDraftPersistenceController(() => slices, {
      storage,
      now: () => nowMs,
      windowTarget: null,
      documentTarget: null,
    });
    controller.markDirty(["host::a", "host::b"]);

    const origStringify = JSON.stringify;
    let fullPayloadStringifies = 0;
    JSON.stringify = ((value: unknown, ...rest: unknown[]) => {
      if (
        typeof value === "object" &&
        value !== null &&
        "version" in value &&
        "drafts" in value
      ) {
        fullPayloadStringifies += 1;
      }
      return (origStringify as (...args: unknown[]) => string)(value, ...rest);
    }) as typeof JSON.stringify;

    try {
      controller.flushNow();
    } finally {
      JSON.stringify = origStringify;
    }

    assert.equal(fullPayloadStringifies, 1);
    assert.equal(storage.setCalls, 1);
    const expected = buildSerializedDraftPayload(slices, controller.exportUpdatedAtByScope(), {
      nowMs,
    });
    assert.equal(storage.getItem(DRAFTS_STORAGE_KEY), expected.json);
    controller.dispose();

    // Evicting many scopes avoids O(N^2) full-payload re-stringification
    const manySlices = emptySlices();
    const manyTimes: Record<string, number> = {};
    for (let i = 0; i < 20; i++) {
      const key = `host::s-${String(i).padStart(2, "0")}`;
      manySlices.composerDrafts[key] = "z".repeat(80);
      manyTimes[key] = nowMs + i * 1_000;
    }
    let evictionFullStringifies = 0;
    JSON.stringify = ((value: unknown, ...rest: unknown[]) => {
      if (
        typeof value === "object" &&
        value !== null &&
        "version" in value &&
        "drafts" in value
      ) {
        evictionFullStringifies += 1;
      }
      return (origStringify as (...args: unknown[]) => string)(value, ...rest);
    }) as typeof JSON.stringify;
    let evictedResult: ReturnType<typeof buildSerializedDraftPayload>;
    try {
      evictedResult = buildSerializedDraftPayload(manySlices, manyTimes, {
        nowMs: nowMs + 30_000,
        maxTotalBytes: 512,
      });
    } finally {
      JSON.stringify = origStringify;
    }
    assert.ok(evictedResult.evictedKeys.length >= 15);
    assert.ok(evictedResult.json !== null);
    assert.ok(utf8ByteLength(evictedResult.json) <= 512);
    assert.ok(
      evictionFullStringifies <= 2,
      `expected <= 2 full-payload stringifications, got ${evictionFullStringifies}`,
    );

    // Tight entry trimming removes only excess bytes rather than halving text
    const tightSlices = emptySlices();
    tightSlices.composerDrafts["host::tight"] = "a".repeat(220);
    const tightPayload = buildPersistedDraftPayload(
      tightSlices,
      { "host::tight": nowMs },
      { nowMs, maxTotalBytes: 512, maxEntryBytes: 256, maxTextBytes: 256 },
    );
    const trimmedText = tightPayload.drafts["host::tight"]?.text ?? "";
    assert.ok(
      trimmedText.length >= 190 && trimmedText.length < 220,
      `expected tight trim preserving >= 190 chars (not halved to ~110), got ${trimmedText.length}`,
    );
  });

  it("persists staged image and file context items across reloads while stripping ephemeral blob URLs", () => {
    const storage = new MemoryStorage();
    const fakeWindow = new FakeEventTarget();
    const fakeDoc = new FakeEventTarget();
    const nowMs = 1_700_000_000_000;

    const slices = emptySlices();
    slices.contextItems = {
      "host-1::agent-img": [
        {
          id: "img_1",
          kind: "image",
          filename: "screenshot.png",
          path: "/Users/jess/.orbit/attachments/1795_shot.png",
          previewUrl: "blob:tauri://localhost/ephemeral-uuid",
          sizeBytes: 42_100,
          stagedHostId: "host-1",
          uploading: false,
        },
        {
          id: "img_pending",
          kind: "image",
          filename: "uploading.png",
          path: "",
          previewUrl: "blob:tauri://localhost/not-staged-yet",
          sizeBytes: 1024,
          uploading: true,
        },
        {
          id: "pdf_1",
          kind: "file",
          path: "/Users/jess/.orbit/attachments/spec.pdf",
          filename: "spec.pdf",
          sizeBytes: 88_000,
          stagedHostId: "host-1",
          mediaKind: "pdf",
          mimeType: "application/pdf",
          pdfPageCount: 4,
        },
      ],
    };

    assert.equal(hasNonEmptyScopeDraft(slices, "host-1::agent-img"), true);

    const writer = createDraftPersistenceController(() => slices, {
      storage,
      windowTarget: fakeWindow,
      documentTarget: fakeDoc,
      debounceMs: 10,
      now: () => nowMs,
    });
    writer.markDirty("host-1::agent-img");
    writer.flushNow();

    const reader = createDraftPersistenceController(() => emptySlices(), {
      storage,
      windowTarget: fakeWindow,
      documentTarget: fakeDoc,
      now: () => nowMs + 1_000,
    });
    const hydrated = reader.hydrate();
    const restored = hydrated.contextItems["host-1::agent-img"] ?? [];
    assert.equal(restored.length, 2, "only staged items with non-empty path persist");

    const img = restored[0];
    assert.equal(img?.kind, "image");
    if (img?.kind === "image") {
      assert.equal(img.path, "/Users/jess/.orbit/attachments/1795_shot.png");
      assert.equal(img.filename, "screenshot.png");
      assert.equal(img.stagedHostId, "host-1");
      assert.equal(img.sizeBytes, 42_100);
      assert.equal(img.previewUrl, "", "ephemeral blob URL must be stripped");
    }

    const pdf = restored[1];
    assert.equal(pdf?.kind, "file");
    if (pdf?.kind === "file") {
      assert.equal(pdf.path, "/Users/jess/.orbit/attachments/spec.pdf");
      assert.equal(pdf.mediaKind, "pdf");
      assert.equal(pdf.pdfPageCount, 4);
    }

    writer.dispose();
    reader.dispose();
  });

  it("sheds heavy terminal_output context before compact staged image references under tight entry budget", () => {
    const nowMs = 1_700_000_000_000;
    const slices = emptySlices();
    slices.contextItems = {
      "host-1::tight": [
        {
          id: "term_1",
          kind: "terminal_output",
          label: "Terminal · 10 lines",
          lineCount: 10,
          content: "x".repeat(400),
        },
        {
          id: "img_1",
          kind: "image",
          filename: "keep-me.png",
          path: "/Users/jess/.orbit/attachments/keep-me.png",
          previewUrl: "",
          sizeBytes: 2048,
          stagedHostId: "host-1",
        },
      ],
    };

    const payload = buildPersistedDraftPayload(
      slices,
      { "host-1::tight": nowMs },
      { nowMs, maxTotalBytes: 1024, maxEntryBytes: 380 },
    );
    const kept = payload.drafts["host-1::tight"]?.contextItems ?? [];
    assert.equal(kept.length, 1);
    assert.equal(kept[0]?.kind, "image");
  });

  it("roundtrips attached skill pills and drops malformed skill entries on hydration", () => {
    const storage = new MemoryStorage();
    const nowMs = 1_700_000_000_000;
    const slices = emptySlices();
    slices.contextItems = {
      "host-1::skill-scope": [
        {
          id: "skill_interface-design",
          kind: "skill",
          slug: "interface-design",
          label: "Interface Design",
          description: "Craft-first UI review & refinement",
        },
      ],
    };

    assert.equal(hasNonEmptyScopeDraft(slices, "host-1::skill-scope"), true);

    const writer = createDraftPersistenceController(() => slices, {
      storage,
      windowTarget: null,
      documentTarget: null,
      now: () => nowMs,
    });
    writer.markDirty("host-1::skill-scope");
    writer.flushNow();
    writer.dispose();

    const reader = createDraftPersistenceController(emptySlices, {
      storage,
      windowTarget: null,
      documentTarget: null,
      now: () => nowMs + 1_000,
    });
    const hydrated = reader.hydrate();
    reader.dispose();

    assert.deepEqual(hydrated.contextItems["host-1::skill-scope"], [
      {
        id: "skill_interface-design",
        kind: "skill",
        slug: "interface-design",
        label: "Interface Design",
        description: "Craft-first UI review & refinement",
      },
    ]);

    const corrupt = JSON.stringify({
      version: DRAFT_SCHEMA_VERSION,
      drafts: {
        "host-1::mixed": {
          updatedAt: nowMs,
          contextItems: [
            { id: "bad_skill", kind: "skill", slug: "   ", label: "Broken" },
            { id: "good_skill", kind: "skill", slug: "shadcn", label: "shadcn" },
          ],
        },
      },
    });
    const parsed = parseAndValidatePersistedDrafts(corrupt, { nowMs });
    assert.deepEqual(parsed.contextItems["host-1::mixed"], [
      { id: "good_skill", kind: "skill", slug: "shadcn", label: "shadcn" },
    ]);
  });

  it("persists, validates, and LRU-bounds inlineMentions (file, folder, pr, web, code_symbol, git_diff, terminal_output) across restarts", () => {
    const nowMs = 1_785_000_000_000;
    const storage = new MemoryStorage();
    const slices: DraftStateSlices = {
      ...emptySlices(),
      composerDrafts: {
        "host-1::mentions": "Check @src/app.ts @folder:src/lib @pr#42 @web:https://docs.rs/tokio",
      },
      inlineMentions: {
        "host-1::mentions": [
          { token: "@src/app.ts", kind: "file", path: "src/app.ts" },
          { token: "@folder:src/lib", kind: "folder", path: "src/lib", fileCount: 9 },
          {
            token: "@pr#42",
            kind: "pr",
            number: 42,
            title: "Add terminal splits",
            url: "https://github.com/org/orbit/pull/42",
            state: "OPEN",
            headRef: "feat/splits",
            baseRef: "main",
            summary: "2/2 checks passing",
          },
          {
            token: "@web:https://docs.rs/tokio",
            kind: "web",
            url: "https://docs.rs/tokio",
            label: "docs.rs/tokio",
          },
          { token: "@git:working", kind: "git_diff", staged: false, label: "Working diff" },
          {
            token: "@terminal",
            kind: "terminal_output",
            label: "Terminal · 2 lines",
            lineCount: 2,
            content: "ok\ndone",
          },
          {
            token: "@render.rs:L10",
            kind: "code_symbol",
            path: "crates/orbit-client-core/src/render.rs",
            startLine: 10,
            endLine: 40,
            symbolName: "render_markdown_blocks",
            symbolKind: "function",
            signature: "pub fn render_markdown_blocks()",
            container: null,
            language: "rust",
          },
        ],
      },
    };

    assert.equal(hasNonEmptyScopeDraft(slices, "host-1::mentions"), true);

    const writer = createDraftPersistenceController(() => slices, {
      storage,
      windowTarget: null,
      documentTarget: null,
      now: () => nowMs,
    });
    writer.markDirty("host-1::mentions");
    writer.flushNow();
    writer.dispose();

    const reader = createDraftPersistenceController(emptySlices, {
      storage,
      windowTarget: null,
      documentTarget: null,
      now: () => nowMs + 1_000,
    });
    const hydrated = reader.hydrate();
    reader.dispose();

    assert.equal(hydrated.inlineMentions["host-1::mentions"]?.length, 7);
    assert.deepEqual(hydrated.inlineMentions["host-1::mentions"]?.[1], {
      token: "@folder:src/lib",
      kind: "folder",
      path: "src/lib",
      fileCount: 9,
    });
    assert.deepEqual(hydrated.inlineMentions["host-1::mentions"]?.[3], {
      token: "@web:https://docs.rs/tokio",
      kind: "web",
      url: "https://docs.rs/tokio",
      label: "docs.rs/tokio",
    });

    // Strict validation rejects non-https URLs, duplicate tokens, malformed PR numbers, and oversized lists
    const corrupt = JSON.stringify({
      version: DRAFT_SCHEMA_VERSION,
      drafts: {
        "host-1::bad": {
          updatedAt: nowMs,
          inlineMentions: [
            { token: "@web:http://insecure.local", kind: "web", url: "http://insecure.local", label: "bad" },
            { token: "@web:file:///etc/passwd", kind: "web", url: "file:///etc/passwd", label: "bad" },
            { token: "@pr#0", kind: "pr", number: 0, title: "Invalid PR" },
            { token: "@src/good.ts", kind: "file", path: "src/good.ts" },
            { token: "@src/good.ts", kind: "file", path: "src/dup.ts" },
          ],
        },
      },
    });
    const parsed = parseAndValidatePersistedDrafts(corrupt, { nowMs });
    assert.deepEqual(parsed.inlineMentions["host-1::bad"], [
      { token: "@src/good.ts", kind: "file", path: "src/good.ts" },
    ]);

    // Entry byte budget sheds heavy terminal_output inline mentions first while preserving lightweight file/web mentions
    const heavySlices: DraftStateSlices = {
      ...emptySlices(),
      composerDrafts: { "host-1::heavy": "Look at @src/a.ts and @terminal" },
      inlineMentions: {
        "host-1::heavy": [
          { token: "@src/a.ts", kind: "file", path: "src/a.ts" },
          {
            token: "@terminal",
            kind: "terminal_output",
            label: "Huge log",
            lineCount: 1000,
            content: "x".repeat(200_000),
          },
        ],
      },
    };
    const serialized = buildSerializedDraftPayload(
      heavySlices,
      { "host-1::heavy": nowMs },
      { nowMs },
    );
    assert.ok(serialized.json !== null);
    assert.ok(utf8ByteLength(serialized.json) <= MAX_TOTAL_DRAFT_BYTES);
    const hydratedHeavy = parseAndValidatePersistedDrafts(serialized.json, { nowMs });
    assert.ok(
      hydratedHeavy.inlineMentions["host-1::heavy"]?.some((m) => m.token === "@src/a.ts"),
      "lightweight inline mention should survive budget trimming",
    );
  });

  it("invokes onPersistPayload mirror hook on flush/clear and merges SQLite fallback without overwriting active edits", () => {
    const storage = new MemoryStorage();
    const nowMs = 1_750_000_000_000;
    const mirroredPayloads: Array<string | null> = [];
    const slices = emptySlices();

    const controller = createDraftPersistenceController(() => slices, {
      storage,
      debounceMs: 0,
      now: () => nowMs,
      onPersistPayload: (json) => mirroredPayloads.push(json),
    });

    slices.composerDrafts["host-1::ag_1"] = "unsent SQLite draft";
    slices.composerDrafts["host-1::ag_2"] = "old SQLite draft";
    controller.markDirty(["host-1::ag_1", "host-1::ag_2"]);
    controller.flushNow(true);
    assert.equal(mirroredPayloads.length, 1);
    assert.ok(typeof mirroredPayloads[0] === "string" && mirroredPayloads[0].includes("unsent SQLite draft"));

    // Clearing all scopes emits null so SQLite deletes stale drafts too
    delete slices.composerDrafts["host-1::ag_1"];
    delete slices.composerDrafts["host-1::ag_2"];
    controller.removeScopes(["host-1::ag_1", "host-1::ag_2"], true);
    assert.equal(mirroredPayloads[mirroredPayloads.length - 1], null);

    // Cold-start fallback recovery only populates scopes empty in current memory
    const fallbackHydrated = parseAndValidatePersistedDrafts(mirroredPayloads[0], { nowMs });
    const liveSlices: DraftStateSlices = {
      ...emptySlices(),
      composerDrafts: { "host-1::ag_2": "newer user input in memory" },
    };
    const merged = mergeMissingDraftSlices(liveSlices, fallbackHydrated);
    assert.ok(merged !== null);
    assert.deepEqual(merged.recoveredScopes, ["host-1::ag_1"]);
    assert.equal(merged.composerDrafts?.["host-1::ag_1"], "unsent SQLite draft");
    assert.equal(merged.composerDrafts?.["host-1::ag_2"], "newer user input in memory");
    controller.dispose();
  });
});
