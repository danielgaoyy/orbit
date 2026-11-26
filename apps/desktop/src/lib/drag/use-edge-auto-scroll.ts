import { useCallback, useEffect, useMemo, useRef } from "react";
import {
  AUTO_SCROLL_EDGE_PX,
  AUTO_SCROLL_MAX_SPEED_PX,
  computeEdgeScrollVelocity,
} from "./drag-constants";

export interface UseEdgeAutoScrollOptions {
  viewportRef: React.RefObject<HTMLElement | null>;
  axis: "x" | "y";
  edgePx?: number;
  maxSpeedPx?: number;
  /** Invoked after the viewport actually scrolls during a frame, with the latest pointer coords. */
  onScrollTick?: (clientX: number, clientY: number) => void;
}

/**
 * Unified horizontal/vertical edge auto-scroller for pointer drag engines.
 * Continuously fires `onScrollTick` while scrolling so drop indicators stay live even if the pointer stops moving.
 */
export function useEdgeAutoScroll({
  viewportRef,
  axis,
  edgePx = AUTO_SCROLL_EDGE_PX,
  maxSpeedPx = AUTO_SCROLL_MAX_SPEED_PX,
  onScrollTick,
}: UseEdgeAutoScrollOptions) {
  const rafRef = useRef<number | null>(null);
  const velocityRef = useRef<number>(0);
  const pointerRef = useRef<{ x: number; y: number }>({ x: 0, y: 0 });
  const onScrollTickRef = useRef(onScrollTick);
  onScrollTickRef.current = onScrollTick;

  const stop = useCallback(() => {
    velocityRef.current = 0;
    if (rafRef.current !== null) {
      window.cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
  }, []);

  const update = useCallback(
    (clientX: number, clientY: number) => {
      pointerRef.current = { x: clientX, y: clientY };
      const viewport = viewportRef.current;
      if (!viewport) {
        stop();
        return;
      }
      const rect = viewport.getBoundingClientRect();
      const velocity =
        axis === "y"
          ? computeEdgeScrollVelocity(clientY, rect.top, rect.bottom, edgePx, maxSpeedPx)
          : computeEdgeScrollVelocity(clientX, rect.left, rect.right, edgePx, maxSpeedPx);

      velocityRef.current = velocity;
      if (velocity === 0) {
        stop();
        return;
      }
      if (rafRef.current !== null) return;

      const step = () => {
        const v = velocityRef.current;
        const el = viewportRef.current;
        if (v === 0 || !el) {
          rafRef.current = null;
          return;
        }
        const prev = axis === "y" ? el.scrollTop : el.scrollLeft;
        if (axis === "y") {
          el.scrollTop += v;
        } else {
          el.scrollLeft += v;
        }
        const next = axis === "y" ? el.scrollTop : el.scrollLeft;
        if (next !== prev) {
          onScrollTickRef.current?.(pointerRef.current.x, pointerRef.current.y);
        }
        rafRef.current = window.requestAnimationFrame(step);
      };
      rafRef.current = window.requestAnimationFrame(step);
    },
    [viewportRef, axis, edgePx, maxSpeedPx, stop],
  );

  useEffect(() => stop, [stop]);

  return useMemo(() => ({ update, stop }), [update, stop]);
}
