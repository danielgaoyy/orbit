import {
  memo,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import { Card } from "@/components/ui/card";
import { OVERVIEW_PANEL_WIDTH_PX } from "@/lib/workspace-layout";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

/**
 * Computes the top viewport offset for any right-docked 304px stage rail (`StageOverviewPanel`
 * or `PersonalAgentBriefingRail`) from the live stage container rect.
 */
export function useDockedRailTopPx(
  stageContainerRef: RefObject<HTMLDivElement | null> | undefined,
  canDock: boolean,
): number {
  const [dockedTopPx, setDockedTopPx] = useState(52);

  useLayoutEffect(() => {
    const el = stageContainerRef?.current;
    if (!el) return;
    let rafId = 0;
    const syncRect = () => {
      const nextTop = Math.round(el.getBoundingClientRect().top) + 12;
      setDockedTopPx((prev) => (prev === nextTop ? prev : nextTop));
    };
    const scheduleSyncRect = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        syncRect();
      });
    };
    syncRect();
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(scheduleSyncRect) : null;
    ro?.observe(el);
    if (el.parentElement) ro?.observe(el.parentElement);
    window.addEventListener("resize", scheduleSyncRect);
    return () => {
      ro?.disconnect();
      window.removeEventListener("resize", scheduleSyncRect);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [stageContainerRef, canDock]);

  return dockedTopPx;
}

/**
 * Isolated Docked Shell so `useWorkspaceMotion()` phase transitions (e.g. Left Sidebar toggling)
 * animate the outer GPU transform without re-reconciling memoized rail body children.
 */
export const DockedStageRailShell = memo(function DockedStageRailShell({
  ariaLabel,
  dockedTopPx,
  children,
}: {
  ariaLabel: string;
  dockedTopPx: number;
  children: ReactNode;
}) {
  const {
    isAnimating,
    dockedOverviewMounted,
    dockedOverviewVisualOpen,
    dockedOverviewTransform,
    dockedOverviewTransition,
  } = useWorkspaceMotion();

  if (!dockedOverviewMounted) return null;

  const responsiveMaxHeight = `min(640px, max(160px, calc(100dvh - ${dockedTopPx + 16}px)))`;

  return (
    <aside
      aria-label={ariaLabel}
      inert={!dockedOverviewVisualOpen}
      style={{
        top: `${dockedTopPx}px`,
        width: `${OVERVIEW_PANEL_WIDTH_PX}px`,
        maxHeight: responsiveMaxHeight,
        transform: dockedOverviewTransform,
        opacity: dockedOverviewVisualOpen ? 1 : 0,
        transition: dockedOverviewTransition,
        willChange: isAnimating ? "transform, opacity" : undefined,
      }}
      className={cn(
        "fixed right-4 z-[25] flex origin-top-right flex-col",
        !dockedOverviewVisualOpen && "pointer-events-none",
      )}
    >
      <Card className="overview-card-surface flex min-h-0 max-h-full w-full flex-col gap-0 overflow-hidden rounded-[12px] p-0">
        {children}
      </Card>
    </aside>
  );
});

/**
 * Narrow-viewport Popover Shell sharing the exact `canDockOverview` breakpoint, Escape handling,
 * outside-click dismissal, and focus return to `[data-overview-trigger]` across both
 * `StageOverviewPanel` and `PersonalAgentBriefingRail`.
 */
export const PopoverStageRailShell = memo(function PopoverStageRailShell({
  canDock,
  ariaLabel,
  children,
}: {
  canDock: boolean;
  ariaLabel: string;
  children: ReactNode;
}) {
  const popoverOpen = useOrbitSelector((s) => s.overviewPopoverOpen);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const isPopoverVisible = !canDock && popoverOpen;
  const [popoverMounted, setPopoverMounted] = useState(isPopoverVisible);
  const [popoverVisualOpen, setPopoverVisualOpen] = useState(isPopoverVisible);
  const [isPopoverAnimating, setIsPopoverAnimating] = useState(false);
  const mountedRef = useRef(popoverMounted);
  mountedRef.current = popoverMounted;

  useEffect(() => {
    const prefersReducedMotion =
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

    if (isPopoverVisible) {
      setPopoverMounted(true);
      if (prefersReducedMotion) {
        setIsPopoverAnimating(false);
        setPopoverVisualOpen(true);
        const raf = window.requestAnimationFrame(() => {
          popoverRef.current?.focus({ preventScroll: true });
        });
        return () => window.cancelAnimationFrame(raf);
      }
      setIsPopoverAnimating(true);
      let raf2 = 0;
      const raf1 = window.requestAnimationFrame(() => {
        raf2 = window.requestAnimationFrame(() => {
          setPopoverVisualOpen(true);
          popoverRef.current?.focus({ preventScroll: true });
        });
      });
      const settleTimer = window.setTimeout(() => setIsPopoverAnimating(false), 215);
      return () => {
        window.cancelAnimationFrame(raf1);
        if (raf2) window.cancelAnimationFrame(raf2);
        window.clearTimeout(settleTimer);
      };
    }

    setPopoverVisualOpen(false);
    if (!mountedRef.current) {
      setIsPopoverAnimating(false);
      return undefined;
    }
    if (prefersReducedMotion) {
      setIsPopoverAnimating(false);
      setPopoverMounted(false);
      return undefined;
    }
    setIsPopoverAnimating(true);
    const timer = window.setTimeout(() => {
      setIsPopoverAnimating(false);
      setPopoverMounted(false);
    }, 150);
    return () => window.clearTimeout(timer);
  }, [isPopoverVisible]);

  useEffect(() => {
    if (!isPopoverVisible) return;
    const onPointerDown = (ev: MouseEvent) => {
      const target = ev.target as Node | null;
      if (popoverRef.current && target && !popoverRef.current.contains(target)) {
        const el =
          target instanceof Element
            ? target.closest("[data-overview-trigger],[data-slot='dropdown-menu-content']")
            : null;
        if (!el) {
          orbitActions.setOverviewPopoverOpen(false);
        }
      }
    };
    const onKeyDown = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") {
        ev.stopPropagation();
        orbitActions.setOverviewPopoverOpen(false);
        const trigger = document.querySelector<HTMLElement>("[data-overview-trigger]");
        trigger?.focus({ preventScroll: true });
      }
    };
    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [isPopoverVisible]);

  useEffect(() => {
    if (canDock && popoverOpen) {
      orbitActions.setOverviewPopoverOpen(false);
    }
  }, [canDock, popoverOpen]);

  if (!popoverMounted) return null;

  return (
    <div
      ref={popoverRef}
      tabIndex={-1}
      role="dialog"
      aria-label={ariaLabel}
      inert={!popoverVisualOpen}
      onTransitionEnd={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.propertyName !== "transform" && e.propertyName !== "opacity") return;
        setIsPopoverAnimating(false);
        if (!isPopoverVisible) {
          setPopoverMounted(false);
        }
      }}
      style={
        isPopoverAnimating
          ? { width: `${OVERVIEW_PANEL_WIDTH_PX}px`, willChange: "transform, opacity" }
          : { width: `${OVERVIEW_PANEL_WIDTH_PX}px` }
      }
      className={cn(
        "absolute top-3 right-4 z-30 flex max-h-[min(560px,calc(100%-24px))] max-w-[calc(100%-32px)] origin-top-right flex-col outline-none motion-reduce:transition-none",
        popoverVisualOpen
          ? "translate-x-0 translate-y-0 scale-100 opacity-100 transition-[opacity,transform] duration-[165ms] ease-[var(--ease-out-expo)]"
          : "pointer-events-none translate-x-1 -translate-y-1.5 scale-[0.97] opacity-0 transition-[opacity,transform] duration-[120ms] ease-[var(--ease-exit)]",
      )}
    >
      <Card className="floating-surface flex min-h-0 max-h-full w-full flex-col gap-0 overflow-hidden rounded-[12px] border p-0">
        {children}
      </Card>
    </div>
  );
});

/**
 * Shared 304px Stage Rail positioning & breakpoint primitive:
 * - Spacious stage (`canDock = true`): fixed viewport-anchored `.overview-card-surface`
 * - Compact/split stage (`canDock = false`): anchored TopBar `.floating-surface` popover
 */
export const StageRailShell = memo(function StageRailShell({
  canDock,
  ariaLabel,
  stageContainerRef,
  children,
}: {
  canDock: boolean;
  ariaLabel: string;
  stageContainerRef?: RefObject<HTMLDivElement | null>;
  children: ReactNode;
}) {
  const dockedTopPx = useDockedRailTopPx(stageContainerRef, canDock);

  return (
    <>
      <DockedStageRailShell ariaLabel={ariaLabel} dockedTopPx={dockedTopPx}>
        {children}
      </DockedStageRailShell>

      <PopoverStageRailShell canDock={canDock} ariaLabel={ariaLabel}>
        {children}
      </PopoverStageRailShell>
    </>
  );
});
