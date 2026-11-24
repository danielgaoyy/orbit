import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import {
  SIDEBAR_SELECTOR,
  isFocusInsideSidebar,
  resolveGlobalFindAction,
  type GlobalFindContext,
} from "../../lib/file-viewer-shortcuts.ts";
import type { ConversationSearchHit, StageTurnView } from "../../lib/orbit-ipc.ts";
import { EMPTY_WORK_COUNTS } from "./stage-helpers.ts";
import {
  doesMatchRequireOlderHistoryPagination,
  findMatchesInLoadedTurns,
  mergeLoadedAndRemoteStageMatches,
  resolveNextActiveFindKey,
  stepStageFindIndex,
} from "./stage-find-helpers.ts";

function makeTurn(
  turnIndex: number,
  anchorSeq: number,
  promptText: string,
  replySeq: number,
  replyText: string,
  extra?: Partial<StageTurnView>,
): StageTurnView {
  return {
    key: `turn-${anchorSeq}`,
    turn_index: turnIndex,
    anchor_seq: anchorSeq,
    user_prompt: {
      item_type: "user_prompt",
      seq: anchorSeq,
      message_id: `u-${anchorSeq}`,
      text: promptText,
      blocks: [],
    },
    agent_message: null,
    is_compaction_command: false,
    folded_steps: [],
    work_counts: EMPTY_WORK_COUNTS,
    work_summary_label: "",
    work_summary_live_label: "",
    work_summary_includes_failure: false,
    thought_duration_ms: null,
    turn_duration_ms: null,
    terminal_reply: null,
    notices: [],
    compactions: [],
    live_segments: [
      {
        kind: "assistant_message",
        key: `seg-${replySeq}`,
        item: {
          item_type: "assistant_message",
          seq: replySeq,
          item_id: `a-${replySeq}`,
          text: replyText,
          streaming: false,
          blocks: [],
        },
      },
    ],
    turn_diffs: [],
    has_error: false,
    has_unrecovered_error: false,
    has_active_item: false,
    started_at_ms: null,
    completed_at_ms: null,
    ...extra,
  };
}

function makeRemoteHit(
  seq: number,
  overrides: Partial<ConversationSearchHit> = {},
): ConversationSearchHit {
  return {
    host_id: "host_1",
    host_name: "Devbox",
    agent_id: "ag_1",
    seq,
    turn_id: `t-${seq}`,
    role: "assistant",
    title: "Session",
    project_name: "orbit",
    snippet: `Historical <mark>match</mark> at seq ${seq}`,
    updated_at_rfc3339: "2026-11-24T12:00:00Z",
    is_remote: true,
    is_unloaded: true,
    ...overrides,
  };
}

function makeFindKeyCtx(overrides: Partial<GlobalFindContext> = {}): GlobalFindContext {
  return {
    mod: true,
    shiftKey: false,
    altKey: false,
    keyLower: "f",
    terminalFocused: false,
    focusInsideSidePanel: false,
    focusInsideSidebar: false,
    isOverlayOpen: false,
    hasOpenConversation: true,
    ...overrides,
  };
}

describe("StageFindBar pure match engine & chronological unification", () => {
  it("finds multiple occurrences across loaded turns in chronological order (ASCII + CJK)", () => {
    const turns = [
      makeTurn(
        0,
        100,
        "Check postgres timeout behavior",
        101,
        "Postgres pool timeout fixed; retry timeout set to 2s.",
      ),
      makeTurn(
        1,
        110,
        "请验证数据库连接超时问题",
        111,
        "已完成数据库连接超时排查。",
      ),
    ];

    const ascii = findMatchesInLoadedTurns(turns, "timeout");
    assert.deepEqual(
      ascii.map((m) => ({
        key: m.key,
        seq: m.seq,
        turnIndex: m.turnIndex,
        vis: m.visibleOccurrenceInTurn,
        unloaded: m.unloaded,
      })),
      [
        { key: "100:0", seq: 100, turnIndex: 0, vis: 0, unloaded: false },
        { key: "101:0", seq: 101, turnIndex: 0, vis: 1, unloaded: false },
        { key: "101:1", seq: 101, turnIndex: 0, vis: 2, unloaded: false },
      ],
    );

    const cjk = findMatchesInLoadedTurns(turns, "连接超时");
    assert.equal(cjk.length, 2);
    assert.deepEqual(
      cjk.map((m) => m.seq),
      [110, 111],
    );
  });

  it("unifies loaded matches with unloaded remote daemon hits in strict chronological seq order without duplicates", () => {
    const turns = [
      makeTurn(0, 100, "Recent turn with nebula token", 101, "Confirmed nebula in tail"),
      makeTurn(1, 110, "Follow-up question", 111, "Another nebula check"),
    ];

    const remoteHits: ConversationSearchHit[] = [
      // Duplicate of already-loaded seq 101 -> must NOT double-count
      makeRemoteHit(101, { is_remote: false, is_unloaded: false }),
      // Unloaded historical hits older than oldestLoadedSeq (100)
      makeRemoteHit(42, { role: "assistant", is_remote: true, is_unloaded: true }),
      makeRemoteHit(7, { role: "user", is_remote: true, is_unloaded: true }),
      // Diacritic/FTS-only hit in loaded turn seq 110 that literal substring didn't match
      makeRemoteHit(110, { role: "user", is_remote: true, is_unloaded: false }),
    ];

    const unified = mergeLoadedAndRemoteStageMatches(turns, "nebula", remoteHits, 100);
    assert.deepEqual(
      unified.map((m) => ({ seq: m.seq, unloaded: m.unloaded, turnIndex: m.turnIndex })),
      [
        { seq: 7, unloaded: true, turnIndex: null },
        { seq: 42, unloaded: true, turnIndex: null },
        { seq: 100, unloaded: false, turnIndex: 0 },
        { seq: 101, unloaded: false, turnIndex: 0 },
        { seq: 110, unloaded: false, turnIndex: 1 },
        { seq: 111, unloaded: false, turnIndex: 1 },
      ],
    );
  });

  it("keeps active cursor anchored on the current loaded match when older remote hits arrive", () => {
    const turns = [makeTurn(0, 100, "nebula here", 101, "nebula there")];
    const initial = mergeLoadedAndRemoteStageMatches(turns, "nebula", [], 100);
    const firstKey = resolveNextActiveFindKey(initial, null);
    assert.equal(firstKey, "100:0");

    // Remote hits at seq 5 and 20 arrive 180ms later: cursor stays calmly locked on `100:0`!
    const expanded = mergeLoadedAndRemoteStageMatches(
      turns,
      "nebula",
      [makeRemoteHit(5), makeRemoteHit(20)],
      100,
    );
    assert.equal(resolveNextActiveFindKey(expanded, firstKey), "100:0");

    // When all matches are unloaded remote history, pick newest historical hit closest to tail
    const historyOnly = mergeLoadedAndRemoteStageMatches(
      turns,
      "ancient_only_token",
      [makeRemoteHit(5), makeRemoteHit(45)],
      100,
    );
    assert.equal(resolveNextActiveFindKey(historyOnly, null), "45:0");
  });

  it("wraps around cleanly on next (+1) and previous (-1) stepping", () => {
    assert.equal(stepStageFindIndex(0, 0, 1), -1);
    assert.equal(stepStageFindIndex(0, 3, 1), 1);
    assert.equal(stepStageFindIndex(2, 3, 1), 0);
    assert.equal(stepStageFindIndex(0, 3, -1), 2);
    assert.equal(stepStageFindIndex(2, 3, -1), 1);
  });

  it("triggers older-history pagination strictly when active match is unloaded and older history exists", () => {
    const turns = [makeTurn(0, 100, "Loaded tail", 101, "Reply tail")];
    const matches = mergeLoadedAndRemoteStageMatches(
      turns,
      "tail",
      [makeRemoteHit(12, { is_unloaded: true })],
      100,
    );
    const unloadedMatch = matches[0];
    const loadedMatch = matches[1];

    assert.equal(unloadedMatch.seq, 12);
    assert.equal(loadedMatch.seq, 100);

    // Unloaded match with older remote history -> triggers pagination
    assert.equal(doesMatchRequireOlderHistoryPagination(unloadedMatch, turns, 0, true), true);
    // Unloaded match with earlier local window turns -> triggers pagination
    assert.equal(doesMatchRequireOlderHistoryPagination(unloadedMatch, turns, 25, false), true);
    // No older history remaining -> does not spin infinitely
    assert.equal(doesMatchRequireOlderHistoryPagination(unloadedMatch, turns, 0, false), false);
    // Already-loaded match -> never triggers pagination
    assert.equal(doesMatchRequireOlderHistoryPagination(loadedMatch, turns, 25, true), false);
  });
});

describe("Global Cmd+F / Cmd+Shift+F focus-precedence contract", () => {
  it("routes Cmd+F to StageFindBar in Main Stage, Sidebar filter in Sidebar, and yields to Terminal & Side Panel", () => {
    // 1. Main Stage / default with open conversation -> StageFindBar
    assert.equal(resolveGlobalFindAction(makeFindKeyCtx()), "stage_find");

    // 2. Focus inside Left Sidebar -> Sidebar thread filter
    assert.equal(
      resolveGlobalFindAction(makeFindKeyCtx({ focusInsideSidebar: true })),
      "sidebar_filter",
    );

    // 3. Cmd+Shift+F -> Sidebar thread filter even from Stage or Side Panel
    assert.equal(
      resolveGlobalFindAction(makeFindKeyCtx({ shiftKey: true, focusInsideSidePanel: true })),
      "sidebar_filter",
    );

    // 4. Focus inside Terminal -> yield to xterm search
    assert.equal(resolveGlobalFindAction(makeFindKeyCtx({ terminalFocused: true })), "yield");
    assert.equal(
      resolveGlobalFindAction(makeFindKeyCtx({ shiftKey: true, terminalFocused: true })),
      "yield",
    );

    // 5. Focus inside Right Side Panel (File Viewer / Code Editor) -> yield to Find-in-File
    assert.equal(
      resolveGlobalFindAction(makeFindKeyCtx({ focusInsideSidePanel: true })),
      "yield",
    );

    // 6. Blocking modal overlay / Settings -> yield
    assert.equal(resolveGlobalFindAction(makeFindKeyCtx({ isOverlayOpen: true })), "yield");

    // 7. No open conversation (draft / launchpad) -> fall back to Sidebar filter
    assert.equal(
      resolveGlobalFindAction(makeFindKeyCtx({ hasOpenConversation: false })),
      "sidebar_filter",
    );
  });

  it("detects sidebar focus cleanly via data-orbit-sidebar marker", () => {
    const body = { tagName: "BODY" } as unknown as Element;
    const html = { tagName: "HTML" } as unknown as Element;
    const sidebarChild = {
      closest: (sel: string) => (sel === SIDEBAR_SELECTOR ? sidebarChild : null),
    } as unknown as Element;
    const stageChild = {
      closest: () => null,
    } as unknown as Element;
    const doc = { body, documentElement: html, querySelector: () => null };

    assert.equal(isFocusInsideSidebar(stageChild, sidebarChild, doc), true);
    assert.equal(isFocusInsideSidebar(sidebarChild, body, doc), true);
    assert.equal(isFocusInsideSidebar(stageChild, stageChild, doc), false);
  });
});

describe("StageFindBar component SSR craft & state checks", () => {
  let server: ViteDevServer;

  before(async () => {
    server = await createServer({
      server: { middlewareMode: true, hmr: false, ws: false, watch: null },
      optimizeDeps: { noDiscovery: true, include: [] },
    });
  });

  after(async () => {
    await server?.close();
  });

  it("renders floating capsule with tabular counter, overview right offset, and disabled controls at 0 matches", async () => {
    const { StageFindBar } = await server.ssrLoadModule(
      "/src/components/stage/stage-find-bar.tsx",
    );

    const htmlMatches = renderToStaticMarkup(
      createElement(StageFindBar, {
        open: true,
        focusNonce: 1,
        query: "postgres",
        onQueryChange: () => {},
        activeIndex: 2,
        totalMatches: 12,
        hasUnloadedMatches: true,
        isActiveMatchUnloaded: false,
        isRemoteSearching: false,
        isPaginatingJump: false,
        overviewReservePx: 304,
        onStep: () => {},
        onClose: () => {},
      }),
    );

    assert.match(htmlMatches, /role="search"/);
    assert.match(htmlMatches, /floating-surface/);
    assert.match(htmlMatches, /right:320px/);
    assert.match(htmlMatches, />3 \/ 12</);
    assert.match(htmlMatches, /tabular-nums/);

    const htmlEmpty = renderToStaticMarkup(
      createElement(StageFindBar, {
        open: true,
        focusNonce: 1,
        query: "nonexistent",
        onQueryChange: () => {},
        activeIndex: -1,
        totalMatches: 0,
        hasUnloadedMatches: false,
        isActiveMatchUnloaded: false,
        isRemoteSearching: false,
        isPaginatingJump: false,
        overviewReservePx: 0,
        onStep: () => {},
        onClose: () => {},
      }),
    );
    assert.match(htmlEmpty, />0 \/ 0</);
    assert.match(htmlEmpty, /text-signal-danger/);
    assert.match(htmlEmpty, /disabled=""/);

    const htmlClosed = renderToStaticMarkup(
      createElement(StageFindBar, {
        open: false,
        focusNonce: 0,
        query: "",
        onQueryChange: () => {},
        activeIndex: -1,
        totalMatches: 0,
        hasUnloadedMatches: false,
        isActiveMatchUnloaded: false,
        isRemoteSearching: false,
        isPaginatingJump: false,
        overviewReservePx: 0,
        onStep: () => {},
        onClose: () => {},
      }),
    );
    assert.equal(htmlClosed, "");
  });
});
