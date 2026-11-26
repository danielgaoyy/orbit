export const DRAG_THRESHOLD_SORT_PX = 4;
export const DRAG_THRESHOLD_RESIZE_PX = 2;
export const DRAG_THRESHOLD_PAN_PX = 4;
export const DRAG_SETTLE_DURATION_MS = 140;
export const DRAG_GHOST_FADE_MS = 110;
export const AUTO_SCROLL_EDGE_PX = 36;
export const AUTO_SCROLL_MAX_SPEED_PX = 10;

export interface InlineSortSlot {
  start: number;
  end: number;
  size: number;
  center: number;
  step: number;
}

export function prefersReducedMotion(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return false;
  }
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Deterministically animate an element's CSS `transform` to `targetTransform` over
 * `DRAG_SETTLE_DURATION_MS` (`140ms --ease-out-expo`), resolving on native `transitionend`
 * with a safe fallback timer.
 *
 * Forces one synchronous style flush before applying the transition so WebKit / Chromium never
 * coalesce the start and target transforms into a single paint (which would skip the settle slide
 * and stall until the fallback timer fires).
 */
export function startTransformSettle(
  el: HTMLElement | null,
  targetTransform: string,
  onComplete: () => void,
  durationMs: number = DRAG_SETTLE_DURATION_MS,
): () => void {
  if (!el || prefersReducedMotion()) {
    onComplete();
    return () => {};
  }

  let done = false;
  let timerId: number | null = null;

  const cleanup = () => {
    el.removeEventListener("transitionend", handleTransitionEnd);
    el.removeEventListener("transitioncancel", handleTransitionEnd);
    if (timerId !== null) {
      window.clearTimeout(timerId);
      timerId = null;
    }
  };

  const finish = () => {
    if (done) return;
    done = true;
    cleanup();
    onComplete();
  };

  const handleTransitionEnd = (ev: Event) => {
    const transEv = ev as TransitionEvent;
    if (transEv.target === el && transEv.propertyName === "transform") {
      finish();
    }
  };

  // Lock the current transform as the before-change style so CSS transition always interpolates.
  el.style.transition = "none";
  void el.getBoundingClientRect();

  el.addEventListener("transitionend", handleTransitionEnd);
  el.addEventListener("transitioncancel", handleTransitionEnd);
  el.style.transition = `transform ${durationMs}ms var(--ease-out-expo)`;
  el.style.transform = targetTransform;

  timerId = window.setTimeout(finish, durationMs + 80);

  return () => {
    if (done) return;
    done = true;
    cleanup();
  };
}

/**
 * Axis-agnostic edge auto-scroll velocity calculation.
 * Returns negative pixels/frame near the leading edge, positive near the trailing edge, or 0.
 */
export function computeEdgeScrollVelocity(
  pointerCoord: number,
  viewportStart: number,
  viewportEnd: number,
  edgePx: number = AUTO_SCROLL_EDGE_PX,
  maxSpeedPx: number = AUTO_SCROLL_MAX_SPEED_PX,
): number {
  const span = viewportEnd - viewportStart;
  if (span <= edgePx * 2 || edgePx <= 0) return 0;

  if (pointerCoord < viewportStart + edgePx) {
    const ratio = Math.min(1, Math.max(0, (viewportStart + edgePx - pointerCoord) / edgePx));
    return -Math.ceil(ratio * maxSpeedPx);
  }
  if (pointerCoord > viewportEnd - edgePx) {
    const ratio = Math.min(1, Math.max(0, (pointerCoord - (viewportEnd - edgePx)) / edgePx));
    return Math.ceil(ratio * maxSpeedPx);
  }
  return 0;
}

/**
 * Clamp floating drag ghost coordinates so the preview pill never clips off viewport edges.
 */
export function clampGhostPosition(
  clientX: number,
  clientY: number,
  ghostW: number,
  ghostH: number,
  viewportW: number,
  viewportH: number,
  offsetX: number = 12,
  offsetY: number = -14,
  margin: number = 8,
): { x: number; y: number } {
  const rawX = clientX + offsetX;
  const rawY = clientY + offsetY;
  const maxX = Math.max(margin, viewportW - ghostW - margin);
  const maxY = Math.max(margin, viewportH - ghostH - margin);
  return {
    x: Math.min(maxX, Math.max(margin, rawX)),
    y: Math.min(maxY, Math.max(margin, rawY)),
  };
}

/**
 * Returns true if dropping `sourceId` at `(targetId, position)` within `orderedIds`
 * would leave the list order completely unchanged (self or adjacent no-op boundary).
 */
export function isNoOpLinearDrop(
  orderedIds: readonly string[],
  sourceId: string,
  targetId: string,
  position: "before" | "after",
): boolean {
  if (!sourceId || !targetId || sourceId === targetId) return true;
  const srcIdx = orderedIds.indexOf(sourceId);
  const dstIdx = orderedIds.indexOf(targetId);
  if (srcIdx < 0 || dstIdx < 0) return false;
  if (position === "after" && dstIdx === srcIdx - 1) return true;
  if (position === "before" && dstIdx === srcIdx + 1) return true;
  return false;
}

/**
 * Build content-relative slot descriptors for inline 1:1 sorting (supports variable sizes & gaps).
 */
export function buildInlineSortSlots(
  rects: ReadonlyArray<{ start: number; size: number }>,
): InlineSortSlot[] {
  const count = rects.length;
  if (count === 0) return [];

  let gapSum = 0;
  let gapCount = 0;
  for (let i = 0; i < count - 1; i += 1) {
    const gap = rects[i + 1].start - (rects[i].start + rects[i].size);
    if (Number.isFinite(gap) && gap >= 0) {
      gapSum += gap;
      gapCount += 1;
    }
  }
  const avgGap = gapCount > 0 ? gapSum / gapCount : 0;

  return rects.map((r, i) => {
    const end = r.start + r.size;
    const step =
      i < count - 1
        ? rects[i + 1].start - r.start
        : count > 1
          ? end - (rects[i - 1].start + rects[i - 1].size)
          : r.size + avgGap;
    return {
      start: r.start,
      end,
      size: r.size,
      center: r.start + r.size * 0.5,
      step: Math.max(1, step),
    };
  });
}

/**
 * Clamp live drag delta so the dragged item stays within the leading and trailing edges of the track.
 */
export function clampInlineSortDelta(
  slots: readonly InlineSortSlot[],
  fromIndex: number,
  rawDelta: number,
): number {
  if (slots.length < 2 || fromIndex < 0 || fromIndex >= slots.length) return 0;
  const fromSlot = slots[fromIndex];
  const minDelta = slots[0].start - fromSlot.start;
  const maxDelta = slots[slots.length - 1].end - fromSlot.end;
  return Math.max(minDelta, Math.min(maxDelta, rawDelta));
}

/**
 * Monotone midpoint-crossover target index for variable-width / variable-height inline sorting.
 * Uses trailing edge when moving forward and leading edge when moving backward so both
 * narrow-over-wide and wide-over-narrow swaps trigger smoothly without oscillation.
 *
 * When `maxLeadInPx` is provided (e.g. variable-height expanded Sidebar Projects), tall neighbors
 * swap once the dragged item enters `min(slot.size * 0.5, maxLeadInPx)` instead of requiring
 * dragging past the full midpoint of a 200px+ expanded section.
 */
export function computeInlineSortTargetIndex(
  slots: readonly InlineSortSlot[],
  fromIndex: number,
  delta: number,
  maxLeadInPx?: number,
): number {
  const count = slots.length;
  if (count < 2 || fromIndex < 0 || fromIndex >= count) return fromIndex;
  const fromSlot = slots[fromIndex];

  if (delta > 0) {
    const trailingEdge = fromSlot.end + delta;
    let target = fromIndex;
    for (let i = fromIndex + 1; i < count; i += 1) {
      const threshold =
        maxLeadInPx !== undefined
          ? slots[i].start + Math.min(slots[i].size * 0.5, maxLeadInPx)
          : slots[i].center;
      if (trailingEdge >= threshold) {
        target = i;
      } else {
        break;
      }
    }
    return target;
  }

  if (delta < 0) {
    const leadingEdge = fromSlot.start + delta;
    let target = fromIndex;
    for (let i = fromIndex - 1; i >= 0; i -= 1) {
      const threshold =
        maxLeadInPx !== undefined
          ? slots[i].end - Math.min(slots[i].size * 0.5, maxLeadInPx)
          : slots[i].center;
      if (leadingEdge <= threshold) {
        target = i;
      } else {
        break;
      }
    }
    return target;
  }

  return fromIndex;
}

/**
 * Exact settle translation for the dragged item landing at `targetIndex`.
 */
export function computeInlineSortSettleDelta(
  slots: readonly InlineSortSlot[],
  fromIndex: number,
  targetIndex: number,
): number {
  if (
    slots.length < 2 ||
    fromIndex === targetIndex ||
    fromIndex < 0 ||
    targetIndex < 0 ||
    fromIndex >= slots.length ||
    targetIndex >= slots.length
  ) {
    return 0;
  }
  if (targetIndex > fromIndex) {
    return slots[targetIndex].end - slots[fromIndex].end;
  }
  return slots[targetIndex].start - slots[fromIndex].start;
}

/**
 * Exact translation for a non-dragged neighbor at `itemIndex` while `fromIndex` hovers `overIndex`.
 */
export function computeInlineSortNeighborOffset(
  slots: readonly InlineSortSlot[],
  fromIndex: number,
  overIndex: number,
  itemIndex: number,
): number {
  if (
    slots.length < 2 ||
    itemIndex === fromIndex ||
    fromIndex === overIndex ||
    fromIndex < 0 ||
    fromIndex >= slots.length
  ) {
    return 0;
  }

  if (fromIndex < overIndex && itemIndex > fromIndex && itemIndex <= overIndex) {
    const shift =
      fromIndex + 1 < slots.length
        ? slots[fromIndex + 1].start - slots[fromIndex].start
        : slots[fromIndex].step;
    return -shift;
  }

  if (fromIndex > overIndex && itemIndex >= overIndex && itemIndex < fromIndex) {
    const shift =
      fromIndex - 1 >= 0
        ? slots[fromIndex].end - slots[fromIndex - 1].end
        : slots[fromIndex].step;
    return shift;
  }

  return 0;
}

/**
 * Clamp pan offset for zoomed images/diagrams so content never drifts completely outside the viewport.
 */
export function clampPanOffset(params: {
  panX: number;
  panY: number;
  contentW: number;
  contentH: number;
  viewportW: number;
  viewportH: number;
  zoom: number;
  allowPanWhenFits?: boolean;
  edgeSlackPx?: number;
}): { x: number; y: number } {
  const {
    panX,
    panY,
    contentW,
    contentH,
    viewportW,
    viewportH,
    zoom,
    allowPanWhenFits = false,
    edgeSlackPx = 48,
  } = params;

  if (contentW <= 0 || contentH <= 0 || viewportW <= 0 || viewportH <= 0 || zoom <= 0) {
    return { x: panX, y: panY };
  }

  const clampAxis = (pan: number, contentSpan: number, viewportSpan: number): number => {
    const scaled = contentSpan * zoom;
    let maxPan: number;
    if (scaled > viewportSpan) {
      maxPan = (scaled - viewportSpan) * 0.5 + edgeSlackPx;
    } else if (allowPanWhenFits) {
      const minVisible = Math.min(120, scaled * 0.4);
      maxPan = Math.max(0, (viewportSpan + scaled) * 0.5 - minVisible);
    } else {
      maxPan = Math.min((viewportSpan - scaled) * 0.5, scaled * 0.35);
    }
    return Math.min(maxPan, Math.max(-maxPan, pan));
  };

  return {
    x: clampAxis(panX, contentW, viewportW),
    y: clampAxis(panY, contentH, viewportH),
  };
}

/**
 * Merge a reordered visible subset (`visibleOrderedIds`) back into a full saved order
 * (`previousOrder`) so temporarily hidden items (e.g. a pinned thread promoted into Needs Attention)
 * keep their original relative slots instead of being dropped or pushed to the bottom.
 */
export function mergeSubsetOrder(
  previousOrder: readonly string[] | undefined,
  visibleOrderedIds: readonly string[],
): string[] {
  if (!previousOrder || previousOrder.length === 0) {
    return [...visibleOrderedIds];
  }
  const visibleSet = new Set(visibleOrderedIds);
  const placed = new Set<string>();
  const result: string[] = [];
  let visibleCursor = 0;

  for (let i = 0; i < previousOrder.length; i += 1) {
    const id = previousOrder[i];
    if (visibleSet.has(id)) {
      if (visibleCursor < visibleOrderedIds.length) {
        const nextId = visibleOrderedIds[visibleCursor++];
        result.push(nextId);
        placed.add(nextId);
      }
    } else if (!placed.has(id)) {
      result.push(id);
      placed.add(id);
    }
  }

  while (visibleCursor < visibleOrderedIds.length) {
    const nextId = visibleOrderedIds[visibleCursor++];
    if (!placed.has(nextId)) {
      result.push(nextId);
      placed.add(nextId);
    }
  }

  return result;
}

/**
 * Strict check that a drag event carries real OS files rather than selected text or links.
 */
export function hasDraggedFiles(
  dataTransfer:
    | {
        types?: ArrayLike<string> | null;
        items?: ArrayLike<{ kind?: string }> | null;
        files?: ArrayLike<unknown> | null;
      }
    | null
    | undefined,
): boolean {
  if (!dataTransfer) return false;
  if (dataTransfer.files && dataTransfer.files.length > 0) return true;
  if (dataTransfer.types) {
    for (let i = 0; i < dataTransfer.types.length; i += 1) {
      if (dataTransfer.types[i] === "Files") return true;
    }
  }
  if (dataTransfer.items) {
    for (let i = 0; i < dataTransfer.items.length; i += 1) {
      if (dataTransfer.items[i]?.kind === "file") return true;
    }
  }
  return false;
}
