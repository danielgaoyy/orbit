import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import {
  DRAG_THRESHOLD_SORT_PX,
  type InlineSortSlot,
  buildInlineSortSlots,
  clampInlineSortDelta,
  computeInlineSortNeighborOffset,
  computeInlineSortSettleDelta,
  computeInlineSortTargetIndex,
  prefersReducedMotion,
  startTransformSettle,
} from "./drag-constants";
import { useEdgeAutoScroll } from "./use-edge-auto-scroll";

export interface InlineSortState {
  phase: "dragging" | "settling";
  fromIndex: number;
  overIndex: number;
  slots: readonly InlineSortSlot[];
  settleDelta: number;
}

export interface UseInlineSortableOptions {
  axis: "x" | "y";
  itemIds: readonly string[];
  scrollViewportRef?: React.RefObject<HTMLElement | null>;
  ignoreSelector?: string;
  thresholdPx?: number;
  onCommit: (fromIndex: number, toIndex: number) => void;
}

/**
 * Unified Plan-A inline 1:1 pointer sortable engine (vertical lists & horizontal variable-width tabs).
 *
 * Key invariants:
 * - Zero React re-renders per pixel: dragged element tracks pointer via direct DOM `translate3d`.
 * - Neighbors shift smoothly via CSS `transition-transform` only when `overIndex` changes.
 * - Settles into exact target slot (`140ms --ease-out-expo`) before committing reorder, honoring `prefers-reduced-motion`.
 * - Full safety: `Escape` / `pointercancel` / `window.blur` smooth rollback, unmount cleanup,
 *   body `grabbing` cursor lock, DragShield state, click suppression, and edge auto-scroll with live hit refresh.
 */
export function useInlineSortable({
  axis,
  itemIds,
  scrollViewportRef,
  ignoreSelector = "button, input, [data-no-drag]",
  thresholdPx = DRAG_THRESHOLD_SORT_PX,
  onCommit,
}: UseInlineSortableOptions) {
  const [sortState, setSortState] = useState<InlineSortState | null>(null);
  const itemElsRef = useRef<Map<string, HTMLElement>>(new Map());
  const itemIdsRef = useRef<readonly string[]>(itemIds);
  itemIdsRef.current = itemIds;
  const draggedDomElRef = useRef<HTMLElement | null>(null);
  const cleanupRef = useRef<(() => void) | null>(null);
  const settleFinishRef = useRef<(() => void) | null>(null);
  const settleCancelRef = useRef<(() => void) | null>(null);
  const suppressClickRef = useRef(false);
  const suppressTimerRef = useRef<number | null>(null);
  const onCommitRef = useRef(onCommit);
  onCommitRef.current = onCommit;
  const activeAutoScrollRefreshRef = useRef<((clientX: number, clientY: number) => void) | null>(
    null,
  );

  const fallbackScrollRef = useRef<HTMLElement | null>(null);
  const effectiveScrollRef = scrollViewportRef ?? fallbackScrollRef;

  const { update: updateAutoScroll, stop: stopAutoScroll } = useEdgeAutoScroll({
    viewportRef: effectiveScrollRef,
    axis,
    onScrollTick: (clientX, clientY) => {
      activeAutoScrollRefreshRef.current?.(clientX, clientY);
    },
  });
  const stopAutoScrollRef = useRef(stopAutoScroll);
  stopAutoScrollRef.current = stopAutoScroll;

  const itemSignature = useMemo(() => itemIds.join("\0"), [itemIds]);

  const clearSettle = useCallback(() => {
    settleCancelRef.current?.();
    settleCancelRef.current = null;
    settleFinishRef.current = null;
  }, []);

  const clearSuppressTimer = useCallback(() => {
    if (suppressTimerRef.current !== null) {
      window.clearTimeout(suppressTimerRef.current);
      suppressTimerRef.current = null;
    }
  }, []);

  // Clear residual direct DOM transition + transform inside the same synchronous `flushSync` commit
  // that applies the reordered DOM order and `sortState === null`, guaranteeing 0-frame flicker.
  useLayoutEffect(() => {
    if (sortState !== null) return;
    if (draggedDomElRef.current) {
      draggedDomElRef.current.style.transition = "";
      draggedDomElRef.current.style.transform = "";
      draggedDomElRef.current = null;
    }
  }, [sortState]);

  // Abort cleanly if external list changes mid-drag
  useEffect(() => {
    cleanupRef.current?.();
  }, [itemSignature]);

  // Unmount-only cleanup (never depend on changing objects so active drags cannot be aborted on re-render)
  useEffect(() => {
    return () => {
      cleanupRef.current?.();
      clearSettle();
      clearSuppressTimer();
      stopAutoScrollRef.current();
    };
  }, [clearSettle, clearSuppressTimer]);

  const registerItem = useCallback((id: string, el: HTMLElement | null) => {
    if (el) {
      itemElsRef.current.set(id, el);
    } else {
      itemElsRef.current.delete(id);
    }
  }, []);

  const shouldSuppressClick = useCallback(() => suppressClickRef.current, []);

  const handleTransitionEnd = useCallback(
    (_index: number, _e: React.TransitionEvent<HTMLElement>) => {
      // Settle completion is handled deterministically via native `transitionend` in `startTransformSettle`.
    },
    [],
  );

  const handlePointerDown = useCallback(
    (e: React.PointerEvent<HTMLElement>, fromIndex: number) => {
      const initialIds = itemIdsRef.current;
      if (e.button !== 0 || initialIds.length < 2 || fromIndex < 0 || fromIndex >= initialIds.length) {
        return;
      }
      if (ignoreSelector) {
        const target = e.target as HTMLElement | null;
        if (target?.closest(ignoreSelector)) return;
      }

      const clickedId = initialIds[fromIndex];

      // Flush any in-flight settle synchronously before measuring the next drag
      if (settleFinishRef.current) {
        settleFinishRef.current();
      }
      cleanupRef.current?.();
      clearSuppressTimer();

      const currentIds = itemIdsRef.current;
      const count = currentIds.length;
      const resolvedFromIndex = clickedId ? currentIds.indexOf(clickedId) : -1;
      if (count < 2 || resolvedFromIndex < 0) return;

      const fromId = currentIds[resolvedFromIndex];
      const viewport = effectiveScrollRef.current;
      const viewportRect = viewport?.getBoundingClientRect() ?? null;
      const startScroll = viewport ? (axis === "x" ? viewport.scrollLeft : viewport.scrollTop) : 0;

      const rawRects: Array<{ start: number; size: number }> = [];
      for (let i = 0; i < count; i += 1) {
        const el = itemElsRef.current.get(currentIds[i]);
        if (!el) return;
        const rect = el.getBoundingClientRect();
        if (axis === "x") {
          const origin = viewportRect ? rect.left - viewportRect.left + startScroll : rect.left;
          rawRects.push({ start: origin, size: rect.width });
        } else {
          const origin = viewportRect ? rect.top - viewportRect.top + startScroll : rect.top;
          rawRects.push({ start: origin, size: rect.height });
        }
      }

      const slots = buildInlineSortSlots(rawRects);
      if (slots.length !== count) return;

      const pointerId = e.pointerId;
      const isTouchPointer = e.pointerType === "touch";
      const startCoord = axis === "x" ? e.clientX : e.clientY;
      let active = false;
      let overIndex = resolvedFromIndex;
      let latestDelta = 0;
      let prevBodyCursor = "";

      const formatTransform = (delta: number) =>
        axis === "x" ? `translate3d(${delta}px, 0, 0)` : `translate3d(0, ${delta}px, 0)`;

      const applyDraggedTransform = (delta: number) => {
        const el = itemElsRef.current.get(fromId);
        if (!el) return;
        draggedDomElRef.current = el;
        el.style.transform = formatTransform(delta);
      };

      const clearDraggedInlineTransform = () => {
        const el = draggedDomElRef.current ?? itemElsRef.current.get(fromId);
        if (el) {
          el.style.transition = "";
          el.style.transform = "";
        }
        draggedDomElRef.current = null;
      };

      const onDragStart = (ev: DragEvent) => {
        ev.preventDefault();
      };

      const detachListeners = () => {
        window.removeEventListener("pointermove", onMove, true);
        window.removeEventListener("pointerup", onUp, true);
        window.removeEventListener("pointercancel", onCancel, true);
        window.removeEventListener("dragstart", onDragStart, true);
        window.removeEventListener("blur", onCancel);
        window.removeEventListener("keydown", onKeyDown, true);
        activeAutoScrollRefreshRef.current = null;
        stopAutoScroll();
      };

      const cleanup = () => {
        detachListeners();
        clearSettle();
        if (active) {
          document.body.style.cursor = prevBodyCursor;
        }
        clearDraggedInlineTransform();
        if (cleanupRef.current === cleanup) {
          cleanupRef.current = null;
        }
        setSortState(null);
      };

      const armClickSuppression = () => {
        suppressClickRef.current = true;
        clearSuppressTimer();
        suppressTimerRef.current = window.setTimeout(() => {
          suppressTimerRef.current = null;
          suppressClickRef.current = false;
        }, 80);
      };

      const startSettle = (targetOverIndex: number, shouldCommit: boolean) => {
        detachListeners();
        if (active) {
          document.body.style.cursor = prevBodyCursor;
        }
        armClickSuppression();

        const targetDelta = computeInlineSortSettleDelta(slots, resolvedFromIndex, targetOverIndex);
        const reduced = prefersReducedMotion();

        let finished = false;
        const finalize = () => {
          if (finished) return;
          finished = true;
          clearSettle();
          cleanupRef.current = null;
          // Commit external store reorder + local sort reset in the exact same React SyncLane
          // render pass so `useSyncExternalStore` never paints an intermediate frame with stale transforms.
          flushSync(() => {
            if (shouldCommit && targetOverIndex !== resolvedFromIndex) {
              onCommitRef.current(resolvedFromIndex, targetOverIndex);
            }
            setSortState(null);
          });
        };

        const draggedEl = draggedDomElRef.current ?? itemElsRef.current.get(fromId) ?? null;
        if (reduced || !draggedEl || Math.abs(latestDelta - targetDelta) < 0.5) {
          finalize();
          return;
        }

        clearSettle();
        settleFinishRef.current = finalize;
        const abortSettle = () => {
          if (finished) return;
          finished = true;
          clearSettle();
          cleanupRef.current = null;
          clearDraggedInlineTransform();
          setSortState(null);
        };
        cleanupRef.current = abortSettle;

        setSortState({
          phase: "settling",
          fromIndex: resolvedFromIndex,
          overIndex: targetOverIndex,
          slots,
          settleDelta: targetDelta,
        });

        draggedDomElRef.current = draggedEl;
        settleCancelRef.current = startTransformSettle(
          draggedEl,
          formatTransform(targetDelta),
          finalize,
        );
      };

      const updateAtPointer = (clientX: number, clientY: number) => {
        const currentScroll = viewport
          ? axis === "x"
            ? viewport.scrollLeft
            : viewport.scrollTop
          : 0;
        const rawDelta = (axis === "x" ? clientX : clientY) - startCoord + (currentScroll - startScroll);
        const clamped = clampInlineSortDelta(slots, resolvedFromIndex, rawDelta);
        latestDelta = clamped;
        applyDraggedTransform(clamped);

        const nextOver = computeInlineSortTargetIndex(slots, resolvedFromIndex, clamped);
        if (nextOver !== overIndex) {
          overIndex = nextOver;
          setSortState({
            phase: "dragging",
            fromIndex: resolvedFromIndex,
            overIndex: nextOver,
            slots,
            settleDelta: 0,
          });
        }
      };

      const onMove = (ev: PointerEvent) => {
        if (isTouchPointer && ev.pointerId !== pointerId) return;
        if (!isTouchPointer && ev.pointerType === "mouse" && ev.buttons === 0) {
          if (!active) {
            cleanup();
          } else {
            startSettle(overIndex, overIndex !== resolvedFromIndex);
          }
          return;
        }
        const coord = axis === "x" ? ev.clientX : ev.clientY;
        if (!active) {
          if (Math.abs(coord - startCoord) < thresholdPx) return;
          active = true;
          suppressClickRef.current = true;
          prevBodyCursor = document.body.style.cursor;
          document.body.style.cursor = "grabbing";
          activeAutoScrollRefreshRef.current = updateAtPointer;
          setSortState({
            phase: "dragging",
            fromIndex: resolvedFromIndex,
            overIndex: resolvedFromIndex,
            slots,
            settleDelta: 0,
          });
        }

        if (viewport) {
          updateAutoScroll(ev.clientX, ev.clientY);
        }
        updateAtPointer(ev.clientX, ev.clientY);
      };

      const onUp = (ev: PointerEvent) => {
        if (isTouchPointer && ev.pointerId !== pointerId) return;
        if (!active) {
          cleanup();
          return;
        }
        startSettle(overIndex, overIndex !== resolvedFromIndex);
      };

      const onCancel = (ev?: Event) => {
        if (isTouchPointer && ev instanceof PointerEvent && ev.pointerId !== pointerId) return;
        if (!active) {
          cleanup();
          return;
        }
        startSettle(resolvedFromIndex, false);
      };

      const onKeyDown = (ev: KeyboardEvent) => {
        if (ev.key === "Escape") {
          if (active) {
            ev.preventDefault();
            ev.stopPropagation();
            startSettle(resolvedFromIndex, false);
          } else {
            cleanup();
          }
        }
      };

      cleanupRef.current = cleanup;
      window.addEventListener("pointermove", onMove, true);
      window.addEventListener("pointerup", onUp, true);
      window.addEventListener("pointercancel", onCancel, true);
      window.addEventListener("dragstart", onDragStart, true);
      window.addEventListener("blur", onCancel);
      window.addEventListener("keydown", onKeyDown, true);
    },
    [
      axis,
      ignoreSelector,
      thresholdPx,
      effectiveScrollRef,
      updateAutoScroll,
      stopAutoScroll,
      clearSettle,
      clearSuppressTimer,
    ],
  );

  const getNeighborStyle = useCallback(
    (index: number): React.CSSProperties | undefined => {
      if (!sortState || sortState.fromIndex === index) return undefined;
      const offset = computeInlineSortNeighborOffset(
        sortState.slots,
        sortState.fromIndex,
        sortState.overIndex,
        index,
      );
      return {
        transform:
          axis === "x" ? `translate3d(${offset}px, 0, 0)` : `translate3d(0, ${offset}px, 0)`,
      };
    },
    [axis, sortState],
  );

  return {
    sortState,
    isDragging: sortState?.phase === "dragging",
    isActive: sortState !== null,
    registerItem,
    handlePointerDown,
    handleTransitionEnd,
    getNeighborStyle,
    shouldSuppressClick,
  };
}
