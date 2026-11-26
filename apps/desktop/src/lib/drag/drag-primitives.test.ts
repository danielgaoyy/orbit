import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, it } from "node:test";
import {
  buildInlineSortSlots,
  clampGhostPosition,
  clampInlineSortDelta,
  clampPanOffset,
  computeEdgeScrollVelocity,
  computeInlineSortNeighborOffset,
  computeInlineSortSettleDelta,
  computeInlineSortTargetIndex,
  hasDraggedFiles,
  isNoOpLinearDrop,
  mergeSubsetOrder,
  startTransformSettle,
} from "./drag-constants.ts";

describe("drag-constants pure primitives", () => {
  it("computes symmetric edge auto-scroll velocities near leading and trailing edges", () => {
    assert.equal(computeEdgeScrollVelocity(150, 0, 300, 36, 10), 0);
    assert.equal(computeEdgeScrollVelocity(36, 0, 300, 36, 10), 0);
    assert.equal(computeEdgeScrollVelocity(264, 0, 300, 36, 10), 0);
    assert.equal(computeEdgeScrollVelocity(0, 0, 300, 36, 10), -10);
    assert.equal(computeEdgeScrollVelocity(300, 0, 300, 36, 10), 10);
    assert.equal(computeEdgeScrollVelocity(18, 0, 300, 36, 10), -5);
    // Viewport too small for two edge bands returns 0
    assert.equal(computeEdgeScrollVelocity(10, 0, 60, 36, 10), 0);
  });

  it("clamps floating ghost pill within viewport margins", () => {
    const inside = clampGhostPosition(100, 200, 180, 32, 1200, 800);
    assert.deepEqual(inside, { x: 112, y: 186 });

    const bottomRight = clampGhostPosition(1190, 795, 180, 32, 1200, 800);
    assert.deepEqual(bottomRight, { x: 1012, y: 760 });

    const topLeft = clampGhostPosition(2, 4, 180, 32, 1200, 800);
    assert.deepEqual(topLeft, { x: 14, y: 8 });
  });

  it("suppresses self and adjacent no-op linear drop indicators", () => {
    const ids = ["a", "b", "c", "d"];
    assert.equal(isNoOpLinearDrop(ids, "b", "b", "before"), true);
    assert.equal(isNoOpLinearDrop(ids, "b", "b", "after"), true);
    assert.equal(isNoOpLinearDrop(ids, "b", "a", "after"), true);
    assert.equal(isNoOpLinearDrop(ids, "b", "c", "before"), true);

    assert.equal(isNoOpLinearDrop(ids, "b", "a", "before"), false);
    assert.equal(isNoOpLinearDrop(ids, "b", "c", "after"), false);
    assert.equal(isNoOpLinearDrop(ids, "b", "d", "before"), false);
    assert.equal(isNoOpLinearDrop(ids, "b", "d", "after"), false);
  });

  it("computes exact variable-width inline sort targets, neighbor shifts, and settle deltas", () => {
    // Tabs: A=[0,80], B=[82,202], C=[204,264] (gap = 2px)
    const slots = buildInlineSortSlots([
      { start: 0, size: 80 },
      { start: 82, size: 120 },
      { start: 204, size: 60 },
    ]);

    // Drag A right: A.end = 80, B.center = 142 -> crosses at delta >= 62
    assert.equal(computeInlineSortTargetIndex(slots, 0, 61), 0);
    assert.equal(computeInlineSortTargetIndex(slots, 0, 62), 1);
    // C.center = 234 -> crosses at delta >= 154
    assert.equal(computeInlineSortTargetIndex(slots, 0, 154), 2);

    // Neighbor shifts when A (0) hovers over index 1 (B): B shifts left by 82px -> [0, 120]
    assert.equal(computeInlineSortNeighborOffset(slots, 0, 1, 1), -82);
    assert.equal(computeInlineSortNeighborOffset(slots, 0, 1, 2), 0);
    // A settles at [122, 202] => delta = 122
    assert.equal(computeInlineSortSettleDelta(slots, 0, 1), 122);
    // A settles at end [184, 264] => delta = 184
    assert.equal(computeInlineSortSettleDelta(slots, 0, 2), 184);

    // Drag C (index 2, width 60) left before A (index 0):
    // B.center = 142 -> C.start(204) + delta <= 142 => delta <= -62
    assert.equal(computeInlineSortTargetIndex(slots, 2, -62), 1);
    // A.center = 40 -> C.start(204) + delta <= 40 => delta <= -164
    assert.equal(computeInlineSortTargetIndex(slots, 2, -164), 0);

    // Both A and B shift right by 62px (C.width + gap)
    assert.equal(computeInlineSortNeighborOffset(slots, 2, 0, 0), 62);
    assert.equal(computeInlineSortNeighborOffset(slots, 2, 0, 1), 62);
    // C settles at [0, 60] => delta = -204
    assert.equal(computeInlineSortSettleDelta(slots, 2, 0), -204);

    // Clamp keeps dragged item within track bounds
    assert.equal(clampInlineSortDelta(slots, 0, -50), 0);
    assert.equal(clampInlineSortDelta(slots, 0, 500), 184);
    assert.equal(clampInlineSortDelta(slots, 2, -500), -204);
    assert.equal(clampInlineSortDelta(slots, 2, 50), 0);
  });

  it("supports capped maxLeadInPx for variable-height collapsed and expanded project sections", () => {
    // Project A (collapsed 30px) at [0, 30], Project B (expanded 220px) at [32, 252], Project C (collapsed 30px) at [254, 284]
    const slots = buildInlineSortSlots([
      { start: 0, size: 30 },
      { start: 32, size: 220 },
      { start: 254, size: 30 },
    ]);

    // Drag collapsed A downward past expanded B:
    // Without cap, midpoint of B is 142 (requires 112px drag). With maxLeadInPx=56, threshold is 32 + 56 = 88 -> delta >= 58.
    assert.equal(computeInlineSortTargetIndex(slots, 0, 57, 56), 0);
    assert.equal(computeInlineSortTargetIndex(slots, 0, 58, 56), 1);
    // Project C has size 30 -> min(15, 56) = 15, threshold is 254 + 15 = 269 -> delta >= 239.
    assert.equal(computeInlineSortTargetIndex(slots, 0, 238, 56), 1);
    assert.equal(computeInlineSortTargetIndex(slots, 0, 239, 56), 2);

    // Drag collapsed C upward past expanded B:
    // Threshold from bottom of B is 252 - 56 = 196 -> C.start(254) + delta <= 196 => delta <= -58.
    assert.equal(computeInlineSortTargetIndex(slots, 2, -57, 56), 2);
    assert.equal(computeInlineSortTargetIndex(slots, 2, -58, 56), 1);
    // Project A has size 30 -> threshold from bottom of A is 30 - 15 = 15 -> delta <= -239.
    assert.equal(computeInlineSortTargetIndex(slots, 2, -238, 56), 1);
    assert.equal(computeInlineSortTargetIndex(slots, 2, -239, 56), 0);
  });

  it("clamps pan offsets cleanly for zoomed images and diagrams", () => {
    // 1000x800 image at 2x inside 1000x800 viewport -> max pan = (2000-1000)/2 + 48 = 548, y = 448
    const clampedZoomed = clampPanOffset({
      panX: 9999,
      panY: -9999,
      contentW: 1000,
      contentH: 800,
      viewportW: 1000,
      viewportH: 800,
      zoom: 2,
    });
    assert.deepEqual(clampedZoomed, { x: 548, y: -448 });

    // Small diagram at 1x inside 1000x800 viewport (allowPanWhenFits = true)
    const clampedDiagram = clampPanOffset({
      panX: 9999,
      panY: -9999,
      contentW: 400,
      contentH: 300,
      viewportW: 1000,
      viewportH: 800,
      zoom: 1,
      allowPanWhenFits: true,
    });
    assert.equal(clampedDiagram.x > 0 && clampedDiagram.x < 700, true);
    assert.equal(clampedDiagram.y < 0 && clampedDiagram.y > -550, true);
  });

  it("strictly identifies file drags and ignores text/URL drags", () => {
    assert.equal(hasDraggedFiles(null), false);
    assert.equal(hasDraggedFiles({ types: ["text/plain", "text/uri-list"] }), false);
    assert.equal(hasDraggedFiles({ types: ["Files"] }), true);
    assert.equal(hasDraggedFiles({ items: [{ kind: "string" }] }), false);
    assert.equal(hasDraggedFiles({ items: [{ kind: "file" }] }), true);
    assert.equal(hasDraggedFiles({ files: [{}] }), true);
  });

  it("merges reordered visible subset while preserving hidden pinned slots", () => {
    assert.deepEqual(mergeSubsetOrder(undefined, ["c", "a"]), ["c", "a"]);
    // `b` is temporarily promoted into Needs Attention; reordering [a, c, d] -> [d, a, c] keeps `b` in slot 2
    assert.deepEqual(mergeSubsetOrder(["a", "b", "c", "d"], ["d", "a", "c"]), ["d", "b", "a", "c"]);
    // Newly pinned item `e` not yet in previousOrder appends cleanly
    assert.deepEqual(mergeSubsetOrder(["a", "b", "c"], ["c", "a", "e"]), ["c", "b", "a", "e"]);
  });

  it("forces a synchronous style flush before transform settle and resolves cleanly on transitionend", () => {
    const listeners = new Map<string, Set<(ev: Event) => void>>();
    const styleLog: string[] = [];
    let layoutFlushes = 0;
    const style = {
      _transition: "",
      _transform: "translate3d(0, 12px, 0)",
      get transition() {
        return this._transition;
      },
      set transition(v: string) {
        this._transition = v;
        styleLog.push(`transition:${v}`);
      },
      get transform() {
        return this._transform;
      },
      set transform(v: string) {
        this._transform = v;
        styleLog.push(`transform:${v}`);
      },
    };
    const fakeEl = {
      style,
      getBoundingClientRect() {
        layoutFlushes += 1;
        styleLog.push("flush");
        return { left: 0, top: 0, width: 100, height: 30 };
      },
      addEventListener(type: string, fn: (ev: Event) => void) {
        if (!listeners.has(type)) listeners.set(type, new Set());
        listeners.get(type)!.add(fn);
      },
      removeEventListener(type: string, fn: (ev: Event) => void) {
        listeners.get(type)?.delete(fn);
      },
    } as unknown as HTMLElement;

    const prevWindow = globalThis.window;
    let clearedTimer = false;
    (globalThis as unknown as { window: unknown }).window = {
      matchMedia: () => ({ matches: false }),
      setTimeout: () => 42,
      clearTimeout: () => {
        clearedTimer = true;
      },
    };

    try {
      let completions = 0;
      const cancel = startTransformSettle(fakeEl, "translate3d(0, 60px, 0)", () => {
        completions += 1;
      });

      assert.equal(layoutFlushes, 1);
      assert.deepEqual(styleLog, [
        "transition:none",
        "flush",
        "transition:transform 140ms var(--ease-out-expo)",
        "transform:translate3d(0, 60px, 0)",
      ]);

      // Unrelated property or bubbled child transition must be ignored
      for (const fn of listeners.get("transitionend") ?? []) {
        fn({ target: fakeEl, propertyName: "opacity" } as unknown as Event);
        fn({ target: {}, propertyName: "transform" } as unknown as Event);
      }
      assert.equal(completions, 0);

      // Matching transform transitionend completes once and detaches listeners
      for (const fn of [...(listeners.get("transitionend") ?? [])]) {
        fn({ target: fakeEl, propertyName: "transform" } as unknown as Event);
      }
      assert.equal(completions, 1);
      assert.equal(clearedTimer, true);
      assert.equal(listeners.get("transitionend")?.size ?? 0, 0);

      // Subsequent cancel is a safe no-op
      cancel();
      assert.equal(completions, 1);
    } finally {
      (globalThis as unknown as { window: unknown }).window = prevWindow;
    }
  });

  it("commits drag reorders atomically via flushSync to prevent SyncLane/DefaultLane frame tearing", () => {
    const inlineSrc = readFileSync(
      path.join(import.meta.dirname, "use-inline-sortable.ts"),
      "utf8",
    );
    const sidebarSrc = readFileSync(
      path.join(import.meta.dirname, "../../components/sidebar/use-sidebar-drag.ts"),
      "utf8",
    );
    assert.match(inlineSrc, /import\s*\{\s*flushSync\s*\}\s*from\s*"react-dom"/);
    assert.match(inlineSrc, /flushSync\(\(\)\s*=>\s*\{[\s\S]*onCommitRef\.current[\s\S]*setSortState\(null\)/);
    assert.match(sidebarSrc, /import\s*\{\s*flushSync\s*\}\s*from\s*"react-dom"/);
    assert.match(sidebarSrc, /flushSync\(\(\)\s*=>\s*\{[\s\S]*reorderThreads[\s\S]*clearThreadVisuals\(\)/);
    assert.match(sidebarSrc, /flushSync\(\(\)\s*=>\s*\{[\s\S]*reorderProjects[\s\S]*setProjectSortState\(null\)/);
    assert.match(sidebarSrc, /flushSync\(\(\)\s*=>\s*\{[\s\S]*reorderPinnedThreads[\s\S]*setPinnedSortState\(null\)/);
  });
});
