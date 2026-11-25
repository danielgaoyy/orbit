import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { createElement, type ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";
import {
  computeDisclosureRevealDelta,
  computeStageScrollState,
  shouldFollowBottomOnDisclosureExpand,
  type ViewportAnchorHoldFn,
} from "./stage-helpers.ts";

const pinned = { previousTop: 100, currentTop: 92, distanceFromBottom: 8, stickToBottom: true };

test("a gentle upward scroll detaches inside the sticky threshold until a downward return", () => {
  const detached = computeStageScrollState(pinned);
  assert.deepEqual(detached, { stickToBottom: false, showJumpLatest: false, isOccludingText: false });
  const reading = { ...pinned, stickToBottom: false };
  for (const [previousTop, currentTop, distanceFromBottom, expected] of [
    [92, 92, 8, false], // A resize/metrics callback between wheel events.
    [92, 90, 10, false],
    [60, 79, 21, false],
    [79, 80, 20, true],
  ] as const) {
    assert.equal(computeStageScrollState({ ...reading, previousTop, currentTop, distanceFromBottom }).stickToBottom, expected);
  }
  assert.equal(computeStageScrollState({ ...reading, currentTop: 102, holdActive: true }).stickToBottom, false);
  assert.equal(computeStageScrollState({ ...pinned, isShellLayoutMoving: true }).stickToBottom, true);
});

test("a browser clamp to the new maximum scrollTop preserves the follower through row remeasurement", () => {
  const clamped = { previousTop: 2116, currentTop: 2086, distanceFromBottom: 0, stickToBottom: true };
  assert.equal(computeStageScrollState(clamped).stickToBottom, true);
  assert.equal(computeStageScrollState({ ...clamped, currentTop: 2078, distanceFromBottom: 8 }).stickToBottom, false);
});

test("fade and jump use independent bottom thresholds", () => {
  for (const [distanceFromBottom, isOccludingText, showJumpLatest] of [
    [0, false, false], [8, false, false], [22, false, false],
    [23, true, false], [72, true, false], [73, true, true],
  ] as const) {
    assert.deepEqual(
      computeStageScrollState({ ...pinned, stickToBottom: false, distanceFromBottom }),
      { stickToBottom: false, isOccludingText, showJumpLatest },
    );
  }
  for (const [stickToBottom, hasTurns] of [[true, true], [false, false]] as const) {
    const state = computeStageScrollState({ ...pinned, currentTop: 100, distanceFromBottom: 150, stickToBottom, hasTurns });
    assert.equal(state.isOccludingText, false);
    assert.equal(state.showJumpLatest, false);
  }
});

const compact = {
  scrollerRect: { top: 100, bottom: 900, height: 800 },
  triggerRect: { top: 750, bottom: 778 },
  beforeContainerHeight: 28,
  containerRect: { bottom: 980, height: 230 },
};

test("collapse pins the trigger without revealing any overflow, including legacy toggles", () => {
  assert.equal(computeDisclosureRevealDelta({ ...compact, intent: "collapse" }), 0);
  assert.equal(computeDisclosureRevealDelta({ ...compact, beforeContainerHeight: 1500 }), 0);
});

test("compact disclosures reveal their overflow plus 16px while reserving 24px above the trigger", () => {
  assert.equal(computeDisclosureRevealDelta({ ...compact, containerRect: { height: 150, bottom: 900 } }), 0);
  assert.equal(computeDisclosureRevealDelta({ ...compact, containerRect: { height: 158, bottom: 908 } }), 0);
  assert.equal(computeDisclosureRevealDelta(compact), 96);
  // A bottom-edge anchor may be nested deep inside the disclosure.
  assert.equal(computeDisclosureRevealDelta({ ...compact, triggerRect: { top: 160, bottom: 188 } }), 36);
  assert.equal(computeDisclosureRevealDelta({ ...compact, triggerRect: { top: 128, bottom: 156 } }), 0);
});

test("a 1500px disclosure reveals only the missing preview and keeps its header visible", () => {
  const tall = { ...compact, containerRect: { bottom: 1700, height: 1500 } };
  assert.equal(computeDisclosureRevealDelta({ ...tall, triggerRect: { top: 200, bottom: 228 } }), 0);
  const triggerRect = { top: 856, bottom: 884 };
  const delta = computeDisclosureRevealDelta({ ...tall, triggerRect });
  assert.equal(delta, 224);
  assert.equal(tall.scrollerRect.bottom - triggerRect.bottom + delta, 240);
  assert.ok(triggerRect.top - delta >= tall.scrollerRect.top + 24);
  assert.equal(computeDisclosureRevealDelta({
    ...tall, scrollerRect: { top: 0, bottom: 400, height: 400 }, triggerRect: { top: 370, bottom: 398 },
  }), 138);
});

test("self-expanding user prompt bubbles measure revealed text below the collapsed preview edge", () => {
  // Bubble expanded from 144px to 760px starting at y=300 inside an 800px scroller (100..900):
  // 456px of expanded content is already visible below y=444 -> must not nudge or jitter.
  assert.equal(computeDisclosureRevealDelta({
    intent: "expand",
    beforeContainerHeight: 144,
    scrollerRect: { top: 100, bottom: 900, height: 800 },
    triggerRect: { top: 300, bottom: 1060 },
    containerRect: { bottom: 1060, height: 760 },
    triggerIsContainer: true,
  }), 0);

  // Bubble expanded right at the bottom edge (top=740, collapsed bottom=884 inside 100..900):
  // only 16px visible below collapsed edge -> smoothly reveals up to targetReveal (240 - 16 = 224px).
  assert.equal(computeDisclosureRevealDelta({
    intent: "expand",
    beforeContainerHeight: 144,
    scrollerRect: { top: 100, bottom: 900, height: 800 },
    triggerRect: { top: 740, bottom: 1500 },
    containerRect: { bottom: 1500, height: 760 },
    triggerIsContainer: true,
  }), 224);
});

test("expanding the trailing latest disclosure while at the bottom keeps following the bottom", () => {
  assert.equal(
    shouldFollowBottomOnDisclosureExpand({
      followBottom: true,
      intent: "expand",
      wasPinnedBottom: true,
      beforeGap: 0,
    }),
    true,
  );
  assert.equal(
    shouldFollowBottomOnDisclosureExpand({
      followBottom: true,
      intent: "expand",
      wasPinnedBottom: false,
      beforeGap: 12,
    }),
    true,
  );
  // Scrolled up into history -> do not yank to bottom
  assert.equal(
    shouldFollowBottomOnDisclosureExpand({
      followBottom: true,
      intent: "expand",
      wasPinnedBottom: false,
      beforeGap: 180,
    }),
    false,
  );
  // Non-trailing (historical) disclosures or collapses never force bottom follow
  assert.equal(
    shouldFollowBottomOnDisclosureExpand({
      followBottom: false,
      intent: "expand",
      wasPinnedBottom: true,
      beforeGap: 0,
    }),
    false,
  );
  assert.equal(
    shouldFollowBottomOnDisclosureExpand({
      followBottom: true,
      intent: "collapse",
      wasPinnedBottom: true,
      beforeGap: 0,
    }),
    false,
  );
});

test("the side viewport renders peer disclosures with separate actions and full-height open content", async (t) => {
  const cacheDir = await mkdtemp(join(tmpdir(), "orbit-scroll-ssr-"));
  t.after(() => rm(cacheDir, { recursive: true, force: true }));
  const server = await createServer({
    cacheDir,
    server: { middlewareMode: true, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
  });
  try {
    const { PeerMessageRow } = await server.ssrLoadModule("/src/components/stage/subagents-track.tsx");
    const { ConversationViewport } = await server.ssrLoadModule("/src/components/conversation/conversation-viewport.tsx");
    const render = (open: boolean) => renderToStaticMarkup(createElement(ConversationViewport, {
      contentVersion: 1,
      children: createElement(PeerMessageRow, {
        label: "Reviewer", preview: "Summary", avatar: null, open,
        onOpenChange: () => {}, holdViewport: () => assert.fail("render must not scroll"),
        action: createElement("button", null, "Inspect peer"), children: "Expanded report",
      }),
    }));
    const collapsed = render(false);
    assert.match(collapsed, /data-slot="scroll-area-viewport"/);
    assert.match(collapsed, /aria-expanded="false"/);
    assert.match(collapsed, /Summary/);
    assert.doesNotMatch(collapsed, /Expanded report|Jump to latest/);
    const expanded = render(true);
    assert.match(expanded, /aria-expanded="true"/);
    assert.match(expanded, /duration-0/);
    assert.match(expanded, /data-\[starting-style\]:h-auto/);
    assert.match(expanded, /Expanded report/);
    assert.match(expanded, /<\/button><button>Inspect peer<\/button>/);

    // Capture the actual controlled callback with a React dispatcher. A keyboard
    // activation and a pointer click both pass through this Base UI callback.
    const events: string[] = [];
    const holdViewport: ViewportAnchorHoldFn = (_trigger, apply, edge, options) => {
      events.push(`${edge}:${options?.intent}`);
      apply();
    };
    let changeOpen!: (open: boolean) => void;
    function RowProbe() {
      const row: ReactElement<{ onOpenChange: typeof changeOpen }> = PeerMessageRow({
        label: "Reviewer", preview: "Summary", avatar: null, children: "Report", holdViewport,
        onOpenChange: (open: boolean) => events.push(open ? "opened" : "closed"),
      });
      changeOpen = row.props.onOpenChange;
      return row;
    }
    renderToStaticMarkup(createElement(RowProbe));
    changeOpen(true);
    changeOpen(false);
    assert.deepEqual(events, ["top:expand", "opened", "top:collapse", "closed"]);
  } finally {
    await server.close();
  }
});
