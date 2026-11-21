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
import { buildWorkspaceShellCssVars, resolveWorkspacePaneWidths } from "@/lib/workspace-layout";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

export type ShellMotionPhase = "idle" | "prep" | "playing";

const PANEL_EASE = "cubic-bezier(0.32, 0.72, 0, 1)";
const OVERVIEW_OPACITY_IN = "opacity 210ms cubic-bezier(0.25, 1, 0.5, 1)";
const OVERVIEW_OPACITY_OUT = "opacity 175ms cubic-bezier(0.4, 0, 0.2, 1)";
const STAGE_MAX_WIDTH_PX = 768;
const STAGE_MIN_LOCK_WIDTH_PX = 240;
const STAGE_H_PADDING_PX = 48;

function availStageContent(V: number, t: ShellGeometryTuple): number {
  return Math.max(0, V - t.L - t.W - t.D - STAGE_H_PADDING_PX);
}

function stageCenterX(V: number, t: ShellGeometryTuple): number {
  return (V + t.L - t.W - t.D) / 2;
}

/**
 * Computes stage motion strategy & geometry during panel open/close transitions:
 * - When content width stays unchanged (`wFrom === wTo`, e.g. wide viewports where prose remains 768px):
 *   stays 100% on the pure GPU `translate3d` path (zero per-frame layout work).
 * - When content width actually changes (`wFrom !== wTo`, e.g. opening/closing a side panel narrows
 *   the conversation column below 768px): switches to fluid CSS `padding` transitions in lockstep
 *   with the sliding sidebar's GPU `translate3d`, so conversation width smoothly expands/compresses
 *   with zero initial snap and zero settle snap.
 */
function computeStageMotionGeometry(
  V: number,
  m: ShellMotionSnapshot,
): {
  isFluidWidthMotion: boolean;
  stageMaxContentWidthPx: number | null;
  prepShiftPx: number;
  playingShiftPx: number;
} {
  const availFrom = availStageContent(V, m.from);
  const availTo = availStageContent(V, m.to);
  const availBase = availStageContent(V, m.base);

  const wFrom = Math.min(STAGE_MAX_WIDTH_PX, Math.max(STAGE_MIN_LOCK_WIDTH_PX, availFrom));
  const wTo = Math.min(STAGE_MAX_WIDTH_PX, Math.max(STAGE_MIN_LOCK_WIDTH_PX, availTo));

  if (Math.abs(wTo - wFrom) > 0.5) {
    return {
      isFluidWidthMotion: true,
      stageMaxContentWidthPx: null,
      prepShiftPx: 0,
      playingShiftPx: 0,
    };
  }

  const stageMaxContentWidthPx =
    availTo < STAGE_MAX_WIDTH_PX && availTo < availBase
      ? Math.max(STAGE_MIN_LOCK_WIDTH_PX, availTo)
      : null;

  const prepShiftPx = stageCenterX(V, m.from) - stageCenterX(V, m.base);
  const playingShiftPx = stageCenterX(V, m.to) - stageCenterX(V, m.base);

  return {
    isFluidWidthMotion: false,
    stageMaxContentWidthPx,
    prepShiftPx,
    playingShiftPx,
  };
}

export interface ShellGeometryTuple {
  L: number;
  W: number;
  D: number;
}

interface ShellMotionSnapshot {
  from: ShellGeometryTuple;
  to: ShellGeometryTuple;
  base: ShellGeometryTuple;
  viewportWidth: number;
  durationMs: number;
}

interface CommittedToggleFlags {
  sidebarOpen: boolean;
  shouldRightOpen: boolean;
  dockedOverview: boolean;
  hasActiveSession: boolean;
}

export interface WorkspaceMotionContextValue {
  phase: ShellMotionPhase;
  isAnimating: boolean;
  isResizing: boolean;
  hasSelectedHost: boolean;
  shouldRightOpen: boolean;
  /** CSS width expression for the left sidebar (`var(--orbit-left-w)` while idle, fixed px while animating) */
  leftWidthCss: string;
  /** CSS width expression for the right side panel (`var(--orbit-right-w)` while idle, fixed px while animating) */
  panelWidthCss: string;
  leftLayoutReserveCss: string;
  rightLayoutReserveCss: string;
  shellReserveTransition: string;
  overviewReservePx: number;
  overviewReserveTransition: string;
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
  topbarPaddingTransition: string;
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
  dockedOverviewTransform: string;
  dockedOverviewTransition: string;
  setIsResizingLeft: (resizing: boolean) => void;
  setIsResizingRight: (resizing: boolean) => void;
  onLeftTransitionEnd: (e: TransitionEvent<HTMLElement>) => void;
  onRightTransitionEnd: (e: TransitionEvent<HTMLElement>) => void;
  onStageTransitionEnd: (e: TransitionEvent<HTMLElement>) => void;
}

const WorkspaceMotionContext = createContext<WorkspaceMotionContextValue | null>(null);

function evalBreakpoints(params: {
  viewportWidth: number;
  sidebarOpen: boolean;
  sidebarWidth: number;
  shouldRightOpen: boolean;
  sidePanelWidth: number;
  hasActiveSession: boolean;
  overviewDesktopOpenPref: boolean;
  prevCanDock: boolean;
  prevIsWide: boolean;
}): { canDockOverview: boolean; isStageWide: boolean } {
  const panes = resolveWorkspacePaneWidths({
    viewportWidth: params.viewportWidth,
    sidebarOpen: params.sidebarOpen,
    sidebarPrefWidth: params.sidebarWidth,
    sidePanelOpen: params.shouldRightOpen,
    sidePanelPrefWidth: params.sidePanelWidth,
  });
  const centerW = panes.centerWidthPx;
  const canDockOverview = params.prevCanDock ? centerW >= 1000 : centerW >= 1040;
  const D = params.hasActiveSession && canDockOverview && params.overviewDesktopOpenPref ? 324 : 0;
  const effectiveContentW = centerW - D;
  const isStageWide = params.prevIsWide ? effectiveContentW >= 776 : effectiveContentW >= 800;
  return { canDockOverview, isStageWide };
}

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

  const isMac = isMacDesktop();
  const hasSelectedHost = Boolean(selectedHostId);
  const hasActiveSession =
    workspacePanel === "conversation" && !isDraftNewSession && Boolean(selectedAgentId);
  const shouldRightOpen = sidePanelOpen && hasSelectedHost;

  // Store live window width strictly in a ref so continuous window resizing triggers
  // ZERO per-frame React re-renders; only discrete breakpoint crossings bump `breakpointTick`.
  const viewportWidthRef = useRef<number>(
    typeof window !== "undefined" ? window.innerWidth : 1280,
  );
  const [, setBreakpointTick] = useState(0);
  const canDockRef = useRef(false);
  const isWideRef = useRef(false);

  // Compute `canDockOverview` & `isStageWide` SYNCHRONOUSLY during render so toggling the
  // right side panel / left sidebar updates `W` and `D` in the exact same render frame
  // (never split across two renders via useLayoutEffect!).
  const liveViewportWidth =
    typeof window !== "undefined" ? window.innerWidth : viewportWidthRef.current;
  viewportWidthRef.current = liveViewportWidth;
  const { canDockOverview, isStageWide } = evalBreakpoints({
    viewportWidth: liveViewportWidth,
    sidebarOpen,
    sidebarWidth,
    shouldRightOpen,
    sidePanelWidth,
    hasActiveSession,
    overviewDesktopOpenPref,
    prevCanDock: canDockRef.current,
    prevIsWide: isWideRef.current,
  });
  canDockRef.current = canDockOverview;
  isWideRef.current = isStageWide;

  const latestPrefsRef = useRef({
    sidebarOpen,
    sidebarWidth,
    shouldRightOpen,
    sidePanelWidth,
    hasActiveSession,
    overviewDesktopOpenPref,
  });
  latestPrefsRef.current = {
    sidebarOpen,
    sidebarWidth,
    shouldRightOpen,
    sidePanelWidth,
    hasActiveSession,
    overviewDesktopOpenPref,
  };

  // Listen to window resize in rAF strictly to detect discrete breakpoint crossings (0 re-renders otherwise)
  useEffect(() => {
    if (typeof window === "undefined") return;
    let rafId = 0;
    const onResize = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        const p = latestPrefsRef.current;
        const vw = window.innerWidth;
        viewportWidthRef.current = vw;
        const next = evalBreakpoints({
          viewportWidth: vw,
          sidebarOpen: p.sidebarOpen,
          sidebarWidth: p.sidebarWidth,
          shouldRightOpen: p.shouldRightOpen,
          sidePanelWidth: p.sidePanelWidth,
          hasActiveSession: p.hasActiveSession,
          overviewDesktopOpenPref: p.overviewDesktopOpenPref,
          prevCanDock: canDockRef.current,
          prevIsWide: isWideRef.current,
        });
        if (next.canDockOverview !== canDockRef.current || next.isStageWide !== isWideRef.current) {
          setBreakpointTick((n) => n + 1);
        }
      });
    };
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, []);

  useLayoutEffect(() => {
    orbitActions.setStageCanDockOverview(canDockOverview);
  }, [canDockOverview]);

  const [isResizingLeft, setIsResizingLeftState] = useState(false);
  const [isResizingRight, setIsResizingRightState] = useState(false);
  const isResizing = isResizingLeft || isResizingRight;
  const isResizingRef = useRef(isResizing);
  isResizingRef.current = isResizing;

  const dockedOverview = hasActiveSession && canDockOverview && overviewDesktopOpenPref;
  const toggleSig = `${sidebarOpen}:${shouldRightOpen}:${dockedOverview}`;
  const toggleSigRef = useRef(toggleSig);
  toggleSigRef.current = toggleSig;

  const [phase, setPhase] = useState<ShellMotionPhase>("idle");
  const phaseRef = useRef<ShellMotionPhase>(phase);
  phaseRef.current = phase;

  const [committedSig, setCommittedSig] = useState(toggleSig);
  const [committedFlags, setCommittedFlags] = useState<CommittedToggleFlags>({
    sidebarOpen,
    shouldRightOpen,
    dockedOverview,
    hasActiveSession,
  });

  const snapshotTupleFor = (
    vw: number,
    flags: { sidebarOpen: boolean; shouldRightOpen: boolean; dockedOverview: boolean },
  ): ShellGeometryTuple => {
    const panes = resolveWorkspacePaneWidths({
      viewportWidth: vw,
      sidebarOpen: flags.sidebarOpen,
      sidebarPrefWidth: sidebarWidth,
      sidePanelOpen: flags.shouldRightOpen,
      sidePanelPrefWidth: sidePanelWidth,
    });
    return {
      L: flags.sidebarOpen ? panes.leftWidthPx : 0,
      W: flags.shouldRightOpen ? panes.rightWidthPx : 0,
      D: flags.dockedOverview ? 324 : 0,
    };
  };

  const [motion, setMotion] = useState<ShellMotionSnapshot>(() => {
    const vw = viewportWidthRef.current;
    const init = snapshotTupleFor(vw, { sidebarOpen, shouldRightOpen, dockedOverview });
    return {
      from: init,
      to: init,
      base: init,
      viewportWidth: vw,
      durationMs: 260,
    };
  });
  const motionRef = useRef(motion);
  motionRef.current = motion;

  // Synchronously adjust motion state during render on panel toggle so React never paints a stale frame
  if (toggleSig !== committedSig) {
    const currentV = typeof window !== "undefined" ? window.innerWidth : viewportWidthRef.current;
    viewportWidthRef.current = currentV;

    const prevFlags = committedFlags;
    const nextFlags: CommittedToggleFlags = {
      sidebarOpen,
      shouldRightOpen,
      dockedOverview,
      hasActiveSession,
    };
    setCommittedSig(toggleSig);
    setCommittedFlags(nextFlags);

    const prefersReducedMotion =
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

    const nextTarget = snapshotTupleFor(currentV, nextFlags);
    const prevFrom =
      phase === "playing" ? motion.to : snapshotTupleFor(currentV, prevFlags);

    // Switching between views (e.g. New Conversation <-> Session, or Automations <-> Session)
    // mounts a different page rather than toggling Overview on the current conversation.
    const onlyViewPresenceChanged =
      hasActiveSession !== prevFlags.hasActiveSession &&
      sidebarOpen === prevFlags.sidebarOpen &&
      shouldRightOpen === prevFlags.shouldRightOpen;

    if (prefersReducedMotion || isResizing || onlyViewPresenceChanged) {
      setPhase("idle");
    } else if (
      phase === "playing" &&
      nextTarget.L === motion.from.L &&
      nextTarget.W === motion.from.W &&
      nextTarget.D === motion.from.D
    ) {
      const openingAny =
        nextTarget.L > motion.to.L || nextTarget.W > motion.to.W || nextTarget.D > motion.to.D;
      setMotion({
        from: motion.to,
        to: nextTarget,
        base: motion.base,
        viewportWidth: currentV,
        durationMs: openingAny ? 260 : 225,
      });
    } else if (
      prevFrom.L === nextTarget.L &&
      prevFrom.W === nextTarget.W &&
      prevFrom.D === nextTarget.D
    ) {
      setPhase("idle");
    } else {
      const base: ShellGeometryTuple = {
        L: Math.min(prevFrom.L, nextTarget.L),
        W: Math.min(prevFrom.W, nextTarget.W),
        D: Math.min(prevFrom.D, nextTarget.D),
      };
      const openingAny =
        nextTarget.L > prevFrom.L || nextTarget.W > prevFrom.W || nextTarget.D > prevFrom.D;
      setMotion({
        from: prevFrom,
        to: nextTarget,
        base,
        viewportWidth: currentV,
        durationMs: openingAny ? 260 : 225,
      });
      setPhase("prep");
    }
  } else if (hasActiveSession !== committedFlags.hasActiveSession) {
    setCommittedFlags({
      sidebarOpen,
      shouldRightOpen,
      dockedOverview,
      hasActiveSession,
    });
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
      if (e.target !== e.currentTarget) return;
      if (phaseRef.current !== "playing") return;
      const m = motionRef.current;
      const { isFluidWidthMotion, prepShiftPx, playingShiftPx } = computeStageMotionGeometry(
        m.viewportWidth,
        m,
      );
      if (axis === "stage" && isFluidWidthMotion) {
        if (e.propertyName !== "padding-right") return;
      } else if (e.propertyName !== "transform") {
        return;
      }
      const stageMoved = !isFluidWidthMotion && Math.abs(playingShiftPx - prepShiftPx) > 0.5;
      const expectedAxis: "left" | "right" | "stage" = stageMoved
        ? "stage"
        : m.from.W !== m.to.W
          ? "right"
          : m.from.L !== m.to.L
            ? "left"
            : "stage";
      if (axis !== expectedAxis) return;

      if (fallbackTimerRef.current !== null) {
        window.clearTimeout(fallbackTimerRef.current);
        fallbackTimerRef.current = null;
      }
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

  // Pure CSS custom properties on the shell wrapper drive 100% of window resize & sash drag
  // geometry natively in WebKit C++ with ZERO child React re-renders!
  const shellCssVars = useMemo(
    () =>
      buildWorkspaceShellCssVars({
        sidebarOpen,
        sidebarPrefWidth: sidebarWidth,
        sidePanelOpen: shouldRightOpen,
        sidePanelPrefWidth: sidePanelWidth,
      }),
    [sidebarOpen, sidebarWidth, shouldRightOpen, sidePanelWidth],
  );

  // Notice `sidebarWidth`, `sidePanelWidth`, and `viewportWidth` are intentionally NOT dependencies
  // of this context value: while `phase === "idle"`, all width/reserve values are stable CSS
  // custom-property expressions (`var(--orbit-left-w)`, `var(--orbit-right-w)`), so dragging
  // sashes or resizing the window never invalidates this context or re-renders Stage / TopBar / SidePanel!
  const value = useMemo<WorkspaceMotionContextValue>(() => {
    const isAnimating = phase !== "idle";
    const m = motion;
    const V = m.viewportWidth;

    const xTitle = (l: number) => l + (l > 0 ? 16 : isMac ? 114 : 48);
    const xTopRight = (w: number) => V - w - (w > 0 ? 12 : hasSelectedHost ? 42 : 12);

    const stageGeom = phase === "idle" ? null : computeStageMotionGeometry(V, m);
    const isFluidWidthMotion = Boolean(stageGeom?.isFluidWidthMotion);

    const leftWidthCss =
      phase === "idle"
        ? "var(--orbit-left-w)"
        : `${Math.max(m.from.L, m.to.L)}px`;
    const panelWidthCss =
      phase === "idle"
        ? "var(--orbit-right-w)"
        : `${Math.max(m.from.W, m.to.W)}px`;

    const leftLayoutReserveCss =
      phase === "idle"
        ? sidebarOpen
          ? "var(--orbit-left-w)"
          : "0px"
        : isFluidWidthMotion
          ? `${phase === "prep" ? m.from.L : m.to.L}px`
          : `${m.base.L}px`;
    const rightLayoutReserveCss =
      phase === "idle"
        ? shouldRightOpen
          ? "var(--orbit-right-w)"
          : "0px"
        : isFluidWidthMotion
          ? `${phase === "prep" ? m.from.W : m.to.W}px`
          : `${m.base.W}px`;
    const overviewReservePx =
      phase === "idle"
        ? dockedOverview
          ? 324
          : 0
        : isFluidWidthMotion
          ? phase === "prep"
            ? m.from.D
            : m.to.D
          : m.base.D;
    const dockedOverviewLayout = overviewReservePx > 0;

    const shellReserveTransition =
      !isResizing && phase === "playing" && isFluidWidthMotion
        ? `padding-left ${m.durationMs}ms ${PANEL_EASE}, padding-right ${m.durationMs}ms ${PANEL_EASE}`
        : "none";
    const overviewReserveTransition =
      !isResizing && phase === "playing" && isFluidWidthMotion
        ? `padding-right ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const leftVisible =
      phase === "idle" ? sidebarOpen : m.from.L > 0 || m.to.L > 0;
    const leftIsIn =
      phase === "idle" ? sidebarOpen : phase === "prep" ? m.from.L > 0 : m.to.L > 0;
    const leftTransform = leftIsIn ? "translate3d(0, 0, 0)" : "translate3d(-100%, 0, 0)";
    const leftTransition =
      !isResizing && phase === "playing" && m.from.L !== m.to.L
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const rightVisible =
      phase === "idle" ? shouldRightOpen : m.from.W > 0 || m.to.W > 0;
    const isRightSettledOpen = shouldRightOpen && phase === "idle";
    const rightIsIn =
      phase === "idle" ? shouldRightOpen : phase === "prep" ? m.from.W > 0 : m.to.W > 0;
    const rightTransform = rightIsIn ? "translate3d(0, 0, 0)" : "translate3d(100%, 0, 0)";
    const rightTransition =
      !isResizing && phase === "playing" && m.from.W !== m.to.W
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const topbarUseOpenLeftPad =
      phase === "idle"
        ? sidebarOpen
        : isFluidWidthMotion
          ? phase === "prep"
            ? m.from.L > 0
            : m.to.L > 0
          : m.base.L > 0;
    const topbarReserveRightToggle =
      hasSelectedHost &&
      (phase === "idle"
        ? !shouldRightOpen
        : isFluidWidthMotion
          ? phase === "prep"
            ? m.from.W === 0
            : m.to.W === 0
          : m.base.W === 0);
    const topbarPaddingTransition =
      !isResizing && phase === "playing" && isFluidWidthMotion
        ? `padding-left ${m.durationMs}ms ${PANEL_EASE}, padding-right ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const baseTitleX = xTitle(m.base.L);
    const topbarTitleOffsetPx =
      phase === "idle" || isFluidWidthMotion
        ? 0
        : phase === "prep"
          ? xTitle(m.from.L) - baseTitleX
          : xTitle(m.to.L) - baseTitleX;
    const topbarTitleTransition =
      !isResizing && phase === "playing" && !isFluidWidthMotion && m.from.L !== m.to.L
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const baseRightX = xTopRight(m.base.W);
    const topbarRightOffsetPx =
      phase === "idle" || isFluidWidthMotion
        ? 0
        : phase === "prep"
          ? xTopRight(m.from.W) - baseRightX
          : xTopRight(m.to.W) - baseRightX;
    const topbarRightTransition =
      !isResizing && phase === "playing" && !isFluidWidthMotion && m.from.W !== m.to.W
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const outlineOffsetPx =
      phase === "idle" || isFluidWidthMotion
        ? 0
        : phase === "prep"
          ? m.from.L - m.base.L
          : m.to.L - m.base.L;
    const outlineTransition =
      !isResizing && phase === "playing" && !isFluidWidthMotion && m.from.L !== m.to.L
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";

    const prepShiftPx = stageGeom?.prepShiftPx ?? 0;
    const playingShiftPx = stageGeom?.playingShiftPx ?? 0;
    const stageMoved = !isFluidWidthMotion && Math.abs(playingShiftPx - prepShiftPx) > 0.5;
    const stageShiftPx =
      phase === "idle" || isFluidWidthMotion
        ? 0
        : phase === "prep"
          ? prepShiftPx
          : playingShiftPx;
    const stageTransition =
      !isResizing && phase === "playing" && stageMoved
        ? `transform ${m.durationMs}ms ${PANEL_EASE}`
        : "none";
    const stageMaxContentWidthPx = stageGeom?.stageMaxContentWidthPx ?? null;

    const dockedOverviewMounted =
      phase === "idle" ? dockedOverview : m.from.D > 0 || m.to.D > 0;
    const dockedOverviewVisualOpen =
      phase === "idle" ? dockedOverview : phase === "prep" ? m.from.D > 0 : m.to.D > 0;

    let dockedOverviewTransform: string;
    if (phase === "idle") {
      dockedOverviewTransform = shouldRightOpen
        ? "translate3d(calc(var(--orbit-right-w) * -1), 0, 0) scale(1)"
        : "translate3d(0, 0, 0) scale(1)";
    } else {
      let offsetPx = -m.to.W;
      if (phase === "prep") {
        offsetPx = m.from.D > 0 ? -m.from.W : -m.to.W + (m.to.W < m.from.W ? 16 : 12);
      } else {
        offsetPx =
          m.to.D > 0
            ? -m.to.W
            : m.to.W > m.from.W
              ? -m.from.W - 40
              : -m.from.W + 12;
      }
      dockedOverviewTransform = `translate3d(${offsetPx}px, 0, 0) scale(${dockedOverviewVisualOpen ? 1 : 0.985})`;
    }

    let dockedOverviewTransition = "none";
    if (!isResizing && phase === "playing") {
      if (m.from.D > 0 && m.to.D > 0) {
        dockedOverviewTransition = `transform ${m.durationMs}ms ${PANEL_EASE}`;
      } else if (m.to.D > 0) {
        dockedOverviewTransition = `transform ${m.durationMs}ms ${PANEL_EASE}, ${OVERVIEW_OPACITY_IN}`;
      } else if (m.from.D > 0) {
        dockedOverviewTransition = `transform ${m.durationMs}ms ${PANEL_EASE}, ${OVERVIEW_OPACITY_OUT}`;
      }
    }

    return {
      phase,
      isAnimating,
      isResizing,
      hasSelectedHost,
      shouldRightOpen,
      leftWidthCss,
      panelWidthCss,
      leftLayoutReserveCss,
      rightLayoutReserveCss,
      shellReserveTransition,
      overviewReservePx,
      overviewReserveTransition,
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
      topbarPaddingTransition,
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
      dockedOverviewTransform,
      dockedOverviewTransition,
      setIsResizingLeft,
      setIsResizingRight,
      onLeftTransitionEnd,
      onRightTransitionEnd,
      onStageTransitionEnd,
    };
  }, [
    phase,
    motion,
    sidebarOpen,
    shouldRightOpen,
    dockedOverview,
    isMac,
    hasSelectedHost,
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
    <WorkspaceMotionContext.Provider value={value}>
      <div style={shellCssVars} className="relative h-screen w-screen overflow-clip">
        {children}
      </div>
    </WorkspaceMotionContext.Provider>
  );
}

export function useWorkspaceMotion(): WorkspaceMotionContextValue {
  const ctx = useContext(WorkspaceMotionContext);
  if (!ctx) {
    throw new Error("useWorkspaceMotion must be used inside WorkspaceMotionProvider");
  }
  return ctx;
}
