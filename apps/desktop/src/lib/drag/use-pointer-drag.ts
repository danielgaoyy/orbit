import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DRAG_THRESHOLD_SORT_PX } from "./drag-constants";

export interface PointerDragFrame {
  startX: number;
  startY: number;
  clientX: number;
  clientY: number;
  deltaX: number;
  deltaY: number;
  event: PointerEvent;
}

export interface StartPointerDragOptions {
  /** Minimum pointer travel in pixels before activating drag. Default: 4px. */
  thresholdPx?: number;
  /** Axis used for threshold measurement. Default: "both". */
  thresholdAxis?: "x" | "y" | "both";
  /** Cursor locked onto `document.body` while actively dragging. */
  cursor?: string;
  /** Prevent default on pointerdown (recommended for sashes/resizers). */
  preventDefaultOnDown?: boolean;
  /** Ignore double-click presses (`e.detail >= 2`) so `onDoubleClick` fires cleanly. */
  ignoreDoubleClick?: boolean;
  /** Ignore presses originating inside elements matching this CSS selector. */
  ignoreSelector?: string;
  /** Coalesce `onMove` callbacks into `requestAnimationFrame`. */
  raf?: boolean;
  /** Called once when pointer travel crosses threshold. Return `false` to abort. */
  onActivate?: (frame: PointerDragFrame) => boolean | void;
  /** Called on every pointermove (or rAF frame when `raf: true`) while active. */
  onMove?: (frame: PointerDragFrame) => void;
  /** Called when the user releases the pointer normally after activation. */
  onCommit?: (frame: PointerDragFrame | null) => void;
  /** Called on Escape, pointercancel, window blur, or component unmount after activation. */
  onCancel?: () => void;
  /** Always called after an activated drag finishes (commit or cancel). */
  onEnd?: (committed: boolean) => void;
}

/**
 * Unified Pointer Events drag session engine for Orbit Desktop.
 *
 * Guarantees across all drag surfaces:
 * - Primary button guard & optional double-click pass-through.
 * - Consistent activation threshold before mutating DOM/cursor/state.
 * - `pointerup`, `pointercancel`, `window.blur`, `Escape`, and unmount cleanup.
 * - `isDragging` flag for mounting `<DragShield />` over iframes & WebGL canvases.
 * - Short click-suppression window after drag release to prevent stray clicks.
 */
export function usePointerDrag() {
  const [isDragging, setIsDragging] = useState(false);
  const activeCleanupRef = useRef<(() => void) | null>(null);
  const suppressClickRef = useRef(false);
  const suppressTimerRef = useRef<number | null>(null);

  const clearSuppressTimer = useCallback(() => {
    if (suppressTimerRef.current !== null) {
      window.clearTimeout(suppressTimerRef.current);
      suppressTimerRef.current = null;
    }
  }, []);

  const cancel = useCallback(() => {
    activeCleanupRef.current?.();
  }, []);

  useEffect(() => {
    return () => {
      activeCleanupRef.current?.();
      clearSuppressTimer();
    };
  }, [clearSuppressTimer]);

  const shouldSuppressClick = useCallback(() => suppressClickRef.current, []);

  const start = useCallback(
    (e: React.PointerEvent<HTMLElement>, options: StartPointerDragOptions) => {
      if (e.button !== 0) return false;
      if (options.ignoreDoubleClick && e.detail >= 2) {
        if (options.preventDefaultOnDown) e.preventDefault();
        return false;
      }
      if (options.ignoreSelector) {
        const target = e.target as HTMLElement | null;
        if (target?.closest(options.ignoreSelector)) return false;
      }
      if (options.preventDefaultOnDown) {
        e.preventDefault();
      }

      // Clean up any stale in-flight session first
      activeCleanupRef.current?.();
      clearSuppressTimer();

      const pointerId = e.pointerId;
      const isTouchPointer = e.pointerType === "touch";
      const startX = e.clientX;
      const startY = e.clientY;
      const thresholdPx = options.thresholdPx ?? DRAG_THRESHOLD_SORT_PX;
      const thresholdAxis = options.thresholdAxis ?? "both";
      const cursor = options.cursor;
      const useRaf = Boolean(options.raf);

      let activated = false;
      let finished = false;
      let rafId: number | null = null;
      let latestFrame: PointerDragFrame | null = null;
      let prevBodyCursor = "";

      const makeFrame = (ev: PointerEvent): PointerDragFrame => ({
        startX,
        startY,
        clientX: ev.clientX,
        clientY: ev.clientY,
        deltaX: ev.clientX - startX,
        deltaY: ev.clientY - startY,
        event: ev,
      });

      const flushPendingMove = () => {
        if (rafId !== null) {
          window.cancelAnimationFrame(rafId);
          rafId = null;
        }
        if (activated && latestFrame) {
          options.onMove?.(latestFrame);
        }
      };

      const finish = (commit: boolean, upEvent?: PointerEvent) => {
        if (finished) return;
        finished = true;

        const hadPendingRaf = rafId !== null;
        if (rafId !== null) {
          window.cancelAnimationFrame(rafId);
          rafId = null;
        }

        window.removeEventListener("pointermove", onMove, true);
        window.removeEventListener("pointerup", onPointerUp, true);
        window.removeEventListener("pointercancel", onPointerCancel, true);
        window.removeEventListener("dragstart", onDragStart, true);
        window.removeEventListener("blur", onWindowBlur);
        window.removeEventListener("keydown", onKeyDown, true);

        if (activeCleanupRef.current === forceCancel) {
          activeCleanupRef.current = null;
        }

        if (activated) {
          if (cursor) {
            document.body.style.cursor = prevBodyCursor;
          }
          setIsDragging(false);

          const finalFrame = upEvent ? makeFrame(upEvent) : latestFrame;
          if (commit) {
            if (useRaf && hadPendingRaf && finalFrame) {
              options.onMove?.(finalFrame);
            }
            options.onCommit?.(finalFrame);
          } else {
            options.onCancel?.();
          }
          options.onEnd?.(commit);

          suppressClickRef.current = true;
          clearSuppressTimer();
          suppressTimerRef.current = window.setTimeout(() => {
            suppressTimerRef.current = null;
            suppressClickRef.current = false;
          }, 80);
        }
      };

      const forceCancel = () => finish(false);

      const onDragStart = (ev: DragEvent) => {
        // Prevent macOS WKWebView (`dragDropEnabled: false`) from hijacking custom pointer drags
        // into native HTML5 drag operations and firing a spurious `pointercancel`.
        ev.preventDefault();
      };

      const onMove = (ev: PointerEvent) => {
        if (isTouchPointer && ev.pointerId !== pointerId) return;
        if (!isTouchPointer && ev.pointerType === "mouse" && ev.buttons === 0) {
          finish(true, ev);
          return;
        }
        const frame = makeFrame(ev);
        latestFrame = frame;

        if (!activated) {
          const dist =
            thresholdAxis === "x"
              ? Math.abs(frame.deltaX)
              : thresholdAxis === "y"
                ? Math.abs(frame.deltaY)
                : Math.hypot(frame.deltaX, frame.deltaY);
          if (dist < thresholdPx) return;

          if (options.onActivate?.(frame) === false) {
            finish(false);
            return;
          }
          activated = true;
          suppressClickRef.current = true;
          if (cursor) {
            prevBodyCursor = document.body.style.cursor;
            document.body.style.cursor = cursor;
          }
          setIsDragging(true);
        }

        if (useRaf) {
          if (rafId === null) {
            rafId = window.requestAnimationFrame(flushPendingMove);
          }
        } else {
          options.onMove?.(frame);
        }
      };

      const onPointerUp = (ev: PointerEvent) => {
        if (isTouchPointer && ev.pointerId !== pointerId) return;
        finish(true, ev);
      };

      const onPointerCancel = (ev: PointerEvent) => {
        if (isTouchPointer && ev.pointerId !== pointerId) return;
        finish(false);
      };

      const onWindowBlur = () => {
        finish(false);
      };

      const onKeyDown = (ev: KeyboardEvent) => {
        if (ev.key === "Escape") {
          if (activated) {
            ev.preventDefault();
            ev.stopPropagation();
          }
          finish(false);
        }
      };

      activeCleanupRef.current = forceCancel;
      window.addEventListener("pointermove", onMove, true);
      window.addEventListener("pointerup", onPointerUp, true);
      window.addEventListener("pointercancel", onPointerCancel, true);
      window.addEventListener("dragstart", onDragStart, true);
      window.addEventListener("blur", onWindowBlur);
      window.addEventListener("keydown", onKeyDown, true);
      return true;
    },
    [clearSuppressTimer],
  );

  return useMemo(
    () => ({
      isDragging,
      start,
      cancel,
      shouldSuppressClick,
    }),
    [isDragging, start, cancel, shouldSuppressClick],
  );
}
