import { useCallback, useEffect, useRef, useState } from "react";
import {
  DRAG_THRESHOLD_PAN_PX,
  clampPanOffset,
  prefersReducedMotion,
} from "./drag-constants";

export interface UsePanZoomOptions {
  open: boolean;
  minZoom: number;
  maxZoom: number;
  /** Whether pointer drag panning is allowed at `zoom <= 1.01`. */
  allowPanAtBaseZoom?: boolean;
  /** Zoom step factor for keyboard `+/-` and helper buttons. */
  zoomStep?: number;
  /** Wheel zoom mode: `"always"` (diagram) or `"zoomed-or-mod"` (image lightbox). */
  wheelMode?: "always" | "zoomed-or-mod";
  /** Optional element measuring the rendered unscaled content box for pan clamping. */
  contentRef?: React.RefObject<HTMLElement | null>;
  /** Viewport element receiving wheel events and bounding pan clamps. */
  viewportEl: HTMLElement | null;
}

/**
 * Shared zoom/pan/pinch engine for `ImageLightbox` and `DiagramLightbox`.
 *
 * Guarantees:
 * - Consistent 4px drag threshold before suppressing click-to-zoom.
 * - Explicit `setPointerCapture` + `releasePointerCapture`.
 * - Multi-touch pinch zoom support.
 * - Viewport-aware pan clamping so zoomed content can never drift completely off-screen.
 * - Canonical `--ease-out-expo` transition for resets/buttons, `none` during active drag or reduced-motion.
 */
export function usePanZoom({
  open,
  minZoom,
  maxZoom,
  allowPanAtBaseZoom = false,
  zoomStep = 1.2,
  wheelMode = "always",
  contentRef,
  viewportEl,
}: UsePanZoomOptions) {
  const [zoom, setZoomState] = useState(1);
  const [pan, setPanState] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  const [isDragging, setIsDragging] = useState(false);

  const zoomRef = useRef(zoom);
  zoomRef.current = zoom;
  const panRef = useRef(pan);
  panRef.current = pan;

  const activePointersRef = useRef<Map<number, { x: number; y: number }>>(new Map());
  const pinchStartRef = useRef<{ distance: number; zoom: number } | null>(null);
  const dragRef = useRef<{
    active: boolean;
    pointerId: number;
    startX: number;
    startY: number;
    origX: number;
    origY: number;
    moved: boolean;
  }>({
    active: false,
    pointerId: -1,
    startX: 0,
    startY: 0,
    origX: 0,
    origY: 0,
    moved: false,
  });

  const clampZoom = useCallback(
    (value: number) => Math.min(maxZoom, Math.max(minZoom, value)),
    [minZoom, maxZoom],
  );

  const clampPan = useCallback(
    (nextX: number, nextY: number, nextZoom: number): { x: number; y: number } => {
      if (!allowPanAtBaseZoom && nextZoom <= 1.01) {
        return { x: 0, y: 0 };
      }
      if (!viewportEl) return { x: nextX, y: nextY };
      const vpW = viewportEl.clientWidth;
      const vpH = viewportEl.clientHeight;
      const contentEl = contentRef?.current;
      const contentW = contentEl?.offsetWidth ?? vpW * 0.7;
      const contentH = contentEl?.offsetHeight ?? vpH * 0.7;
      return clampPanOffset({
        panX: nextX,
        panY: nextY,
        contentW,
        contentH,
        viewportW: vpW,
        viewportH: vpH,
        zoom: nextZoom,
        allowPanWhenFits: allowPanAtBaseZoom,
      });
    },
    [allowPanAtBaseZoom, contentRef, viewportEl],
  );

  const setZoom = useCallback(
    (updater: number | ((prev: number) => number)) => {
      const raw = typeof updater === "function" ? updater(zoomRef.current) : updater;
      const clamped = clampZoom(raw);
      const next = !allowPanAtBaseZoom && clamped <= 1.01 ? 1 : clamped;
      zoomRef.current = next;
      setZoomState(next);
      if (!allowPanAtBaseZoom && next <= 1.01) {
        const zero = { x: 0, y: 0 };
        panRef.current = zero;
        setPanState(zero);
      } else {
        setPanState((cur) => {
          const nextPan = clampPan(cur.x, cur.y, next);
          panRef.current = nextPan;
          return nextPan;
        });
      }
    },
    [allowPanAtBaseZoom, clampPan, clampZoom],
  );

  const resetTransform = useCallback(() => {
    setZoomState(1);
    setPanState({ x: 0, y: 0 });
    setIsDragging(false);
    dragRef.current.active = false;
    activePointersRef.current.clear();
    pinchStartRef.current = null;
  }, []);

  const zoomIn = useCallback(() => setZoom((z) => z * zoomStep), [setZoom, zoomStep]);
  const zoomOut = useCallback(() => setZoom((z) => z / zoomStep), [setZoom, zoomStep]);

  useEffect(() => {
    if (!open || !viewportEl) return;
    const handleWheel = (ev: WheelEvent) => {
      if (wheelMode === "zoomed-or-mod" && !ev.ctrlKey && !ev.metaKey && zoomRef.current <= 1.01) {
        return;
      }
      ev.preventDefault();
      if (wheelMode === "always") {
        const sensitivity = ev.ctrlKey || ev.metaKey ? 0.0085 : 0.0022;
        const factor = Math.exp(-ev.deltaY * sensitivity);
        setZoom((z) => z * factor);
      } else {
        const factor = ev.deltaY < 0 ? 1.14 : 1 / 1.14;
        setZoom((z) => z * factor);
      }
    };
    viewportEl.addEventListener("wheel", handleWheel, { passive: false });
    return () => viewportEl.removeEventListener("wheel", handleWheel);
  }, [open, viewportEl, wheelMode, setZoom]);

  const canPan = allowPanAtBaseZoom || zoom > 1.01;

  const handlePointerDown = useCallback(
    (ev: React.PointerEvent<HTMLElement>) => {
      if (ev.button !== 0 && ev.pointerType === "mouse") return;
      if (!allowPanAtBaseZoom && zoomRef.current <= 1.01) return;
      ev.preventDefault();
      ev.stopPropagation();

      try {
        ev.currentTarget.setPointerCapture(ev.pointerId);
      } catch {
        // Ignore in headless environments
      }

      activePointersRef.current.set(ev.pointerId, { x: ev.clientX, y: ev.clientY });

      if (activePointersRef.current.size === 2) {
        const pts = Array.from(activePointersRef.current.values());
        const dist = Math.hypot(pts[1].x - pts[0].x, pts[1].y - pts[0].y);
        pinchStartRef.current = { distance: Math.max(dist, 1), zoom: zoomRef.current };
        dragRef.current.active = false;
        dragRef.current.moved = true;
        setIsDragging(false);
        return;
      }

      dragRef.current = {
        active: true,
        pointerId: ev.pointerId,
        startX: ev.clientX,
        startY: ev.clientY,
        origX: panRef.current.x,
        origY: panRef.current.y,
        moved: false,
      };
      setIsDragging(true);
    },
    [allowPanAtBaseZoom],
  );

  const handlePointerMove = useCallback(
    (ev: React.PointerEvent<HTMLElement>) => {
      if (activePointersRef.current.has(ev.pointerId)) {
        activePointersRef.current.set(ev.pointerId, { x: ev.clientX, y: ev.clientY });
      }

      if (activePointersRef.current.size === 2 && pinchStartRef.current) {
        const pts = Array.from(activePointersRef.current.values());
        const dist = Math.hypot(pts[1].x - pts[0].x, pts[1].y - pts[0].y);
        const ratio = dist / pinchStartRef.current.distance;
        setZoom(pinchStartRef.current.zoom * ratio);
        return;
      }

      const d = dragRef.current;
      if (!d.active || d.pointerId !== ev.pointerId) return;
      const dx = ev.clientX - d.startX;
      const dy = ev.clientY - d.startY;
      if (!d.moved && Math.hypot(dx, dy) >= DRAG_THRESHOLD_PAN_PX) {
        d.moved = true;
      }
      setPanState(clampPan(d.origX + dx, d.origY + dy, zoomRef.current));
    },
    [clampPan, setZoom],
  );

  const handlePointerUp = useCallback((ev: React.PointerEvent<HTMLElement>) => {
    activePointersRef.current.delete(ev.pointerId);
    if (activePointersRef.current.size < 2) {
      pinchStartRef.current = null;
    }
    try {
      if (ev.currentTarget.hasPointerCapture(ev.pointerId)) {
        ev.currentTarget.releasePointerCapture(ev.pointerId);
      }
    } catch {
      // Ignore in headless environments
    }
    if (dragRef.current.pointerId === ev.pointerId) {
      dragRef.current.active = false;
      setIsDragging(false);
    }
  }, []);

  const consumeMovedClick = useCallback(() => {
    if (dragRef.current.moved) {
      dragRef.current.moved = false;
      return true;
    }
    return false;
  }, []);

  const transformStyle: React.CSSProperties = {
    transform: `translate3d(${pan.x}px, ${pan.y}px, 0) scale(${zoom})`,
    transition:
      isDragging || prefersReducedMotion()
        ? "none"
        : "transform 180ms var(--ease-out-expo)",
  };

  return {
    zoom,
    pan,
    canPan,
    isDragging,
    setZoom,
    zoomIn,
    zoomOut,
    resetTransform,
    consumeMovedClick,
    transformStyle,
    pointerHandlers: {
      onPointerDown: handlePointerDown,
      onPointerMove: handlePointerMove,
      onPointerUp: handlePointerUp,
      onPointerCancel: handlePointerUp,
    },
  };
}
