import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type TransitionEvent,
} from "react";

import { isMacDesktop } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

export type ShellMotionPhase = "idle" | "prep" | "playing";

const PANEL_EASE = "cubic-bezier(0.32, 0.72, 0, 1)";
const OVERVIEW_OPACITY_IN = "opacity 210ms cubic-bezier(0.25, 1, 0.5, 1)";
const OVERVIEW_OPACITY_OUT = "opacity 175ms cubic-bezier(0.4, 0, 0.2, 1)";

export interface ShellGeometryTuple {
  L: number;
  W: number;
  D: number;
}

interface ShellMotionSnapshot {
  from: ShellGeometryTuple;
  to: ShellGeometryTuple;
  base: ShellGeometryTuple;
  durationMs: number;
}

export interface WorkspaceMotionContextValue {
  phase: ShellMotionPhase;
  isAnimating: boolean;
  isResizing: boolean;
  hasSelectedHost: boolean;
  shouldRightOpen: boolean;
  panelWidthPx: number;
  leftLayoutReservePx: number;
  rightLayoutReservePx: number;
  dockedOverviewLayout: boolean;
  leftVisible: boolean;
  leftTransform: string;
  leftTransition: string;
  rightVisible: boolean;
  isRightSettledOpen: boolean;
  rightTransform: string;
  rightTransition: string;
  topbarUseOpenLeftPad: boolean;
  topbarReserveRightToggle: boolean;
  topbarTitleOffsetPx: number;
  topbarTitleTransition: string;
  topbarRightOffsetPx: number;
  topbarRightTransition: string;
  stageShiftPx: number;
  stageTransition: string;
  stageMaxContentWidthPx: number | null;
  outlineOffsetPx: number;
  outlineTransition: string;
  canDockOverview: boolean;
  isStageWide: boolean;
  dockedOverviewMounted: boolean;
  dockedOverviewVisualOpen: boolean;
  dockedOverviewOffsetPx: number;
  dockedOverviewTransition: string;
  setIsResizingLeft: (resizing: boolean) => void;
  setIsResizingRight: (resizing: boolean) => void;
  onLeftTransitionEnd: (e: TransitionEvent<HTMLElement>) => void;
  onRightTransitionEnd: (e: TransitionEvent<HTMLElement>) => void;
  onStageTransitionEnd: (e: TransitionEvent<HTMLElement>) => void;
}

const WorkspaceMotionContext = createContext<WorkspaceMotionContextValue | null>(null);

export function WorkspaceMotionProvider({ children }: { children: ReactNode }) {
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const sidebarWidth = useOrbitSelector((s) => s.sidebarWidth);
  const sidePanelOpen = useOrbitSelector((s) => s.sidePanelOpen);
  const sidePanelWidth = useOrbitSelector((s) => s.sidePanelWidth);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const overviewDesktopOpenPref = useOrbitSelector((s) => s.overviewDesktopOpenPref);
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);

  const [viewportWidth, setViewportWidth] = useState<number>(() =>
    typeof window !== "undefined" ? window.innerWidth : 1280,
  );
  const [isResizingLeft, setIsResizingLeftState] = useState(false);
  const [isResizingRight, setIsResizingRightState] = useState(false);

  const isResizing = isResizingLeft || isResizingRight;
  const isResizingRef = useRef(isResizing);
  isResizingRef.current = isResizing;

  useEffect(() => {
    if (typeof window === "undefined") return;
    let rafId = 0;
    const onResize = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        setViewportWidth(window.innerWidth);
      });
    };
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, []);

  const isMac = isMacDesktop();
  const hasSelectedHost = Boolean(selectedHostId);
  const hasActiveSession =
    workspacePanel === "conversation" && !isDraftNewSession && Boolean(selectedAgentId);
  const shouldRightOpen = sidePanelOpen && hasSelectedHost;
  const panelWidthPx = Math.min(sidePanelWidth, Math.floor(viewportWidth * 0.58));

  const canDockRef = useRef(false);
  const isWideRef = useRef(false);

  const L = sidebarOpen ? sidebarWidth : 0;
  const W = shouldRightOpen ? panelWidthPx : 0;
  const centerW = Math.max(0, viewportWidth - L - W);
  const canDockOverview = canDockRef.current ? centerW >= 1000 : centerW >= 1040;
  canDockRef.current = canDockOverview;

  const D = hasActiveSession && canDockOverview && overviewDesktopOpenPref ? 324 : 0;
  const effectiveContentW = centerW - D;
  const isStageWide = isWideRef.current ? effectiveContentW >= 868 : effectiveContentW >= 900;
  isWideRef.current = isStageWide;

  useLayoutEffect(() => {
    orbitActions.setStageCanDockOverview(canDockOverview);
  }, [canDockOverview]);

  const target: ShellGeometryTuple = useMemo(() => ({ L, W, D }), [L, W, D]);
  const targetRef = useRef(target);
  targetRef.current = target;

  const toggleSig = `${sidebarOpen}:${shouldRightOpen}:${D > 0}`;
  const toggleSigRef = useRef(toggleSig);
  toggleSigRef.current = toggleSig;

  const [phase, setPhase] = useState<ShellMotionPhase>("idle");
  const phaseRef = useRef<ShellMotionPhase>(phase);
  phaseRef.current = phase;

  const [committedSig, setCommittedSig] = useState(toggleSig);
  const [committedTarget, setCommittedTarget] = useState<ShellGeometryTuple>(target);

  const [motion, setMotion] = useState<ShellMotionSnapshot>(() => ({
    from: target,
    to: target,
    base: target,
    durationMs: 260,
  }));
  const motionRef = useRef(motion);
  motionRef.current = motion;

  // Synchronously adjust motion state during render so React never paints a stale `idle` frame before `prep`
  if (toggleSig !== committedSig) {
    setCommittedSig(toggleSig);

    const prefersReducedMotion =
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

    if (prefersReducedMotion || isResizing) {
      setCommittedTarget(target);
      setPhase("idle");
    } else if (
      phase === "playing" &&
      target.L === motion.from.L &&
      target.W === motion.from.W &&
      target.D === motion.from.D
    ) {
      const openingAny =
        target.L > motion.to.L || target.W > motion.to.W || target.D > motion.to.D;
      const reversed: ShellMotionSnapshot = {
        from: motion.to,
        to: target,
        base: motion.base,
        durationMs: openingAny ? 260 : 225,
      };
      setMotion(reversed);
      setCommittedTarget(target);
    } else {
      const prevFrom = phase === "playing" ? motion.to : committedTarget;
      if (prevFrom.L === target.L && prevFrom.W === target.W && prevFrom.D === target.D) {
        setCommittedTarget(target);
        setPhase("idle");
      } else {
        const base: ShellGeometryTuple = {
          L: Math.min(prevFrom.L, target.L),
          W: Math.min(prevFrom.W, target.W),
          D: Math.min(prevFrom.D, target.D),
        };
        const openingAny =
          target.L > prevFrom.L || target.W > prevFrom.W || target.D > prevFrom.D;
        const durationMs = openingAny ? 260 : 225;
        setMotion({
          from: prevFrom,
          to: target,
          base,
          durationMs,
        });
        setPhase("prep");
      }
    }
  } else if (
    phase === "idle" &&
    (committedTarget.L !== target.L ||
      committedTarget.W !== target.W ||
      committedTarget.D !== target.D)
  ) {
    setCommittedTarget(target);
  }

  const raf1Ref = useRef<number | null>(null);
  const raf2Ref = useRef<number | null>(null);
  const fallbackTimerRef = useRef<number | null>(null);

  const cancelPendingMotion = useCallback(() => {
    if (raf1Ref.current !== null) {
      window.cancelAnimationFrame(raf1Ref.current);
      raf1Ref.current = null;
    }
    if (raf2Ref.current !== null) {
      window.cancelAnimationFrame(raf2Ref.current);
      raf2Ref.current = null;
    }
    if (fallbackTimerRef.current !== null) {
      window.clearTimeout(fallbackTimerRef.current);
      fallbackTimerRef.current = null;
    }
  }, []);

  const armFallbackTimer = useCallback((durationMs: number) => {
    if (fallbackTimerRef.current !== null) {
      window.clearTimeout(fallbackTimerRef.current);
    }
    fallbackTimerRef.current = window.setTimeout(() => {
      fallbackTimerRef.current = null;
      if (phaseRef.current === "playing") {
        setCommittedTarget(motionRef.current.to);
        setPhase("idle");
      }
    }, durationMs + 65);
  }, []);

  const setIsResizingLeft = useCallback(
    (resizing: boolean) => {
      isResizingRef.current = resizing || isResizingRight;
      setIsResizingLeftState(resizing);
      if (resizing && phaseRef.current !== "idle") {
        cancelPendingMotion();
        setCommittedSig(toggleSigRef.current);
        setCommittedTarget(targetRef.current);
        setPhase("idle");
      }
    },
    [cancelPendingMotion, isResizingRight],
  );

  const setIsResizingRight = useCallback(
    (resizing: boolean) => {
      isResizingRef.current = isResizingLeft || resizing;
      setIsResizingRightState(resizing);
      if (resizing && phaseRef.current !== "idle") {
        cancelPendingMotion();
        setCommittedSig(toggleSigRef.current);
        setCommittedTarget(targetRef.current);
        setPhase("idle");
      }
    },
    [cancelPendingMotion, isResizingLeft],
  );

  // Schedule GPU `prep -> playing` transition and fallback settle strictly via layout effects
  useLayoutEffect(() => {
    cancelPendingMotion();

    if (phase === "prep") {
      raf1Ref.current = window.requestAnimationFrame(() => {
        raf1Ref.current = null;
        raf2Ref.current = window.requestAnimationFrame(() => {
          raf2Ref.current = null;
          setCommittedTarget(motionRef.current.to);
          setPhase("playing");
        });
      });
      return cancelPendingMotion;
    }

    if (phase === "playing") {
      armFallbackTimer(motion.durationMs);
      return cancelPendingMotion;
    }

    return undefined;
  }, [phase, motion, armFallbackTimer, cancelPendingMotion]);

  useEffect(() => {
    return () => {
      cancelPendingMotion();
    };
  }, [cancelPendingMotion]);

  const handleAxisTransitionEnd = useCallback(
    (axis: "left" | "right" | "stage", e: TransitionEvent<HTMLElement>) => {
      if (e.target !== e.currentTarget || e.propertyName !== "transform") return;
      if (phaseRef.current !== "playing") return;
      const m = motionRef.current;
      const stageMoved =
        m.from.L !== m.to.L || m.from.W !== m.to.W || m.from.D !== m.to.D;
      // Always wait for the stage transform when stage moved so layout commit never snaps ahead of GPU slide.
      const expectedAxis: "left" | "right" | "stage" = stageMoved
        ? "stage"
        : m.from.W !== m.to.W
          ? "right"
          : "left";
      if (axis !== expectedAxis) return;

      if (fallbackTimerRef.current !== null) {
        window.clearTimeout(fallbackTimerRef.current);
        fallbackTimerRef.current = null;
      }
      setCommittedTarget(m.to);
      setPhase("idle");
    },
    [],
  );

  const onLeftTransitionEnd = useCallback(
    (e: TransitionEvent<HTMLElement>) => handleAxisTransitionEnd("left", e),
    [handleAxisTransitionEnd],
  );
  const onRightTransitionEnd = useCallback(
    (e: TransitionEvent<HTMLElement>) => handleAxisTransitionEnd("right", e),
    [handleAxisTransitionEnd],
  );
  const onStageTransitionEnd = useCallback(
    (e: TransitionEvent<HTMLElement>) => handleAxisTransitionEnd("stage", e),
    [handleAxisTransitionEnd],
  );

  const value = useMemo<WorkspaceMotionContextValue>(() => {
    const V = viewportWidth;
    const isAnimating = phase !== "idle";
    const m = motion;

    const xTitle = (l: number) => l + (l > 0 ? 16 : isMac ? 114 : 48);
    const xTopRight = (w: number) => V - w - (w > 0 ? 12 : hasSelectedHost ? 42 : 12);
    const xCenter = (l: number, w: number, d: number) => (V + l - w - d) / 2;
    const availContent = (l: number, w: number, d: number) => Math.max(0, V - l - w - d - 48);

    const leftLayoutReservePx = phase === "idle" ? target.L : m.base.L;
    const rightLayoutReservePx = phase === "idle" ? target.W : m.base.W;
    const dockedOverviewLayout = phase === "idle" ? target.D > 0 : m.base.D > 0;

    const leftVisible = phase === "idle" ? target.L > 0 : m.from.L > 0 || m.to.L > 0;
    const leftShown =
      phase === "idle" ? target.L > 0 : phase === "prep" ? m.from.L > 0 : m.to.L > 0;
    const leftTransform = leftShown ? "translate3d(0, 0, 0)" : "translate3d(-100%, 0, 0)";
    const leftTransition =
      !isResizing && phase === "playing" && m.from.L !== m.to.L
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const rightVisible = phase === "idle" ? target.W > 0 : m.from.W > 0 || m.to.W > 0;
    const isRightSettledOpen = phase === "idle" && target.W > 0;
    const rightShown =
      phase === "idle" ? target.W > 0 : phase === "prep" ? m.from.W > 0 : m.to.W > 0;
    const rightTransform = rightShown ? "translate3d(0, 0, 0)" : "translate3d(100%, 0, 0)";
    const rightTransition =
      !isResizing && phase === "playing" && m.from.W !== m.to.W
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const topbarUseOpenLeftPad = leftLayoutReservePx > 0;
    const topbarReserveRightToggle = hasSelectedHost && rightLayoutReservePx === 0;

    const topbarTitleOffsetPx =
      phase === "idle"
        ? 0
        : (phase === "prep" ? xTitle(m.from.L) : xTitle(m.to.L)) - xTitle(m.base.L);
    const topbarTitleTransition =
      !isResizing && phase === "playing" && m.from.L !== m.to.L
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const outlineOffsetPx =
      phase === "idle"
        ? 0
        : (phase === "prep" ? m.from.L : m.to.L) - m.base.L;
    const outlineTransition =
      !isResizing && phase === "playing" && m.from.L !== m.to.L
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const topbarRightOffsetPx =
      phase === "idle"
        ? 0
        : (phase === "prep" ? xTopRight(m.from.W) : xTopRight(m.to.W)) - xTopRight(m.base.W);
    const topbarRightTransition =
      !isResizing && phase === "playing" && m.from.W !== m.to.W
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const stageShiftPx =
      phase === "idle"
        ? 0
        : (phase === "prep"
            ? xCenter(m.from.L, m.from.W, m.from.D)
            : xCenter(m.to.L, m.to.W, m.to.D)) - xCenter(m.base.L, m.base.W, m.base.D);
    const stageTransition =
      !isResizing && phase === "playing" ? `transform ${m.durationMs}ms ${PANEL_EASE}` : "none";

    let stageMaxContentWidthPx: number | null = null;
    if (phase !== "idle") {
      const availTo = availContent(m.to.L, m.to.W, m.to.D);
      const availFrom = availContent(m.from.L, m.from.W, m.from.D);
      if (availTo < 768 && availTo < availFrom) {
        stageMaxContentWidthPx = Math.max(240, availTo);
      }
    }

    const dockedOverviewMounted =
      phase === "idle" ? target.D > 0 : m.from.D > 0 || m.to.D > 0;
    const dockedOverviewVisualOpen =
      phase === "idle" ? target.D > 0 : phase === "prep" ? m.from.D > 0 : m.to.D > 0;

    let dockedOverviewOffsetPx = -target.W;
    if (phase === "prep") {
      dockedOverviewOffsetPx =
        m.from.D > 0 ? -m.from.W : -m.to.W + (m.to.W < m.from.W ? 16 : 12);
    } else if (phase === "playing") {
      dockedOverviewOffsetPx =
        m.to.D > 0
          ? -m.to.W
          : m.to.W > m.from.W
            ? -m.from.W - 40
            : -m.from.W + 12;
    }

    let dockedOverviewTransition = "none";
    if (!isResizing && phase === "playing") {
      if (m.from.D > 0 && m.to.D > 0) {
        dockedOverviewTransition = `transform ${m.durationMs}ms ${PANEL_EASE}`;
      } else if (m.to.D > 0) {
        dockedOverviewTransition = `transform ${m.durationMs}ms ${PANEL_EASE}, ${OVERVIEW_OPACITY_IN}`;
      } else {
        dockedOverviewTransition = `transform ${m.durationMs}ms ${PANEL_EASE}, ${OVERVIEW_OPACITY_OUT}`;
      }
    }

    return {
      phase,
      isAnimating,
      isResizing,
      hasSelectedHost,
      shouldRightOpen,
      panelWidthPx,
      leftLayoutReservePx,
      rightLayoutReservePx,
      dockedOverviewLayout,
      leftVisible,
      leftTransform,
      leftTransition,
      rightVisible,
      isRightSettledOpen,
      rightTransform,
      rightTransition,
      topbarUseOpenLeftPad,
      topbarReserveRightToggle,
      topbarTitleOffsetPx,
      topbarTitleTransition,
      topbarRightOffsetPx,
      topbarRightTransition,
      stageShiftPx,
      stageTransition,
      stageMaxContentWidthPx,
      outlineOffsetPx,
      outlineTransition,
      canDockOverview,
      isStageWide,
      dockedOverviewMounted,
      dockedOverviewVisualOpen,
      dockedOverviewOffsetPx,
      dockedOverviewTransition,
      setIsResizingLeft,
      setIsResizingRight,
      onLeftTransitionEnd,
      onRightTransitionEnd,
      onStageTransitionEnd,
    };
  }, [
    viewportWidth,
    phase,
    motion,
    target,
    isMac,
    hasSelectedHost,
    shouldRightOpen,
    panelWidthPx,
    isResizing,
    canDockOverview,
    isStageWide,
    setIsResizingLeft,
    setIsResizingRight,
    onLeftTransitionEnd,
    onRightTransitionEnd,
    onStageTransitionEnd,
  ]);

  return (
    <WorkspaceMotionContext.Provider value={value}>{children}</WorkspaceMotionContext.Provider>
  );
}

export function useWorkspaceMotion(): WorkspaceMotionContextValue {
  const ctx = useContext(WorkspaceMotionContext);
  if (!ctx) {
    throw new Error("useWorkspaceMotion must be used inside WorkspaceMotionProvider");
  }
  return ctx;
}
