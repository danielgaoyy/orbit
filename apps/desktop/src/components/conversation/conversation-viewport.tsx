import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
  type ReactNode,
  type RefObject,
} from "react";
import { ArrowDown, Check, Copy } from "lucide-react";
import { MarkdownLinkProvider } from "@/components/markdown-blocks";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  computeDisclosureRevealDelta,
  computeStageScrollState,
  mayUserPromptOverflow,
  observeSharedBubbleResize,
  shouldFollowBottomOnDisclosureExpand,
  STAGE_STICKY_REENGAGE_THRESHOLD_PX,
  USER_PROMPT_PREVIEW_MAX_PX,
  type ViewportAnchorHoldFn,
} from "@/components/stage/stage-helpers";
import { useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface ViewportTracking {
  holdActiveRef: RefObject<boolean>;
  syncScrollMetrics: () => void;
  holdViewport: ViewportAnchorHoldFn;
  cancelHold?: () => void;
}

// A side panel owns the disclosure callback while its child owns the scroll element.
// Register the current viewport so both observe the same hold, including on session changes.
const trackedViewports = new WeakMap<HTMLElement, ViewportTracking>();

// Rows rendered through a legacy parent still have access to their viewport owner.
export const holdNearestViewport: ViewportAnchorHoldFn = (trigger, apply, edge, options) => {
  const scroller = trigger?.closest<HTMLElement>('[data-slot="scroll-area-viewport"]');
  const owner = scroller ? trackedViewports.get(scroller) : undefined;
  if (owner) owner.holdViewport(trigger, apply, edge, options);
  else apply();
};

export function useViewportTracking(
  viewportRef: RefObject<HTMLDivElement | null>,
  stickToBottomRef: RefObject<boolean>,
  holdActiveRef: RefObject<boolean>,
  syncScrollMetrics: () => void,
  resetKey?: string | null,
) {
  const holdViewport = useViewportAnchorHold(viewportRef, stickToBottomRef);
  useLayoutEffect(() => {
    const el = viewportRef.current;
    if (!el) return;
    const tracking: ViewportTracking = { holdViewport, holdActiveRef, syncScrollMetrics };
    trackedViewports.set(el, tracking);
    return () => {
      tracking.cancelHold?.();
      trackedViewports.delete(el);
    };
  }, [viewportRef, holdViewport, holdActiveRef, syncScrollMetrics, resetKey]);

  const cancelHold = useCallback(() => {
    const el = viewportRef.current;
    if (el) trackedViewports.get(el)?.cancelHold?.();
  }, [viewportRef]);
  return { holdViewport, cancelHold };
}

export function useViewportAnchorHold(
  viewportRef: RefObject<HTMLDivElement | null>,
  stickToBottomRef?: MutableRefObject<boolean>,
): ViewportAnchorHoldFn {
  const pending = useRef<(() => void) | undefined>(undefined);
  useLayoutEffect(() => () => pending.current?.(), []);
  return useCallback<ViewportAnchorHoldFn>((triggerEl, apply, edge = "top", options) => {
    pending.current?.();
    const scroller = viewportRef.current;
    if (!scroller || !triggerEl) {
      apply();
      return;
    }
    const tracking = trackedViewports.get(scroller);
    tracking?.cancelHold?.();
    const container = triggerEl.closest<HTMLElement>(
      '[data-slot="trace-collapsible"], [data-slot="collapsible"], [data-orbit-disclosure-root]',
    ) ?? triggerEl.parentElement ?? triggerEl;
    const beforeContainerHeight = container.getBoundingClientRect().height;
    let anchorTarget = triggerEl.getBoundingClientRect()[edge];
    const wasPinnedBottom = stickToBottomRef?.current;
    const beforeGap = Math.max(0, scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight);
    const followBottomExpand = shouldFollowBottomOnDisclosureExpand({
      followBottom: options?.followBottom,
      intent: options?.intent,
      wasPinnedBottom,
      beforeGap,
    });
    let expanding = options?.intent === "expand";
    let frame = 0;
    let timer = 0;
    let revealTarget: number | undefined;
    let trackedPanel: HTMLElement | null = null;
    let targetPanelHeight = 0;

    const cleanup = () => {
      window.cancelAnimationFrame(frame);
      window.clearTimeout(timer);
      scroller.removeEventListener("scrollend", onScrollEnd);
      if (trackedPanel) {
        trackedPanel.removeEventListener("transitionend", onPanelTransitionEnd);
        trackedPanel.removeEventListener("transitioncancel", onPanelTransitionEnd);
        trackedPanel = null;
      }
      pending.current = undefined;
      if (tracking) {
        tracking.cancelHold = undefined;
        tracking.holdActiveRef.current = false;
      }
    };
    const settle = () => {
      if (scroller === viewportRef.current && scroller.isConnected) {
        if (followBottomExpand) {
          scroller.scrollTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
        }
        const gap = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
        if (stickToBottomRef) {
          stickToBottomRef.current =
            followBottomExpand ||
            (wasPinnedBottom === true && !expanding) ||
            gap <= STAGE_STICKY_REENGAGE_THRESHOLD_PX;
        }
        // Consume the hold's own movement before accepting user scroll deltas again.
        tracking?.syncScrollMetrics();
      }
      cleanup();
    };
    const syncAnimatedFrame = () => {
      if (scroller !== viewportRef.current || !triggerEl.isConnected) {
        cleanup();
        return;
      }
      if (followBottomExpand) {
        const bottomTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
        if (Math.abs(scroller.scrollTop - bottomTop) > 0.5) {
          scroller.scrollTop = bottomTop;
        }
        return;
      }
      // Batch all geometry reads before mutating `scrollTop` once to avoid Read->Write->Read
      // forced synchronous layout thrashing on every frame of the CSS height transition.
      const triggerRect = triggerEl.getBoundingClientRect();
      const rawDelta = triggerRect[edge] - anchorTarget;
      const anchorDelta = Math.abs(rawDelta) > 0.5 ? rawDelta : 0;
      let nudge = 0;
      if (expanding) {
        const rawRect = container.getBoundingClientRect();
        const scrollerRect = scroller.getBoundingClientRect();
        const projectedHeight = Math.max(rawRect.height, beforeContainerHeight + targetPanelHeight);
        nudge = computeDisclosureRevealDelta({
          intent: options?.intent ?? "expand",
          beforeContainerHeight,
          containerRect: { bottom: rawRect.bottom - anchorDelta, height: projectedHeight },
          scrollerRect,
          triggerRect: {
            top: triggerRect.top - anchorDelta,
            bottom: triggerRect.bottom - anchorDelta,
          },
          triggerIsContainer: triggerEl === container,
        });
        if (nudge <= 0.5) nudge = 0;
      }
      const totalDelta = anchorDelta + nudge;
      if (Math.abs(totalDelta) > 0.5) {
        scroller.scrollTop += totalDelta;
      }
      if (nudge > 0) {
        anchorTarget -= nudge;
      }
    };
    const finishAnimatedTransition = () => {
      if (trackedPanel) {
        trackedPanel.removeEventListener("transitionend", onPanelTransitionEnd);
        trackedPanel.removeEventListener("transitioncancel", onPanelTransitionEnd);
        trackedPanel = null;
      }
      window.cancelAnimationFrame(frame);
      window.clearTimeout(timer);
      syncAnimatedFrame();
      if (followBottomExpand) {
        // Wait one post-ResizeObserver frame so `height: auto` + virtualizer measurement
        // finish updating `scrollHeight` before the final bottom snap and hold release.
        frame = window.requestAnimationFrame(() => {
          settle();
        });
        return;
      }
      settle();
    };
    const onPanelTransitionEnd = (event: TransitionEvent) => {
      if (event.target !== trackedPanel) return;
      if (event.propertyName !== "height") return;
      finishAnimatedTransition();
    };
    const onScrollEnd = (event: Event) => {
      if (event.target === scroller && revealTarget !== undefined &&
          Math.abs(scroller.scrollTop - revealTarget) <= 1) settle();
    };
    const cancel = () => {
      cleanup();
      if (revealTarget !== undefined) {
        scroller.scrollTo({ top: scroller.scrollTop, behavior: "instant" });
      }
    };
    pending.current = cancel;
    if (tracking) {
      tracking.cancelHold = cancel;
      tracking.holdActiveRef.current = true;
    }
    if (stickToBottomRef) stickToBottomRef.current = false;
    apply();

    frame = window.requestAnimationFrame(() => {
      if (scroller !== viewportRef.current || !triggerEl.isConnected) {
        cleanup();
        return;
      }
      const directPanel = container.querySelector<HTMLElement>(
        ':scope > [data-slot="trace-collapsible-content"], :scope > [data-slot="collapsible-content"]',
      );
      const panelDurationMs = directPanel
        ? Math.max(
            0,
            ...window
              .getComputedStyle(directPanel)
              .transitionDuration.split(",")
              .map((part) => {
                const trimmed = part.trim();
                const value = Number.parseFloat(trimmed);
                if (!Number.isFinite(value) || value <= 0) return 0;
                return trimmed.endsWith("ms") ? value : value * 1000;
              }),
          )
        : 0;

      if (directPanel && panelDurationMs > 0) {
        // Animated Collapsible panel: keep trigger anchored and reveal overflow smoothly
        // frame-by-frame in lockstep with the CSS height transition instead of jumping on frame 1.
        trackedPanel = directPanel;
        const cssTargetHeight = Number.parseFloat(
          window.getComputedStyle(directPanel).getPropertyValue("--collapsible-panel-height"),
        );
        targetPanelHeight = Math.max(
          Number.isFinite(cssTargetHeight) ? cssTargetHeight : 0,
          directPanel.scrollHeight,
        );
        directPanel.addEventListener("transitionend", onPanelTransitionEnd);
        directPanel.addEventListener("transitioncancel", onPanelTransitionEnd);
        const tick = () => {
          syncAnimatedFrame();
          if (trackedPanel) {
            frame = window.requestAnimationFrame(tick);
          }
        };
        syncAnimatedFrame();
        frame = window.requestAnimationFrame(tick);
        timer = window.setTimeout(() => {
          finishAnimatedTransition();
        }, Math.round(panelDurationMs) + 80);
        return;
      }

      if (followBottomExpand) {
        scroller.scrollTop = Math.max(0, scroller.scrollHeight - scroller.clientHeight);
        frame = window.requestAnimationFrame(() => {
          if (scroller !== viewportRef.current || !triggerEl.isConnected) {
            cleanup();
            return;
          }
          settle();
        });
        return;
      }

      const delta = triggerEl.getBoundingClientRect()[edge] - anchorTarget;
      if (Math.abs(delta) > 1) scroller.scrollTop += delta;

      // Wait one post-ResizeObserver frame before computing reveal nudge or releasing the hold so
      // virtualized lists (`VirtualizedTurnList` / `measureTurn`) finish updating `scrollHeight`
      // and downstream row offsets; otherwise `scrollBy` at the bottom clamps against stale height.
      frame = window.requestAnimationFrame(() => {
        if (scroller !== viewportRef.current || !triggerEl.isConnected) {
          cleanup();
          return;
        }
        const postMeasureDelta = triggerEl.getBoundingClientRect()[edge] - anchorTarget;
        if (Math.abs(postMeasureDelta) > 1) scroller.scrollTop += postMeasureDelta;
        const containerRect = container.getBoundingClientRect();
        expanding = options?.intent
          ? options.intent === "expand"
          : containerRect.height > beforeContainerHeight + 2;
        const nudge = computeDisclosureRevealDelta({
          intent: options?.intent,
          beforeContainerHeight,
          containerRect,
          scrollerRect: scroller.getBoundingClientRect(),
          triggerRect: triggerEl.getBoundingClientRect(),
          triggerIsContainer: triggerEl === container,
        });
        if (nudge <= 1) {
          settle();
          return;
        }
        revealTarget = Math.min(
          scroller.scrollTop + nudge,
          scroller.scrollHeight - scroller.clientHeight,
        );
        scroller.addEventListener("scrollend", onScrollEnd);
        timer = window.setTimeout(settle, 380);
        scroller.scrollBy({ top: nudge, behavior: "smooth" });
      });
    });
  }, [viewportRef, stickToBottomRef]);
}

/**
 * Shared sticky-bottom conversation viewport used across side panels (`/btw` Side Question,
 * Subagent inspection, and secondary threads). Unlocks sticky scrolling immediately on user
 * upward scroll and reveals a restrained jump-to-latest pill.
 */
export const ConversationViewport = memo(function ConversationViewport({
  hostId,
  cwd,
  contentVersion,
  resetKey,
  viewportRef: externalViewportRef,
  stickToBottomRef: externalStickToBottomRef,
  className,
  viewportClassName,
  contentClassName,
  children,
}: {
  hostId?: string | null;
  cwd?: string | null;
  contentVersion: string | number;
  resetKey?: string;
  viewportRef?: MutableRefObject<HTMLDivElement | null>;
  stickToBottomRef?: MutableRefObject<boolean>;
  className?: string;
  viewportClassName?: string;
  contentClassName?: string;
  children: ReactNode;
}) {
  const internalViewportRef = useRef<HTMLDivElement | null>(null);
  const viewportRef = externalViewportRef ?? internalViewportRef;
  const internalStickToBottomRef = useRef(true);
  const stickToBottomRef = externalStickToBottomRef ?? internalStickToBottomRef;
  const [showJumpLatest, setShowJumpLatest] = useState(false);
  const [jumpPillMounted, setJumpPillMounted] = useState(false);
  const [jumpPillVisible, setJumpPillVisible] = useState(false);

  useEffect(() => {
    if (showJumpLatest) {
      setJumpPillMounted(true);
      const raf = window.requestAnimationFrame(() => {
        setJumpPillVisible(true);
      });
      return () => window.cancelAnimationFrame(raf);
    }
    if (!jumpPillMounted) return;
    setJumpPillVisible(false);
    const reduced =
      typeof window !== "undefined" &&
      Boolean(window.matchMedia?.("(prefers-reduced-motion: reduce)").matches);
    if (reduced) {
      setJumpPillMounted(false);
      return;
    }
    const timer = window.setTimeout(() => {
      setJumpPillMounted(false);
    }, 125);
    return () => window.clearTimeout(timer);
  }, [showJumpLatest, jumpPillMounted]);

  const holdActiveRef = useRef(false);
  const lastScrollTopRef = useRef(0);
  const lastClientHeightRef = useRef(0);
  const handleScroll = useCallback(() => {
    const el = viewportRef.current;
    if (!el) return;
    const currentTop = el.scrollTop;
    const currentClientHeight = el.clientHeight;
    const prevClientHeight = lastClientHeightRef.current || currentClientHeight;
    lastClientHeightRef.current = currentClientHeight;
    const distanceFromBottom = Math.max(0, el.scrollHeight - currentTop - currentClientHeight);
    const clampedAtBottom = currentClientHeight > prevClientHeight && distanceFromBottom <= 2;
    const state = computeStageScrollState({
      previousTop: clampedAtBottom ? currentTop : lastScrollTopRef.current,
      currentTop,
      distanceFromBottom,
      stickToBottom: stickToBottomRef.current,
      holdActive: holdActiveRef.current,
    });
    lastScrollTopRef.current = currentTop;
    stickToBottomRef.current = state.stickToBottom;
    setShowJumpLatest(state.showJumpLatest);
  }, [stickToBottomRef, viewportRef]);
  const { cancelHold } = useViewportTracking(viewportRef, stickToBottomRef, holdActiveRef, handleScroll, resetKey);

  useLayoutEffect(() => {
    const el = viewportRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    let prevCh = el.clientHeight;
    lastClientHeightRef.current = prevCh;
    let rafId: number | null = null;
    const ro = new ResizeObserver(() => {
      if (rafId !== null) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = null;
        const ch = el.clientHeight;
        const shrankBy = prevCh - ch;
        prevCh = ch;
        if (shrankBy > 0.5 && stickToBottomRef.current && !holdActiveRef.current) {
          el.scrollTop += shrankBy;
          lastScrollTopRef.current = el.scrollTop;
        }
        handleScroll();
      });
    });
    ro.observe(el);
    return () => {
      if (rafId !== null) window.cancelAnimationFrame(rafId);
      ro.disconnect();
    };
  }, [viewportRef, stickToBottomRef, handleScroll, resetKey]);

  const scrollToLatest = useCallback((smooth = false) => {
    const el = viewportRef.current;
    if (!el) return;
    cancelHold();
    stickToBottomRef.current = true;
    setShowJumpLatest(false);
    el.scrollTo({ top: el.scrollHeight, behavior: smooth ? "smooth" : "instant" });
    lastScrollTopRef.current = el.scrollTop;
  }, [cancelHold, stickToBottomRef, viewportRef]);

  useLayoutEffect(() => {
    scrollToLatest();
  }, [resetKey, scrollToLatest]);

  useEffect(() => {
    const raf = requestAnimationFrame(() => {
      if (stickToBottomRef.current && !holdActiveRef.current) scrollToLatest();
      else handleScroll();
    });
    return () => cancelAnimationFrame(raf);
  }, [contentVersion, handleScroll, scrollToLatest]);

  const interruptFollow = () => {
    cancelHold();
    stickToBottomRef.current = false;
  };

  return (
    <div className={cn("bg-canvas relative flex min-h-0 flex-1 flex-col overflow-hidden", className)}>
      <MarkdownLinkProvider hostId={hostId ?? undefined} cwd={cwd ?? undefined}>
        <ScrollArea
          className="min-h-0 flex-1"
          viewportRef={viewportRef}
          viewportClassName={cn("px-4 py-4 [overflow-anchor:none]", viewportClassName)}
          contentClassName={contentClassName}
          onViewportScroll={handleScroll}
          onWheel={(event) => {
            if (event.deltaY < 0) interruptFollow();
            else if (event.deltaY > 0) cancelHold();
          }}
          onTouchStart={interruptFollow}
          onTouchMove={interruptFollow}
          onPointerDown={(event) => {
            const el = viewportRef.current;
            if (el && event.target === el && event.clientX >= el.getBoundingClientRect().right - 20) interruptFollow();
          }}
          onKeyDown={(event) => {
            if (["ArrowUp", "PageUp", "Home"].includes(event.key)) interruptFollow();
          }}
        >
          {children}
        </ScrollArea>
      </MarkdownLinkProvider>

      {jumpPillMounted && (
        <div
          className={cn(
            "pointer-events-none absolute inset-x-0 bottom-3 z-10 flex justify-center transition-[opacity,scale,translate]",
            jumpPillVisible
              ? "translate-y-0 scale-100 opacity-100 duration-[160ms] ease-[var(--ease-out-expo)]"
              : "translate-y-1.5 scale-95 opacity-0 duration-[115ms] ease-[var(--ease-exit)]",
          )}
        >
          <button
            type="button"
            aria-hidden={!jumpPillVisible}
            tabIndex={jumpPillVisible ? 0 : -1}
            onClick={() => scrollToLatest(true)}
            className={cn(
              "orbit-press floating-surface border-border-default bg-surface-raised text-ink-primary hover:border-border-strong hover:bg-accent inline-flex cursor-pointer items-center gap-1.5 rounded-full border px-3 py-1 text-[11px] font-medium",
              jumpPillVisible ? "pointer-events-auto" : "pointer-events-none",
            )}
          >
            <ArrowDown className="size-3" />
            <span>Jump to latest</span>
          </button>
        </div>
      )}
    </div>
  );
});

/**
 * Shared user message bubble matching `UserPromptBubble` parity:
 * bottom fade preview when overflowing, click anywhere on the bubble to expand,
 * and click anywhere outside the bubble (including left blank gutter) to collapse.
 */
export const ConversationUserBubble = memo(function ConversationUserBubble({
  text,
  compact = false,
}: {
  text: string;
  compact?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  const bubbleRef = useRef<HTMLDivElement | null>(null);
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const conversationFontSize = useOrbitSelector((s) => s.conversationFontSize);
  const previewMaxPx = useMemo(
    () => Math.round(USER_PROMPT_PREVIEW_MAX_PX * (conversationFontSize / 15)),
    [conversationFontSize],
  );

  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    if (!mayUserPromptOverflow(text)) {
      setOverflowing((prev) => (prev ? false : prev));
      return;
    }
    const measure = () => {
      const next = el.scrollHeight > previewMaxPx + 10;
      setOverflowing((prev) => (prev === next ? prev : next));
    };
    measure();
    return observeSharedBubbleResize(el, measure);
  }, [text, previewMaxPx]);

  useEffect(() => {
    if (!expanded) return;
    const handleClickOutside = (event: MouseEvent) => {
      if (!(event.target instanceof Element)) return;
      if (bubbleRef.current?.contains(event.target)) return;
      if (event.target.closest('[data-slot="dialog-content"], .orbit-dialog-overlay')) return;
      const sel = window.getSelection();
      if (
        sel &&
        !sel.isCollapsed &&
        sel.toString().trim().length > 0 &&
        bubbleRef.current &&
        sel.anchorNode &&
        bubbleRef.current.contains(sel.anchorNode)
      ) {
        return;
      }
      holdNearestViewport(bubbleRef.current, () => setExpanded(false), "top", {
        intent: "collapse",
      });
    };
    document.addEventListener("click", handleClickOutside);
    return () => document.removeEventListener("click", handleClickOutside);
  }, [expanded]);

  const handleBubbleClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!overflowing || expanded) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("a, button")) return;
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed && sel.toString().trim().length > 0) return;
    holdNearestViewport(bubbleRef.current, () => setExpanded(true), "top", {
      intent: "expand",
    });
  };

  return (
    <div className="flex justify-end">
      <div
        ref={bubbleRef}
        data-orbit-disclosure-root=""
        onClick={handleBubbleClick}
        className={cn(
          "prompt-bubble-surface selectable-text text-foreground relative max-w-[90%] overflow-hidden rounded-[20px] whitespace-pre-wrap break-words",
          compact
            ? "px-3.5 py-2 text-[13px] leading-[1.5]"
            : "px-4 py-2.5 font-(family-name:--orbit-font-content) text-(length:--orbit-read-body) leading-(--orbit-read-leading)",
          overflowing && !expanded && "cursor-pointer",
        )}
      >
        <div
          ref={bodyRef}
          style={
            !expanded && overflowing
              ? { maxHeight: `${previewMaxPx}px`, overflow: "hidden" }
              : undefined
          }
        >
          {text}
        </div>
        {overflowing && (
          <div
            aria-hidden="true"
            className={cn(
              "pointer-events-none absolute inset-x-0 bottom-0 h-14 rounded-b-[19px] transition-opacity duration-150 ease-[var(--ease-out-quart)]",
              expanded ? "opacity-0" : "opacity-100",
            )}
            style={{
              background:
                "linear-gradient(to top, var(--surface-raised) 14%, color-mix(in oklab, var(--surface-raised) 82%, transparent) 52%, transparent 100%)",
            }}
          />
        )}
      </div>
    </div>
  );
});

/**
 * Shared assistant message footer with one-click copy, metadata label, and optional trailing action.
 */
export const ConversationMessageFooter = memo(function ConversationMessageFooter({
  copyText,
  metaLabel,
  rightSlot,
}: {
  copyText: string;
  metaLabel?: string;
  rightSlot?: ReactNode;
}) {
  const [copied, setCopied] = useState(false);
  const copyTimerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
        copyTimerRef.current = null;
      }
    };
  }, []);

  const handleCopy = () => {
    void navigator.clipboard.writeText(copyText).then(() => {
      setCopied(true);
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
      }
      copyTimerRef.current = window.setTimeout(() => {
        copyTimerRef.current = null;
        setCopied(false);
      }, 1400);
    });
  };

  return (
    <div className="flex items-center justify-between gap-2 pt-0.5">
      <div className="flex min-w-0 items-center gap-1.5">
        <button
          type="button"
          title="Copy response"
          onClick={handleCopy}
          className="orbit-press text-muted-foreground hover:text-foreground hover:bg-accent relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md"
        >
          <Copy
            className={cn(
              "size-[12px] transition-[opacity,scale,rotate] duration-150 ease-[var(--ease-out-expo)]",
              copied ? "scale-50 -rotate-12 opacity-0" : "scale-100 rotate-0 opacity-100",
            )}
          />
          <Check
            className={cn(
              "text-signal-success absolute size-[12px] transition-[opacity,scale,rotate] duration-150 ease-[var(--ease-out-expo)]",
              copied ? "scale-100 rotate-0 opacity-100" : "scale-50 rotate-12 opacity-0",
            )}
          />
        </button>
        {metaLabel && (
          <span className="text-muted-foreground/75 truncate text-[11px]">{metaLabel}</span>
        )}
      </div>
      {rightSlot && <div className="flex shrink-0 items-center gap-1">{rightSlot}</div>}
    </div>
  );
});
