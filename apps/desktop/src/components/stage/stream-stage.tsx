import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
} from "react";
import {
  observeElementOffset,
  useVirtualizer,
  type Range,
  type ReactVirtualizer,
  type Virtualizer,
} from "@tanstack/react-virtual";
import { ArrowDown, GitBranch, RotateCcw, RotateCw } from "lucide-react";
import { OmniComposer } from "@/components/composer/omni-composer";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import { DecisionDock } from "@/components/stage/stage-approval-dock";
import { ChatOutlineRail } from "@/components/stage/stage-outline-rail";
import { StageOverviewPanel } from "@/components/stage/stage-overview-panel";
import { ParentSubagentBanner } from "@/components/stage/subagents-track";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  type ArtifactSummary,
  type PromptBadge,
  type StageTurnView,
  type TimelineItemView,
  isAgentForegroundRunning,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isAgentInGitRepo } from "@/lib/projects";
import {
  type ResolvedSubagentItem,
  type SubagentNameMatcher,
  areResolvedSubagentsEqual,
  buildSubagentNameMatcher,
} from "@/lib/subagents";
import { computePrependRestoredScrollTop } from "@/lib/stage-pagination";
import { stripWorkspacePrefix } from "@/lib/timeline-coalesce";
import { cn } from "@/lib/utils";
import {
  type SessionSubagentWithAvatar,
  selectPendingDecisions,
  selectSessionSubagents,
} from "@/state/selectors/session-selectors";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import {
  EMPTY_ARTIFACTS,
  EMPTY_DIFF_FILES,
  EMPTY_INSIGHTS,
  EMPTY_TURNS,
  FAST_FLICK_PX_PER_SEC,
  IDLE_LOOKAHEAD_DOWN_PX,
  IDLE_LOOKAHEAD_UP_PX,
  IDLE_PREWARM_BATCH,
  MAX_OVERSCAN_ITEMS_PER_SIDE,
  type OutlinePromptWithIndex,
  type PromptImagePreviewTarget,
  type RewindFileEntry,
  type RewindRequestTarget,
  SCROLL_FAST_LEADING_PX,
  SCROLL_LEADING_PX,
  SCROLL_TRAILING_PX,
  type ScrollVelocityState,
  type StageScrollUiStore,
  TURN_LIST_GAP_PX,
  type ThinkingDurationLookupFn,
  type TurnTimingRecord,
  type ViewportAnchorHoldFn,
  WARM_RESTING_CAP,
  WARM_SCROLLING_CAP,
  areArtifactListsEquivalent,
  createStageScrollUiStore,
  estimateTurnSizeFallback,
  extractUserPromptPresentation,
  findTurnIndexForSeq,
  getCachedHistoricalThinkingSignature,
  getCachedTurnHeight,
  isRevertibleWorkspaceFile,
  setCachedTurnHeight,
} from "./stage-helpers";
import {
  ForkedFromBanner,
  PromptImageLightbox,
  RewindTurnDialog,
} from "./stage-prompt-controls";
import { TurnStageSection } from "./stage-turn-card";

export { PromptImageLightbox, TurnStageSection };
export type { PromptImagePreviewTarget };

const StageHydratingFallback = memo(function StageHydratingFallback({
  bootstrapError,
}: {
  bootstrapError: string | null;
}) {
  const [showIndicator, setShowIndicator] = useState(false);
  const [showSlowRetry, setShowSlowRetry] = useState(false);
  const [retrying, setRetrying] = useState(false);

  useEffect(() => {
    const showTimer = window.setTimeout(() => setShowIndicator(true), 350);
    const slowTimer = window.setTimeout(() => setShowSlowRetry(true), 5000);
    return () => {
      window.clearTimeout(showTimer);
      window.clearTimeout(slowTimer);
    };
  }, []);

  if (!bootstrapError && !showIndicator) {
    return <div className="flex-1" />;
  }

  const needsRetry = Boolean(bootstrapError) || showSlowRetry;

  return (
    <div className="orbit-stage-enter text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2.5 px-6 text-[13px] select-none">
      <div className="flex items-center gap-2">
        {!bootstrapError && (
          <OrbitSpinner size={13} className="text-muted-foreground/75 shrink-0" />
        )}
        <span className="text-muted-foreground/85">
          {bootstrapError
            ? "Unable to load workspace"
            : showSlowRetry
              ? "Taking longer than usual to connect…"
              : "Connecting to workspace…"}
        </span>
      </div>
      {needsRetry && (
        <button
          type="button"
          disabled={retrying}
          onClick={() => {
            setRetrying(true);
            setShowSlowRetry(false);
            void orbitActions.retryBootstrap().finally(() => {
              setRetrying(false);
            });
          }}
          className="border-border/80 text-foreground/80 hover:bg-accent hover:text-foreground inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
        >
          <RotateCw className={cn("size-3", retrying && "animate-spin")} />
          <span>{retrying ? "Retrying…" : "Retry"}</span>
        </button>
      )}
    </div>
  );
});

const NewSessionLaunchpad = memo(function NewSessionLaunchpad() {
  return (
    <div className="orbit-stage-enter flex h-full w-full flex-col justify-center px-[24px] py-12 pb-[10vh]">
      <div className="mx-auto w-full max-w-[var(--orbit-stage-max-w,740px)] space-y-6">
        <h1 className="text-ink-primary px-1 text-center text-[24px] font-medium tracking-[-0.02em]">
          What would you like to build?
        </h1>

        <OmniComposer variant="hero" />
      </div>
    </div>
  );
});

const StageOutlineRailBridge = memo(function StageOutlineRailBridge({
  scrollUiStore,
  prompts,
  onJumpToSeq,
  visible,
  offsetPx,
  offsetTransition,
  isAnimating,
}: {
  scrollUiStore: StageScrollUiStore;
  prompts: OutlinePromptWithIndex[];
  onJumpToSeq: (seq: number) => void;
  visible: boolean;
  offsetPx: number;
  offsetTransition: string;
  isAnimating: boolean;
}) {
  const activePromptSeq = useSyncExternalStore(
    scrollUiStore.subscribe,
    () => scrollUiStore.getSnapshot().activePromptSeq,
    () => scrollUiStore.getSnapshot().activePromptSeq,
  );
  return (
    <ChatOutlineRail
      prompts={prompts}
      activeSeq={activePromptSeq ?? prompts[prompts.length - 1]?.seq ?? null}
      onJumpToSeq={onJumpToSeq}
      visible={visible}
      offsetPx={offsetPx}
      offsetTransition={offsetTransition}
      isAnimating={isAnimating}
    />
  );
});

const STAGE_TOP_PAGINATE_THRESHOLD_PX = 480;

const StageBottomViewportOverlay = memo(function StageBottomViewportOverlay({
  scrollUiStore,
  stagePushStyle,
  overviewReservePx,
  overviewReserveTransition,
  onJumpLatest,
}: {
  scrollUiStore: StageScrollUiStore;
  stagePushStyle: CSSProperties;
  overviewReservePx: number;
  overviewReserveTransition: string;
  onJumpLatest: () => void;
}) {
  const snap = useSyncExternalStore(
    scrollUiStore.subscribe,
    scrollUiStore.getSnapshot,
    scrollUiStore.getSnapshot,
  );
  const showFadeMask = snap.showJumpLatest || snap.isOccludingText;

  return (
    <div className="pointer-events-none absolute right-0 bottom-0 left-0 z-10">
      {/* Subtle 24px bottom feather above Composer — never obscures streaming lines */}
      <div
        style={{
          paddingRight: `${Math.max(20, overviewReservePx)}px`,
          transition: overviewReserveTransition,
        }}
        className="pointer-events-none absolute right-0 bottom-0 left-0"
      >
        <div
          className={cn(
            "from-surface-base via-surface-base/75 h-6 w-full bg-gradient-to-t to-transparent transition-opacity duration-200 ease-out",
            showFadeMask ? "opacity-100" : "opacity-0",
          )}
        />
      </div>

      {/* Centered Jump-to-Latest circular button — zero collision since Subagent capsule moved to Overview/TopBar */}
      <div
        style={{
          paddingRight: `${overviewReservePx}px`,
          transition: overviewReserveTransition,
        }}
        className="pointer-events-none relative"
      >
        <div style={stagePushStyle} className="pointer-events-none relative px-[24px]">
          <div className="pointer-events-none relative mx-auto flex w-full max-w-[var(--orbit-stage-max-w,740px)] items-end justify-center">
            <button
              type="button"
              aria-label="Scroll to latest"
              aria-hidden={!snap.showJumpLatest}
              tabIndex={snap.showJumpLatest ? 0 : -1}
              onClick={onJumpLatest}
              className={cn(
                "border-border-default bg-surface-raised text-ink-tertiary hover:text-ink-primary hover:border-border-strong floating-surface absolute bottom-3 left-1/2 inline-flex size-[32px] -translate-x-1/2 cursor-pointer items-center justify-center rounded-full border transition-[transform,opacity,color,border-color] duration-200 ease-[var(--ease-out-expo)]",
                snap.showJumpLatest
                  ? "pointer-events-auto translate-y-0 scale-100 opacity-100"
                  : "pointer-events-none translate-y-1 scale-95 opacity-0",
              )}
            >
              <ArrowDown className="size-[16px] stroke-[1.9]" />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
});

const StageDockedComposer = memo(function StageDockedComposer({
  scrollUiStore,
}: {
  scrollUiStore: StageScrollUiStore;
}) {
  const isOccludingText = useSyncExternalStore(
    scrollUiStore.subscribe,
    () => scrollUiStore.getSnapshot().isOccludingText,
    () => scrollUiStore.getSnapshot().isOccludingText,
  );
  return <OmniComposer variant="docked" occluding={isOccludingText} />;
});

const VirtualizedTurnList = memo(function VirtualizedTurnList({
  selectedHostId,
  selectedAgentId,
  stageTurns,
  highlightedTurnKey,
  hasPendingJump,
  isLatestTurnActive,
  agentStatusLabel,
  hasLiveBackgroundTasks,
  baselineMaxSeq,
  isNetworkOffline,
  autoFoldCompleted,
  effectiveCwd,
  stageMaxContentWidthPx,
  hostArtifacts,
  timingTick,
  scrollRef,
  stickToBottomRef,
  holdActiveRef,
  virtualizerRef,
  turnTimingsRef,
  thinkingTimingsRef,
  getThinkingDurationMs,
  holdViewport,
  onRequestRewind,
  onRequestFork,
  canForkWorktree,
  onPreviewImage,
  sessionSubagents,
  sessionSubagentMatcher,
  onOpenSubagent,
  queuedNotificationCount,
  onInterruptAndContinue,
}: {
  selectedHostId: string;
  selectedAgentId: string;
  stageTurns: StageTurnView[];
  highlightedTurnKey: string | null;
  hasPendingJump: boolean;
  isLatestTurnActive: boolean;
  agentStatusLabel: string | null;
  hasLiveBackgroundTasks: boolean;
  baselineMaxSeq: number;
  isNetworkOffline: boolean;
  autoFoldCompleted: boolean;
  effectiveCwd: string;
  stageMaxContentWidthPx: number | null;
  hostArtifacts: readonly ArtifactSummary[];
  timingTick: number;
  scrollRef: React.RefObject<HTMLDivElement | null>;
  stickToBottomRef: React.RefObject<boolean>;
  holdActiveRef: React.RefObject<boolean>;
  virtualizerRef: React.MutableRefObject<ReactVirtualizer<HTMLDivElement, HTMLDivElement> | null>;
  turnTimingsRef: React.RefObject<Record<string, TurnTimingRecord>>;
  thinkingTimingsRef: React.RefObject<Record<string, TurnTimingRecord>>;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  holdViewport: ViewportAnchorHoldFn;
  onRequestRewind: (seq: number, promptText: string, badges: readonly PromptBadge[]) => void;
  onRequestFork: (upToSeq: number | null, worktree: boolean) => void;
  canForkWorktree: boolean;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
  sessionSubagents: readonly ResolvedSubagentItem[];
  sessionSubagentMatcher: SubagentNameMatcher | null;
  onOpenSubagent: (subagent: ResolvedSubagentItem) => void;
  queuedNotificationCount: number;
  onInterruptAndContinue: (() => void) | null;
}) {
  const widthBucket = stageMaxContentWidthPx ? Math.round(stageMaxContentWidthPx / 32) : 24;
  const selectionTurnsRef = useRef<number[]>([]);
  const warmKeysRef = useRef<Set<string>>(new Set());
  const [warmRevision, setWarmRevision] = useState(0);
  const scrollVelocityRef = useRef<ScrollVelocityState>({
    offset: 0,
    ts: 0,
    dir: null,
    fast: false,
    wasScrolling: false,
  });
  const localVirtualizerRef = useRef<ReactVirtualizer<HTMLDivElement, HTMLDivElement> | null>(null);

  const turnsRef = useRef(stageTurns);
  turnsRef.current = stageTurns;
  const activeRef = useRef(isLatestTurnActive);
  activeRef.current = isLatestTurnActive;

  useEffect(() => {
    const onSelectionChange = () => {
      const sel = window.getSelection();
      if (!sel || sel.isCollapsed || sel.rangeCount === 0) {
        selectionTurnsRef.current = [];
        return;
      }
      const getTurnIndex = (node: Node | null): number | null => {
        const el = node instanceof Element ? node : (node?.parentElement ?? null);
        const host = el?.closest<HTMLElement>("[data-orbit-turn-index]");
        if (!host) return null;
        const parsed = Number.parseInt(host.dataset.orbitTurnIndex ?? "", 10);
        return Number.isNaN(parsed) ? null : parsed;
      };
      const aIdx = getTurnIndex(sel.anchorNode);
      const fIdx = getTurnIndex(sel.focusNode);
      const pins: number[] = [];
      if (aIdx !== null && fIdx !== null) {
        const lo = Math.min(aIdx, fIdx);
        const hi = Math.max(aIdx, fIdx);
        if (hi - lo <= 8) {
          for (let k = lo; k <= hi; k += 1) pins.push(k);
        } else {
          pins.push(aIdx, fIdx);
        }
      } else {
        if (aIdx !== null) pins.push(aIdx);
        if (fIdx !== null && fIdx !== aIdx) pins.push(fIdx);
      }
      selectionTurnsRef.current = pins;
    };
    document.addEventListener("selectionchange", onSelectionChange);
    return () => document.removeEventListener("selectionchange", onSelectionChange);
  }, []);

  const readScrollVelocity = useCallback(
    (v: Virtualizer<HTMLDivElement, HTMLDivElement> | null): ScrollVelocityState => {
      const prev = scrollVelocityRef.current;
      if (!v) return prev;
      const offset = v.scrollOffset ?? 0;
      const isScrolling = Boolean(v.isScrolling);
      if (offset === prev.offset && isScrolling === prev.wasScrolling) {
        return prev;
      }

      const now = performance.now();
      const dt = now - prev.ts;
      const delta = offset - prev.offset;
      let dir = prev.dir;
      let fast = prev.fast;

      if (!isScrolling) {
        dir = null;
        fast = false;
      } else if (Math.abs(delta) > 0.5) {
        dir = delta < 0 ? "up" : "down";
        if (dt >= 2 && dt <= 250) {
          const speed = (Math.abs(delta) * 1000) / dt;
          fast = speed >= FAST_FLICK_PX_PER_SEC;
        } else if (dt > 250) {
          fast = false;
        }
      }

      const next: ScrollVelocityState = {
        offset,
        ts: now,
        dir,
        fast,
        wasScrolling: isScrolling,
      };
      scrollVelocityRef.current = next;
      return next;
    },
    [],
  );

  const rangeExtractor = useCallback(
    (range: Range): number[] => {
      const turns = turnsRef.current;
      const total = turns.length;
      if (total === 0) {
        warmKeysRef.current.clear();
        return [];
      }

      const v = localVirtualizerRef.current;
      const measurements = v?.measurementsCache;
      const isScrolling = Boolean(v?.isScrolling);
      const vel = readScrollVelocity(v);

      let upBudgetPx = IDLE_LOOKAHEAD_UP_PX;
      let downBudgetPx = IDLE_LOOKAHEAD_DOWN_PX;
      if (vel.dir === "up") {
        upBudgetPx = vel.fast ? SCROLL_FAST_LEADING_PX : SCROLL_LEADING_PX;
        downBudgetPx = SCROLL_TRAILING_PX;
      } else if (vel.dir === "down") {
        upBudgetPx = SCROLL_TRAILING_PX;
        downBudgetPx = vel.fast ? SCROLL_FAST_LEADING_PX : SCROLL_LEADING_PX;
      }

      const clampedStart = Math.max(0, Math.min(range.startIndex, total - 1));
      const clampedEnd = Math.max(clampedStart, Math.min(range.endIndex, total - 1));

      let windowStart = clampedStart;
      let accUp = 0;
      while (
        windowStart > 0 &&
        accUp < upBudgetPx &&
        clampedStart - windowStart < MAX_OVERSCAN_ITEMS_PER_SIDE
      ) {
        const prevIdx = windowStart - 1;
        const size = measurements?.[prevIdx]?.size ?? estimateTurnSizeFallback(turns[prevIdx]);
        accUp += size + TURN_LIST_GAP_PX;
        windowStart = prevIdx;
      }

      let windowEnd = clampedEnd;
      let accDown = 0;
      while (
        windowEnd < total - 1 &&
        accDown < downBudgetPx &&
        windowEnd - clampedEnd < MAX_OVERSCAN_ITEMS_PER_SIDE
      ) {
        const nextIdx = windowEnd + 1;
        const size = measurements?.[nextIdx]?.size ?? estimateTurnSizeFallback(turns[nextIdx]);
        accDown += size + TURN_LIST_GAP_PX;
        windowEnd = nextIdx;
      }

      const activeSet = new Set<number>();
      for (let i = windowStart; i <= windowEnd; i += 1) {
        activeSet.add(i);
      }
      if (activeRef.current) {
        activeSet.add(total - 1);
      }
      for (const idx of selectionTurnsRef.current) {
        if (idx >= 0 && idx < total) {
          activeSet.add(idx);
        }
      }

      const prevWarmKeys = warmKeysRef.current;
      const nextWarmKeys = new Set<string>();
      const merged = new Set<number>(activeSet);
      for (const idx of activeSet) {
        nextWarmKeys.add(turns[idx].key);
      }
      if (prevWarmKeys.size > 0) {
        for (let i = 0; i < total; i += 1) {
          const key = turns[i].key;
          if (prevWarmKeys.has(key)) {
            merged.add(i);
            nextWarmKeys.add(key);
          }
        }
      }

      const cap = isScrolling ? WARM_SCROLLING_CAP : WARM_RESTING_CAP;
      if (merged.size > cap) {
        const center = (clampedStart + clampedEnd) * 0.5;
        const evictable: number[] = [];
        for (const idx of merged) {
          if (!activeSet.has(idx)) {
            evictable.push(idx);
          }
        }
        evictable.sort((a, b) => Math.abs(b - center) - Math.abs(a - center));
        for (const idx of evictable) {
          if (merged.size <= cap) break;
          merged.delete(idx);
          nextWarmKeys.delete(turns[idx].key);
        }
      }

      warmKeysRef.current = nextWarmKeys;
      return Array.from(merged).sort((a, b) => a - b);
    },
    [readScrollVelocity, warmRevision],
  );

  const getItemKey = useCallback(
    (index: number) => turnsRef.current[index]?.key ?? index,
    [],
  );

  const estimateSize = useCallback(
    (index: number) => {
      const turn = turnsRef.current[index];
      if (!turn) return 180;
      const cached = getCachedTurnHeight(`${selectedAgentId}::${turn.key}::${widthBucket}`);
      return cached ?? estimateTurnSizeFallback(turn);
    },
    [selectedAgentId, widthBucket],
  );

  const handleVirtualizerChange = useCallback(
    (instance: Virtualizer<HTMLDivElement, HTMLDivElement>) => {
      const wasScrolling = scrollVelocityRef.current.wasScrolling;
      readScrollVelocity(instance);
      if (wasScrolling && !instance.isScrolling && warmKeysRef.current.size > WARM_RESTING_CAP) {
        setWarmRevision((n) => n + 1);
      }
    },
    [readScrollVelocity],
  );

  const initialRect = useMemo(() => {
    const el = scrollRef.current;
    const width =
      el && el.clientWidth > 0
        ? el.clientWidth
        : typeof window !== "undefined"
          ? Math.max(360, window.innerWidth)
          : 1280;
    const height =
      el && el.clientHeight > 0
        ? el.clientHeight
        : typeof window !== "undefined"
          ? Math.max(400, window.innerHeight - 160)
          : 720;
    return { width, height };
    // Compute initial viewport estimate only when mounting a fresh session list
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedAgentId]);

  const hasPendingJumpRef = useRef(hasPendingJump);
  hasPendingJumpRef.current = hasPendingJump;

  const initialBottomOffset = useMemo(() => {
    if (hasPendingJump || stageTurns.length === 0) return 0;
    let total = 0;
    for (let i = 0; i < stageTurns.length; i += 1) {
      const turn = stageTurns[i];
      const cached = getCachedTurnHeight(`${selectedAgentId}::${turn.key}::${widthBucket}`);
      total += (cached ?? estimateTurnSizeFallback(turn)) + TURN_LIST_GAP_PX;
    }
    return Math.max(0, total - initialRect.height);
    // Compute initial bottom anchor only when mounting a fresh session list
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedAgentId, initialRect.height, hasPendingJump]);

  const observeOffsetWithBottomSeed = useCallback(
    (
      instance: Virtualizer<HTMLDivElement, HTMLDivElement>,
      cb: (offset: number, isScrolling: boolean) => void,
    ) => {
      const el = instance.scrollElement;
      if (
        el &&
        stickToBottomRef.current &&
        !hasPendingJumpRef.current &&
        initialBottomOffset > 0 &&
        el.scrollTop === 0
      ) {
        const maxTop = Math.max(0, el.scrollHeight - el.clientHeight);
        if (maxTop > 0) {
          el.scrollTop = maxTop;
        }
      }
      return observeElementOffset(instance, cb);
    },
    [initialBottomOffset, stickToBottomRef],
  );

  const virtualizer = useVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: stageTurns.length,
    getScrollElement: () => scrollRef.current,
    getItemKey,
    estimateSize,
    initialRect,
    initialOffset: initialBottomOffset,
    observeElementOffset: observeOffsetWithBottomSeed,
    rangeExtractor,
    onChange: handleVirtualizerChange,
    gap: TURN_LIST_GAP_PX,
    overscan: 0,
    useScrollendEvent: true,
    directDomUpdates: true,
  });

  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = (item, _delta, instance) => {
    if (holdActiveRef.current) {
      return false;
    }
    const scrollOffset = instance.scrollOffset ?? 0;
    if (stickToBottomRef.current) {
      return item.end <= scrollOffset;
    }
    const key = String(item.key);
    const alreadyMeasured = instance.itemSizeCache.has(key);
    return alreadyMeasured ? item.end <= scrollOffset : item.start < scrollOffset;
  };

  localVirtualizerRef.current = virtualizer;
  virtualizerRef.current = virtualizer;
  useEffect(() => {
    return () => {
      if (localVirtualizerRef.current === virtualizer) {
        localVirtualizerRef.current = null;
      }
      if (virtualizerRef.current === virtualizer) {
        virtualizerRef.current = null;
      }
    };
  }, [virtualizer, virtualizerRef]);

  // Gently pre-warm offscreen preceding turns during idle time (never while streaming or scrolling)
  useEffect(() => {
    if (isLatestTurnActive || stageTurns.length <= 2) return;

    let cancelled = false;
    let initialTimer: number | null = null;
    let fallbackTimer: number | null = null;
    let idleHandle: number | null = null;

    const scheduleStep = () => {
      if (cancelled) return;
      if (typeof window.requestIdleCallback === "function") {
        idleHandle = window.requestIdleCallback(runStep, { timeout: 180 });
      } else {
        fallbackTimer = window.setTimeout(runStep, 48);
      }
    };

    const runStep = () => {
      idleHandle = null;
      fallbackTimer = null;
      if (cancelled) return;

      if (localVirtualizerRef.current?.isScrolling) {
        scheduleStep();
        return;
      }

      const turns = turnsRef.current;
      const warm = warmKeysRef.current;
      if (turns.length === 0 || warm.size >= WARM_RESTING_CAP) {
        return;
      }

      let lowestMounted = turns.length;
      for (let i = 0; i < turns.length; i += 1) {
        if (warm.has(turns[i].key)) {
          lowestMounted = i;
          break;
        }
      }

      let added = 0;
      for (
        let idx = lowestMounted - 1;
        idx >= 0 && added < IDLE_PREWARM_BATCH && warm.size < WARM_RESTING_CAP;
        idx -= 1
      ) {
        const key = turns[idx].key;
        if (!warm.has(key)) {
          warm.add(key);
          added += 1;
        }
      }

      if (added > 0) {
        setWarmRevision((n) => n + 1);
        if (warm.size < WARM_RESTING_CAP) {
          scheduleStep();
        }
      }
    };

    initialTimer = window.setTimeout(scheduleStep, 220);

    return () => {
      cancelled = true;
      if (initialTimer !== null) window.clearTimeout(initialTimer);
      if (fallbackTimer !== null) window.clearTimeout(fallbackTimer);
      if (idleHandle !== null && typeof window.cancelIdleCallback === "function") {
        window.cancelIdleCallback(idleHandle);
      }
    };
  }, [selectedAgentId, stageTurns.length, isLatestTurnActive]);

  const measureTurnNode = useCallback(
    (el: HTMLDivElement | null) => {
      virtualizer.measureElement(el);
      if (el) {
        const idxAttr = el.getAttribute("data-index");
        const idx = idxAttr ? Number.parseInt(idxAttr, 10) : -1;
        const turn = idx >= 0 ? turnsRef.current[idx] : undefined;
        const cachedSize = turn ? virtualizer.itemSizeCache.get(turn.key) : undefined;
        if (turn && cachedSize && cachedSize > 0) {
          setCachedTurnHeight(`${selectedAgentId}::${turn.key}::${widthBucket}`, cachedSize);
        }
      }
    },
    [virtualizer, selectedAgentId, widthBucket],
  );

  const virtualItems = virtualizer.getVirtualItems();

  return (
    <div ref={virtualizer.containerRef} className="relative w-full min-w-0">
      {virtualItems.map((vItem) => {
        const turn = stageTurns[vItem.index];
        if (!turn) return null;
        const isLast = vItem.index === stageTurns.length - 1;
        const turnActive = isLast && isLatestTurnActive;
        const isFreshLivePrompt =
          turn.user_prompt?.item_type === "user_prompt" &&
          turn.user_prompt.seq > baselineMaxSeq;
        const timing = turnTimingsRef.current[`${selectedAgentId}::${turn.key}`];
        const thinkingSignature = getCachedHistoricalThinkingSignature(
          turn,
          selectedAgentId,
          thinkingTimingsRef.current,
          timingTick,
          turnActive,
        );

        const isHighlighted = highlightedTurnKey === turn.key;

        return (
          <div
            key={vItem.key}
            data-index={vItem.index}
            data-orbit-turn-index={vItem.index}
            ref={measureTurnNode}
            className={cn(
              "absolute top-0 left-0 w-full min-w-0 rounded-2xl transition-colors duration-700",
              isHighlighted &&
                "bg-foreground/[0.035] ring-1 ring-inset ring-foreground/[0.12] dark:bg-foreground/[0.055] dark:ring-foreground/[0.18]",
            )}
          >
            <TurnStageSection
              turn={turn}
              isTurnActive={turnActive}
              isLastTurn={isLast}
              agentStatusLabel={turnActive ? agentStatusLabel : null}
              hasLiveBackgroundTasks={turnActive && hasLiveBackgroundTasks}
              isNetworkOffline={isNetworkOffline}
              animatePromptEntrance={isFreshLivePrompt}
              autoFoldCompleted={autoFoldCompleted}
              hostId={selectedHostId}
              cwd={effectiveCwd}
              agentId={selectedAgentId}
              hostArtifacts={hostArtifacts}
              timing={timing}
              thinkingSignature={thinkingSignature}
              getThinkingDurationMs={getThinkingDurationMs}
              holdViewport={holdViewport}
              onRequestRewind={onRequestRewind}
              onRequestFork={onRequestFork}
              canForkWorktree={canForkWorktree}
              onPreviewImage={onPreviewImage}
              sessionSubagents={sessionSubagents}
              sessionSubagentMatcher={sessionSubagentMatcher}
              onOpenSubagent={onOpenSubagent}
              queuedNotificationCount={turnActive ? queuedNotificationCount : 0}
              onInterruptAndContinue={turnActive ? onInterruptAndContinue : null}
            />
          </div>
        );
      })}
    </div>
  );
});

export function StreamStage() {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const sessionScrollNonce = useOrbitSelector((s) => s.sessionScrollNonce);
  const pendingJumpTarget = useOrbitSelector((s) => s.pendingJumpTarget);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const [highlightedTurnKey, setHighlightedTurnKey] = useState<string | null>(null);
  const [jumpFallbackNonce, setJumpFallbackNonce] = useState<number | null>(null);
  const highlightTimerRef = useRef<number | null>(null);
  const jumpFallbackTimerRef = useRef<number | null>(null);
  const jumpRafRef = useRef<{ raf1: number | null; raf2: number | null }>({
    raf1: null,
    raf2: null,
  });
  const isNetworkOffline = useOrbitSelector((s) => s.isBrowserOffline);
  const {
    isStageWide,
    overviewReservePx,
    overviewReserveTransition,
    stageShiftPx,
    stageTransition,
    stageMaxContentWidthPx,
    outlineOffsetPx,
    outlineTransition,
    isAnimating,
    isResizing,
    canDockOverview,
    onStageTransitionEnd,
  } = useWorkspaceMotion();
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const fleet = useOrbitSelector((s) => s.fleet);
  const archivedSubagents = useOrbitSelector((s) => s.archivedSubagents);
  const hostDetail = selectedHostId ? (hostDetails[selectedHostId] ?? null) : null;
  const stageView = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedAgentId
      ? (s.stages[`${s.selectedHostId}::${s.selectedAgentId}`] ?? null)
      : null,
  );
  const wasSessionRewound = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedAgentId
      ? Boolean(s.rewoundSessions[`${s.selectedHostId}::${s.selectedAgentId}`])
      : false,
  );

  // Stabilize hostArtifacts across `host_changed` bursts so completed historical turns
  // never re-render when unrelated host metadata changes.
  const hostArtifactsRef = useRef<readonly ArtifactSummary[]>(EMPTY_ARTIFACTS);
  const rawHostArtifacts = hostDetail?.artifacts ?? EMPTY_ARTIFACTS;
  const stableHostArtifacts = useMemo(() => {
    if (areArtifactListsEquivalent(hostArtifactsRef.current, rawHostArtifacts)) {
      return hostArtifactsRef.current;
    }
    hostArtifactsRef.current = rawHostArtifacts;
    return rawHostArtifacts;
  }, [rawHostArtifacts]);

  const stageContainerRef = useRef<HTMLDivElement | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const composerShelfRef = useRef<HTMLDivElement | null>(null);
  const virtualizerRef = useRef<ReactVirtualizer<HTMLDivElement, HTMLDivElement> | null>(null);
  const prevComposerHeightRef = useRef<number>(0);
  const lastObservedScrollTopRef = useRef<number>(0);
  const stickToBottomRef = useRef<boolean>(true);
  const sessionEntryLockUntilRef = useRef<number>(0);
  const prevTurnsCountRef = useRef<number>(0);
  const holdActiveRef = useRef<boolean>(false);
  const programmaticScrollRef = useRef<boolean>(false);
  const programmaticScrollTimerRef = useRef<number | null>(null);
  const scrollRafRef = useRef<number | null>(null);
  const bottomSyncRafRef = useRef<number | null>(null);
  const lastOutlineSyncAtRef = useRef<number>(0);
  const scrollUiStoreRef = useRef<StageScrollUiStore | null>(null);
  if (scrollUiStoreRef.current === null) {
    scrollUiStoreRef.current = createStageScrollUiStore();
  }
  const scrollUiStore = scrollUiStoreRef.current;

  const turnTimingsRef = useRef<Record<string, TurnTimingRecord>>({});
  const thinkingTimingsRef = useRef<Record<string, TurnTimingRecord>>({});
  const mountedBaselineRef = useRef<{ agentId: string | null; maxSeq: number }>({
    agentId: null,
    maxSeq: 0,
  });

  const isStageWideRef = useRef(isStageWide);
  isStageWideRef.current = isStageWide;
  const isShellLayoutMovingRef = useRef(isAnimating || isResizing);
  isShellLayoutMovingRef.current = isAnimating || isResizing;
  const [timingTick, setTimingTick] = useState(0);
  const [rewindTarget, setRewindTarget] = useState<RewindRequestTarget | null>(null);
  const [rewindSubmitting, setRewindSubmitting] = useState(false);
  const [loadingEarlierTurns, setLoadingEarlierTurns] = useState(false);
  const loadingEarlierTurnsRef = useRef(false);
  const earlierLoadReqIdRef = useRef(0);
  const exhaustedEarlierWindowRef = useRef<{
    scope: string;
    windowStart: number;
    revision: number;
  } | null>(null);
  const pendingTurnPrependRef = useRef<{
    scope: string;
    prevScrollHeight: number;
    prevScrollTop: number;
  } | null>(null);
  const prevWindowStartRef = useRef<{
    scope: string | null;
    windowStart: number;
    firstTurnKey: string | null;
    turnCount: number;
  }>({
    scope: null,
    windowStart: 0,
    firstTurnKey: null,
    turnCount: 0,
  });
  const jumpEarlierAttemptedNonceRef = useRef<number | null>(null);
  const [imagePreviewTarget, setImagePreviewTarget] =
    useState<PromptImagePreviewTarget | null>(null);
  const handleCloseImagePreview = useCallback(() => setImagePreviewTarget(null), []);
  const handleRewindOpenChange = useCallback((open: boolean) => {
    if (!open) setRewindTarget(null);
  }, []);

  const getThinkingDurationMs = useCallback<ThinkingDurationLookupFn>((thinkKey) => {
    return thinkingTimingsRef.current[thinkKey]?.durationMs ?? null;
  }, []);

  const currentAgent = useMemo(
    () =>
      hostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ??
      stageView?.agent ??
      null,
    [hostDetail, selectedAgentId, stageView?.agent],
  );
  const isAgentRunning = isAgentForegroundRunning(currentAgent);
  const fleetHydrated = useOrbitSelector((s) => s.fleetHydrated);
  const bootstrapError = useOrbitSelector((s) => s.bootstrapError);

  const stageTurns = stageView?.turns ?? EMPTY_TURNS;
  const firstTurnKey = stageTurns[0]?.key ?? null;
  const firstTurnKeyRef = useRef(firstTurnKey);
  firstTurnKeyRef.current = firstTurnKey;
  const turnsLengthRef = useRef(stageTurns.length);
  turnsLengthRef.current = stageTurns.length;
  const windowStartTurnIndex = Math.max(0, stageView?.window_start_turn_index ?? 0);
  const windowStartTurnIndexRef = useRef(windowStartTurnIndex);
  windowStartTurnIndexRef.current = windowStartTurnIndex;
  const hasOlderHistory = Boolean(stageView?.has_older_history);
  const hasOlderHistoryRef = useRef(hasOlderHistory);
  hasOlderHistoryRef.current = hasOlderHistory;
  const stageRevisionRef = useRef(stageView?.revision ?? 0);
  stageRevisionRef.current = stageView?.revision ?? 0;
  const activeHostIdRef = useRef(selectedHostId);
  activeHostIdRef.current = selectedHostId;
  const activeAgentIdRef = useRef(selectedAgentId);
  activeAgentIdRef.current = selectedAgentId;
  const activeSessionScope =
    !isDraft && selectedHostId && selectedAgentId
      ? `${selectedHostId}::${selectedAgentId}`
      : null;
  const activeSessionScopeRef = useRef<string | null>(activeSessionScope);
  activeSessionScopeRef.current = activeSessionScope;
  const selectorSlice = useMemo(
    () => ({
      hostDetails,
      fleet,
      archivedSubagents,
      stages:
        selectedHostId && selectedAgentId && stageView
          ? { [`${selectedHostId}::${selectedAgentId}`]: stageView }
          : {},
    }),
    [hostDetails, fleet, archivedSubagents, selectedHostId, selectedAgentId, stageView],
  );

  const unifiedDecisions = useMemo(
    () => selectPendingDecisions(selectorSlice, selectedHostId, selectedAgentId),
    [selectorSlice, selectedHostId, selectedAgentId],
  );
  const pendingPermissions = unifiedDecisions.parentPermissions;
  const pendingQuestions = unifiedDecisions.parentQuestions;
  const subagentPermissions = unifiedDecisions.subagentPermissions;
  const sessionTurnDiffs = stageView?.session_turn_diffs ?? EMPTY_DIFF_FILES;
  const stageInsights = stageView?.insights ?? EMPTY_INSIGHTS;

  const sessionSubagentsRef = useRef<readonly SessionSubagentWithAvatar[]>([]);
  const sessionSubagents = useMemo(() => {
    const summary = selectSessionSubagents(selectorSlice, selectedHostId, selectedAgentId);
    const next = summary.all;
    if (
      areResolvedSubagentsEqual(sessionSubagentsRef.current, next) &&
      sessionSubagentsRef.current.length === next.length &&
      sessionSubagentsRef.current.every((s, idx) => s.character.id === next[idx]?.character.id)
    ) {
      return sessionSubagentsRef.current;
    }
    sessionSubagentsRef.current = next;
    return next;
  }, [selectorSlice, selectedHostId, selectedAgentId]);

  const sessionSubagentMatcher = useMemo(
    () => buildSubagentNameMatcher(sessionSubagents),
    [sessionSubagents],
  );

  const handleOpenSubagent = useCallback(
    (sa: ResolvedSubagentItem) => {
      if (sa.status === "awaiting_permission" && sa.isManaged && sa.childAgentId) {
        orbitActions.selectAgent(sa.childHostId, sa.childAgentId);
        return;
      }
      orbitActions.openSubagentTab({
        callId: sa.callId,
        role: sa.title,
        promptPreview: sa.promptPreview,
        childAgentId: sa.childAgentId,
        childHostId: sa.childHostId || undefined,
        providerSubagentId: sa.providerSubagentId,
        parentAgentId: selectedAgentId,
      });
    },
    [selectedAgentId],
  );

  const latestTurn = stageTurns[stageTurns.length - 1] ?? null;
  const isLatestTurnActive = Boolean(latestTurn && isAgentRunning);

  // Record live turn & per-thinking-block start/completion durations for the active session
  useEffect(() => {
    if (!selectedAgentId || !latestTurn) return;
    let changed = false;
    const now = Date.now();

    const timingKey = `${selectedAgentId}::${latestTurn.key}`;
    const existing = turnTimingsRef.current[timingKey];
    const seededStartMs =
      latestTurn.started_at_ms !== null && latestTurn.started_at_ms <= now
        ? latestTurn.started_at_ms
        : now;

    if (isLatestTurnActive) {
      if (!existing || existing.completedAtMs !== null) {
        turnTimingsRef.current[timingKey] = {
          startedAtMs: seededStartMs,
          completedAtMs: null,
          durationMs: null,
          workDurationMs: null,
        };
        changed = true;
      } else if (latestTurn.terminal_reply !== null && existing.workDurationMs === null) {
        turnTimingsRef.current[timingKey] = {
          ...existing,
          workDurationMs: Math.max(0, now - existing.startedAtMs),
        };
        changed = true;
      } else if (latestTurn.terminal_reply === null && existing.workDurationMs !== null) {
        turnTimingsRef.current[timingKey] = {
          ...existing,
          workDurationMs: null,
        };
        changed = true;
      }
    } else if (existing && existing.completedAtMs === null) {
      const totalMs = Math.max(0, now - existing.startedAtMs);
      turnTimingsRef.current[timingKey] = {
        ...existing,
        completedAtMs: now,
        durationMs: totalMs,
        workDurationMs: existing.workDurationMs ?? totalMs,
      };
      changed = true;
    }

    const lastSeg = latestTurn.live_segments[latestTurn.live_segments.length - 1] ?? null;
    const lastSegItem =
      lastSeg?.kind === "step_group" ? (lastSeg.items[lastSeg.items.length - 1] ?? null) : null;

    for (const seg of latestTurn.live_segments) {
      if (seg.kind !== "step_group") continue;
      for (const item of seg.items) {
        if (item.item_type !== "assistant_thinking") continue;

        const thinkKey = `${selectedAgentId}::think::${item.item_id || item.seq}`;
        const isCurrentlyThinking =
          isLatestTurnActive &&
          lastSegItem?.item_type === "assistant_thinking" &&
          lastSegItem.seq === item.seq;
        const prevThink = thinkingTimingsRef.current[thinkKey];

        if (isCurrentlyThinking) {
          if (!prevThink) {
            thinkingTimingsRef.current[thinkKey] = {
              startedAtMs: now,
              completedAtMs: null,
              durationMs: null,
              workDurationMs: null,
            };
            changed = true;
          }
        } else if (prevThink && prevThink.completedAtMs === null) {
          thinkingTimingsRef.current[thinkKey] = {
            ...prevThink,
            completedAtMs: now,
            durationMs: Math.max(0, now - prevThink.startedAtMs),
            workDurationMs: null,
          };
          changed = true;
        }
      }
    }

    if (changed) {
      setTimingTick((n) => n + 1);
    }
  }, [selectedAgentId, latestTurn, isLatestTurnActive]);

  const queuedNotificationCount = useMemo(() => {
    if (!isLatestTurnActive || !latestTurn) return 0;
    let count = 0;
    if (
      latestTurn.agent_message?.item_type === "agent_message" &&
      latestTurn.agent_message.delivery_status === "queued"
    ) {
      count += 1;
    }
    for (const seg of latestTurn.live_segments) {
      if (
        seg.kind === "agent_message" &&
        seg.item.item_type === "agent_message" &&
        seg.item.delivery_status === "queued"
      ) {
        count += 1;
      }
    }
    return count;
  }, [isLatestTurnActive, latestTurn]);

  const handleInterruptAndContinue = useCallback(() => {
    if (!selectedHostId || !selectedAgentId) return;
    void orbitIpc.interruptAgentAndContinue(selectedHostId, selectedAgentId);
  }, [selectedHostId, selectedAgentId]);

  const stagePushStyle = useMemo<CSSProperties>(
    () => ({
      transform: stageShiftPx !== 0 ? `translate3d(${stageShiftPx}px, 0, 0)` : undefined,
      transition: stageTransition,
      willChange: isAnimating ? "transform" : undefined,
    }),
    [stageShiftPx, stageTransition, isAnimating],
  );

  const outlinePrompts = useMemo<OutlinePromptWithIndex[]>(() => {
    const out: OutlinePromptWithIndex[] = [];
    for (let turnIndex = 0; turnIndex < stageTurns.length; turnIndex += 1) {
      const t = stageTurns[turnIndex];
      if (t.user_prompt?.item_type === "user_prompt" && !t.is_compaction_command) {
        const promptItem = t.user_prompt as Extract<TimelineItemView, { item_type: "user_prompt" }>;
        const pres = extractUserPromptPresentation(promptItem);
        const summary =
          pres.cleanText || pres.badges.map((b) => b.label).join(" ") || "Prompt";
        out.push({
          seq: t.anchor_seq,
          turnIndex,
          preview: summary.replace(/\s+/g, " ").trim().slice(0, 110),
        });
      }
    }
    return out;
  }, [stageTurns]);

  const outlinePromptsRef = useRef(outlinePrompts);
  outlinePromptsRef.current = outlinePrompts;
  const stageTurnsRef = useRef(stageTurns);
  stageTurnsRef.current = stageTurns;
  const turnsCountRef = useRef(stageTurns.length);
  turnsCountRef.current = stageTurns.length;

  const effectiveCwdRef = useRef("");
  effectiveCwdRef.current = selectedCwd || currentAgent?.cwd || "";

  const handleRequestRewind = useCallback(
    (seq: number, promptText: string, promptBadges: readonly PromptBadge[] = []) => {
      const activeCwd = effectiveCwdRef.current;
      const affected = stageTurnsRef.current.filter((t) => t.anchor_seq >= seq);
      const byPath = new Map<string, RewindFileEntry>();

      const recordFile = (rawPath: string, additions: number, deletions: number) => {
        if (!isRevertibleWorkspaceFile(rawPath, activeCwd)) return;
        const displayPath = stripWorkspacePrefix(rawPath, activeCwd).replace(/^\.\//, "");
        if (!displayPath) return;
        const prev = byPath.get(displayPath);
        if (prev) {
          prev.additions += additions;
          prev.deletions += deletions;
        } else {
          const basename = displayPath.split("/").pop() || displayPath;
          byPath.set(displayPath, {
            displayPath,
            basename,
            additions,
            deletions,
          });
        }
      };

      for (const turn of affected) {
        for (const diff of turn.turn_diffs) {
          if (diff.path) recordFile(diff.path, diff.additions, diff.deletions);
        }
        for (const seg of turn.live_segments) {
          if (seg.kind !== "step_group") continue;
          for (const step of seg.items) {
            if (
              step.item_type === "tool_call" &&
              step.detail.kind === "file_edit" &&
              step.detail.path &&
              step.diff_files.length === 0
            ) {
              recordFile(step.detail.path, 0, 0);
            }
          }
        }
      }

      setRewindTarget({
        seq,
        promptText,
        promptBadges,
        affectedTurnsCount: Math.max(1, affected.length),
        editedFiles: Array.from(byPath.values()),
      });
    },
    [],
  );

  const handleRequestFork = useCallback(
    (upToSeq: number | null, worktree: boolean) => {
      if (!selectedHostId || !selectedAgentId) return;
      void orbitActions.forkAgent({
        hostId: selectedHostId,
        sourceAgentId: selectedAgentId,
        upToSeq,
        worktree,
      });
    },
    [selectedHostId, selectedAgentId],
  );

  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  const canForkWorktree = useMemo(
    () => isAgentInGitRepo(selectedHostId, currentAgent, hostDetail, sidebarView),
    [selectedHostId, currentAgent, hostDetail, sidebarView],
  );

  const handleConfirmRewind = useCallback(
    async (restoreFiles: boolean) => {
      if (!rewindTarget || !selectedHostId || !selectedAgentId || rewindSubmitting) return;
      setRewindSubmitting(true);
      try {
        const ok = await orbitActions.rewindAgent(
          selectedHostId,
          selectedAgentId,
          rewindTarget.seq,
          restoreFiles,
          rewindTarget.promptText,
          rewindTarget.promptBadges,
        );
        if (ok) {
          setRewindTarget(null);
        }
      } finally {
        setRewindSubmitting(false);
      }
    },
    [rewindTarget, selectedHostId, selectedAgentId, rewindSubmitting],
  );

  const handleConfirmRewindSync = useCallback(
    (restoreFiles: boolean) => {
      void handleConfirmRewind(restoreFiles);
    },
    [handleConfirmRewind],
  );

  useEffect(() => {
    return () => {
      if (programmaticScrollTimerRef.current !== null) {
        window.clearTimeout(programmaticScrollTimerRef.current);
      }
      if (scrollRafRef.current !== null) {
        window.cancelAnimationFrame(scrollRafRef.current);
      }
      if (bottomSyncRafRef.current !== null) {
        window.cancelAnimationFrame(bottomSyncRafRef.current);
      }
    };
  }, []);

  const triggerLoadEarlierTurns = useCallback(
    (targetTurnIndex?: number, capturePrependAnchor = true) => {
      const hostId = activeHostIdRef.current;
      const agentId = activeAgentIdRef.current;
      const scope = activeSessionScopeRef.current;
      const curWindowStart = windowStartTurnIndexRef.current;
      const canLoadOlder = curWindowStart > 0 || hasOlderHistoryRef.current;
      if (!hostId || !agentId || !scope || !canLoadOlder || loadingEarlierTurnsRef.current) {
        return;
      }
      const curRev = stageRevisionRef.current;
      const exhausted = exhaustedEarlierWindowRef.current;
      if (
        curWindowStart > 0 &&
        targetTurnIndex === undefined &&
        exhausted &&
        exhausted.scope === scope &&
        exhausted.windowStart === curWindowStart &&
        exhausted.revision === curRev
      ) {
        return;
      }

      if (capturePrependAnchor && scrollRef.current) {
        pendingTurnPrependRef.current = {
          scope,
          prevScrollHeight: scrollRef.current.scrollHeight,
          prevScrollTop: scrollRef.current.scrollTop,
        };
      }

      loadingEarlierTurnsRef.current = true;
      setLoadingEarlierTurns(true);
      const reqId = (earlierLoadReqIdRef.current += 1);

      void orbitActions
        .loadEarlierStageTurns(hostId, agentId, targetTurnIndex)
        .then((loadedCount) => {
          if (earlierLoadReqIdRef.current !== reqId) return;
          if (loadedCount === 0) {
            pendingTurnPrependRef.current = null;
            if (targetTurnIndex === undefined && curWindowStart > 0) {
              exhaustedEarlierWindowRef.current = {
                scope,
                windowStart: curWindowStart,
                revision: curRev,
              };
            }
          }
        })
        .catch(() => {
          if (earlierLoadReqIdRef.current !== reqId) return;
          pendingTurnPrependRef.current = null;
          if (targetTurnIndex === undefined && curWindowStart > 0) {
            exhaustedEarlierWindowRef.current = {
              scope,
              windowStart: curWindowStart,
              revision: curRev,
            };
          }
        })
        .finally(() => {
          if (earlierLoadReqIdRef.current === reqId) {
            loadingEarlierTurnsRef.current = false;
            setLoadingEarlierTurns(false);
          }
        });
    },
    [],
  );

  const syncScrollMetrics = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const currentTop = el.scrollTop;
    const prevTop = lastObservedScrollTopRef.current;
    lastObservedScrollTopRef.current = currentTop;

    const distanceFromBottom = Math.max(0, el.scrollHeight - currentTop - el.clientHeight);
    const hasTurns = turnsCountRef.current > 0;

    let nextOccluding = false;
    let nextJump = false;

    if (programmaticScrollRef.current) {
      if (distanceFromBottom < 8) {
        programmaticScrollRef.current = false;
        if (programmaticScrollTimerRef.current !== null) {
          window.clearTimeout(programmaticScrollTimerRef.current);
          programmaticScrollTimerRef.current = null;
        }
      }
      stickToBottomRef.current = true;
    } else {
      const nearBottom = distanceFromBottom < 36;
      const scrolledUp = currentTop < prevTop - 1.5;
      const inEntrySettle = performance.now() < sessionEntryLockUntilRef.current;
      if (inEntrySettle && stickToBottomRef.current) {
        if (distanceFromBottom >= 2) {
          el.scrollTop = el.scrollHeight;
          lastObservedScrollTopRef.current = el.scrollTop;
        }
      } else if (nearBottom) {
        stickToBottomRef.current = true;
      } else if (scrolledUp && !isShellLayoutMovingRef.current) {
        stickToBottomRef.current = false;
      }
      const pinned = stickToBottomRef.current;
      nextOccluding = !pinned && distanceFromBottom > 4 && hasTurns;
      nextJump = !pinned && !nearBottom && hasTurns;
    }

    if (
      loadingEarlierTurnsRef.current &&
      pendingTurnPrependRef.current &&
      pendingTurnPrependRef.current.scope === activeSessionScopeRef.current
    ) {
      pendingTurnPrependRef.current = {
        scope: pendingTurnPrependRef.current.scope,
        prevScrollHeight: el.scrollHeight,
        prevScrollTop: currentTop,
      };
    } else if (
      !stickToBottomRef.current &&
      !programmaticScrollRef.current &&
      currentTop < STAGE_TOP_PAGINATE_THRESHOLD_PX &&
      (windowStartTurnIndexRef.current > 0 || hasOlderHistoryRef.current)
    ) {
      triggerLoadEarlierTurns(undefined, true);
    }

    let nextActiveSeq = scrollUiStore.getSnapshot().activePromptSeq;
    const prompts = outlinePromptsRef.current;
    if (isStageWideRef.current && prompts.length >= 2) {
      const now = performance.now();
      if (now - lastOutlineSyncAtRef.current >= 80) {
        lastOutlineSyncAtRef.current = now;
        const measurements = virtualizerRef.current?.measurementsCache;
        const anchorOffset = Math.max(0, currentTop + 96);
        if (measurements && measurements.length > 0) {
          let lo = 0;
          let hi = prompts.length - 1;
          let best = prompts[0].seq;
          while (lo <= hi) {
            const mid = (lo + hi) >> 1;
            const m = measurements[prompts[mid].turnIndex];
            if (!m || m.start <= anchorOffset) {
              best = prompts[mid].seq;
              lo = mid + 1;
            } else {
              hi = mid - 1;
            }
          }
          nextActiveSeq = best;
        }
      }
    }

    scrollUiStore.update({
      isOccludingText: nextOccluding,
      showJumpLatest: nextJump,
      activePromptSeq: nextActiveSeq,
    });
  }, [scrollUiStore, triggerLoadEarlierTurns]);

  const snapScrollerToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    lastObservedScrollTopRef.current = el.scrollTop;
  }, []);

  const scheduleBottomSync = useCallback(() => {
    if (bottomSyncRafRef.current !== null) return;
    bottomSyncRafRef.current = window.requestAnimationFrame(() => {
      bottomSyncRafRef.current = null;
      if (
        stickToBottomRef.current &&
        !programmaticScrollRef.current &&
        !loadingEarlierTurnsRef.current
      ) {
        snapScrollerToBottom();
      }
      syncScrollMetrics();
    });
  }, [snapScrollerToBottom, syncScrollMetrics]);

  const handleScroll = useCallback(() => {
    if (scrollRafRef.current !== null) return;
    scrollRafRef.current = window.requestAnimationFrame(() => {
      scrollRafRef.current = null;
      syncScrollMetrics();
    });
  }, [syncScrollMetrics]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onPassiveWheel = (e: WheelEvent) => {
      if (e.deltaY < -0.5) {
        sessionEntryLockUntilRef.current = 0;
        stickToBottomRef.current = false;
        if (programmaticScrollRef.current) {
          programmaticScrollRef.current = false;
          if (programmaticScrollTimerRef.current !== null) {
            window.clearTimeout(programmaticScrollTimerRef.current);
            programmaticScrollTimerRef.current = null;
          }
        }
        if (
          el.scrollTop < STAGE_TOP_PAGINATE_THRESHOLD_PX &&
          (windowStartTurnIndexRef.current > 0 || hasOlderHistoryRef.current)
        ) {
          triggerLoadEarlierTurns(undefined, true);
        }
      }
    };
    const onTouchStart = () => {
      sessionEntryLockUntilRef.current = 0;
    };
    const onPointerDown = (e: PointerEvent) => {
      const rect = el.getBoundingClientRect();
      if (e.clientX >= rect.right - 20) {
        sessionEntryLockUntilRef.current = 0;
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "ArrowUp" || e.key === "PageUp" || e.key === "Home") {
        sessionEntryLockUntilRef.current = 0;
        stickToBottomRef.current = false;
      }
    };
    el.addEventListener("wheel", onPassiveWheel, { passive: true });
    el.addEventListener("touchstart", onTouchStart, { passive: true });
    el.addEventListener("pointerdown", onPointerDown, { passive: true });
    el.addEventListener("keydown", onKeyDown);
    return () => {
      el.removeEventListener("wheel", onPassiveWheel);
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("pointerdown", onPointerDown);
      el.removeEventListener("keydown", onKeyDown);
    };
  }, [isDraft, selectedHostId, selectedAgentId, triggerLoadEarlierTurns]);

  useLayoutEffect(() => {
    const contentEl = contentRef.current;
    const scrollerEl = scrollRef.current;
    if (!contentEl || !scrollerEl || typeof ResizeObserver === "undefined") return;

    let lastScrollHeight = scrollerEl.scrollHeight;
    let lastClientHeight = scrollerEl.clientHeight;

    const ro = new ResizeObserver(() => {
      const sh = scrollerEl.scrollHeight;
      const ch = scrollerEl.clientHeight;
      const heightChanged = sh !== lastScrollHeight || ch !== lastClientHeight;
      lastScrollHeight = sh;
      lastClientHeight = ch;

      if (heightChanged) {
        const distanceFromBottom = Math.max(0, sh - scrollerEl.scrollTop - ch);
        const allowSnap =
          !loadingEarlierTurnsRef.current &&
          (performance.now() < sessionEntryLockUntilRef.current || distanceFromBottom <= 120);
        if (stickToBottomRef.current && !programmaticScrollRef.current && allowSnap) {
          snapScrollerToBottom();
        }
        scheduleBottomSync();
      } else {
        handleScroll();
      }
    });
    ro.observe(contentEl);
    ro.observe(scrollerEl);
    return () => ro.disconnect();
  }, [
    isDraft,
    selectedHostId,
    selectedAgentId,
    snapScrollerToBottom,
    scheduleBottomSync,
    handleScroll,
  ]);

  useEffect(() => {
    const shelfEl = composerShelfRef.current;
    if (!shelfEl || typeof ResizeObserver === "undefined") return;
    prevComposerHeightRef.current = shelfEl.getBoundingClientRect().height;

    const ro = new ResizeObserver((entries) => {
      const nextH =
        entries[0]?.borderBoxSize?.[0]?.blockSize ??
        entries[0]?.contentRect.height ??
        shelfEl.getBoundingClientRect().height;
      const prevH = prevComposerHeightRef.current;
      prevComposerHeightRef.current = nextH;

      const deltaH = nextH - prevH;
      if (Math.abs(deltaH) < 0.5) return;

      const scroller = scrollRef.current;
      if (scroller && stickToBottomRef.current) {
        if (deltaH > 0) {
          scroller.scrollTop += deltaH;
        }
        lastObservedScrollTopRef.current = scroller.scrollTop;
      }
      syncScrollMetrics();
    });
    ro.observe(shelfEl);
    return () => ro.disconnect();
  }, [isDraft, selectedAgentId, syncScrollMetrics]);

  const scrollToLatest = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    programmaticScrollRef.current = true;
    stickToBottomRef.current = true;
    scrollUiStore.update({ isOccludingText: false, showJumpLatest: false });

    if (programmaticScrollTimerRef.current !== null) {
      window.clearTimeout(programmaticScrollTimerRef.current);
    }
    programmaticScrollTimerRef.current = window.setTimeout(() => {
      programmaticScrollRef.current = false;
      programmaticScrollTimerRef.current = null;
      syncScrollMetrics();
    }, 800);

    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }, [scrollUiStore, syncScrollMetrics]);

  const holdViewport: ViewportAnchorHoldFn = useCallback(
    (triggerEl, apply, edge = "top") => {
      const scroller = scrollRef.current;
      if (!scroller || !triggerEl) {
        apply();
        return;
      }
      const beforeRect = triggerEl.getBoundingClientRect();
      const beforeAnchor = edge === "bottom" ? beforeRect.bottom : beforeRect.top;
      const wasPinnedBottom = stickToBottomRef.current;
      stickToBottomRef.current = false;
      holdActiveRef.current = true;

      apply();

      requestAnimationFrame(() => {
        const curScroller = scrollRef.current;
        if (!curScroller || !triggerEl.isConnected) {
          holdActiveRef.current = false;
          return;
        }
        const afterRect = triggerEl.getBoundingClientRect();
        const afterAnchor = edge === "bottom" ? afterRect.bottom : afterRect.top;
        const delta = afterAnchor - beforeAnchor;
        if (Math.abs(delta) > 1) {
          curScroller.scrollTop += delta;
        }
        lastObservedScrollTopRef.current = curScroller.scrollTop;

        const parentWrapper = triggerEl.parentElement;
        if (parentWrapper) {
          const wrapperRect = parentWrapper.getBoundingClientRect();
          const scrollerRect = curScroller.getBoundingClientRect();
          const overflowBelow = wrapperRect.bottom - scrollerRect.bottom;
          if (overflowBelow > 8 && wrapperRect.height < scrollerRect.height * 0.75) {
            curScroller.scrollBy({
              top: Math.min(overflowBelow + 12, 220),
              behavior: "smooth",
            });
          }
        }

        if (wasPinnedBottom) {
          const dist =
            curScroller.scrollHeight - curScroller.scrollTop - curScroller.clientHeight;
          stickToBottomRef.current = dist < 48;
        }
        syncScrollMetrics();
        requestAnimationFrame(() => {
          holdActiveRef.current = false;
        });
      });
    },
    [syncScrollMetrics],
  );

  const jumpToPromptSeq = useCallback(
    (seq: number) => {
      const entry = outlinePromptsRef.current.find((p) => p.seq === seq);
      const v = virtualizerRef.current;
      if (!entry || !v) return;
      stickToBottomRef.current = false;
      scrollUiStore.update({ activePromptSeq: seq });
      v.scrollToIndex(entry.turnIndex, { align: "start", behavior: "smooth" });
    },
    [scrollUiStore],
  );

  const hasPendingJumpForActive = Boolean(
    pendingJumpTarget &&
      selectedHostId &&
      selectedAgentId &&
      pendingJumpTarget.hostId === selectedHostId &&
      pendingJumpTarget.agentId === selectedAgentId,
  );
  const hasPendingJumpForActiveRef = useRef(hasPendingJumpForActive);
  hasPendingJumpForActiveRef.current = hasPendingJumpForActive;

  useLayoutEffect(() => {
    earlierLoadReqIdRef.current += 1;
    loadingEarlierTurnsRef.current = false;
    setLoadingEarlierTurns(false);
    pendingTurnPrependRef.current = null;
    exhaustedEarlierWindowRef.current = null;
    jumpEarlierAttemptedNonceRef.current = null;
    prevWindowStartRef.current = {
      scope: activeSessionScope,
      windowStart: windowStartTurnIndexRef.current,
      firstTurnKey: firstTurnKeyRef.current,
      turnCount: turnsLengthRef.current,
    };

    if (!activeSessionScope) {
      prevTurnsCountRef.current = 0;
      return;
    }
    if (programmaticScrollTimerRef.current !== null) {
      window.clearTimeout(programmaticScrollTimerRef.current);
      programmaticScrollTimerRef.current = null;
    }
    if (jumpRafRef.current.raf1 !== null) {
      window.cancelAnimationFrame(jumpRafRef.current.raf1);
      jumpRafRef.current.raf1 = null;
    }
    if (jumpRafRef.current.raf2 !== null) {
      window.cancelAnimationFrame(jumpRafRef.current.raf2);
      jumpRafRef.current.raf2 = null;
    }
    if (jumpFallbackTimerRef.current !== null) {
      window.clearTimeout(jumpFallbackTimerRef.current);
      jumpFallbackTimerRef.current = null;
    }
    setJumpFallbackNonce(null);
    programmaticScrollRef.current = false;
    holdActiveRef.current = false;
    const isTargetedJump = hasPendingJumpForActiveRef.current;
    if (!isTargetedJump) {
      if (highlightTimerRef.current !== null) {
        window.clearTimeout(highlightTimerRef.current);
        highlightTimerRef.current = null;
      }
      setHighlightedTurnKey(null);
    }
    stickToBottomRef.current = !isTargetedJump;
    sessionEntryLockUntilRef.current = isTargetedJump ? 0 : performance.now() + 200;
    lastObservedScrollTopRef.current = 0;
    prevTurnsCountRef.current = turnsCountRef.current;
    scrollUiStore.update({
      isOccludingText: false,
      showJumpLatest: false,
      activePromptSeq: null,
    });
    setRewindTarget(null);

    if (isTargetedJump) {
      return;
    }

    const anchorToBottom = () => {
      if (!stickToBottomRef.current || programmaticScrollRef.current) return;
      if (turnsCountRef.current > 0) {
        virtualizerRef.current?.scrollToIndex(turnsCountRef.current - 1, { align: "end" });
      }
      snapScrollerToBottom();
    };

    anchorToBottom();
    scheduleBottomSync();

    let raf2: number | null = null;
    const raf1 = window.requestAnimationFrame(() => {
      anchorToBottom();
      raf2 = window.requestAnimationFrame(() => {
        anchorToBottom();
        syncScrollMetrics();
      });
    });
    return () => {
      window.cancelAnimationFrame(raf1);
      if (raf2 !== null) window.cancelAnimationFrame(raf2);
    };
  }, [
    activeSessionScope,
    sessionScrollNonce,
    scrollUiStore,
    snapScrollerToBottom,
    scheduleBottomSync,
    syncScrollMetrics,
  ]);

  // Zero-jump scroll anchoring right after prepending earlier historical turns above the viewport
  useLayoutEffect(() => {
    const prev = prevWindowStartRef.current;
    prevWindowStartRef.current = {
      scope: activeSessionScope,
      windowStart: windowStartTurnIndex,
      firstTurnKey,
      turnCount: stageTurns.length,
    };
    const wirePrepended =
      firstTurnKey !== null &&
      prev.firstTurnKey !== null &&
      firstTurnKey !== prev.firstTurnKey &&
      stageTurns.length > prev.turnCount;
    if (
      !activeSessionScope ||
      prev.scope !== activeSessionScope ||
      (windowStartTurnIndex >= prev.windowStart && !wirePrepended)
    ) {
      return;
    }
    const snap = pendingTurnPrependRef.current;
    pendingTurnPrependRef.current = null;
    const el = scrollRef.current;
    if (!snap || snap.scope !== activeSessionScope || !el) return;
    if (hasPendingJumpForActiveRef.current) return;

    const deltaHeight = el.scrollHeight - snap.prevScrollHeight;
    if (deltaHeight > 0) {
      holdActiveRef.current = true;
      el.scrollTop = computePrependRestoredScrollTop(
        snap.prevScrollTop,
        snap.prevScrollHeight,
        el.scrollHeight,
      );
      lastObservedScrollTopRef.current = el.scrollTop;
      const raf = window.requestAnimationFrame(() => {
        holdActiveRef.current = false;
        syncScrollMetrics();
      });
      return () => {
        window.cancelAnimationFrame(raf);
        holdActiveRef.current = false;
      };
    }
  }, [activeSessionScope, windowStartTurnIndex, firstTurnKey, stageTurns.length, syncScrollMetrics]);

  useEffect(() => {
    if (!activeSessionScope || !pendingJumpTarget) return;
    if (
      pendingJumpTarget.hostId !== selectedHostId ||
      pendingJumpTarget.agentId !== selectedAgentId
    ) {
      return;
    }
    if (stageTurns.length === 0) return;

    const firstLoadedAnchorSeq = stageTurns[0]?.anchor_seq ?? 0;
    if ((windowStartTurnIndex > 0 || hasOlderHistory) && pendingJumpTarget.seq < firstLoadedAnchorSeq) {
      if (loadingEarlierTurns) {
        return;
      }
      if (jumpEarlierAttemptedNonceRef.current !== pendingJumpTarget.nonce) {
        stickToBottomRef.current = false;
        sessionEntryLockUntilRef.current = 0;
        jumpEarlierAttemptedNonceRef.current = pendingJumpTarget.nonce;
        triggerLoadEarlierTurns(0, false);
        return;
      }
    }

    const match = findTurnIndexForSeq(stageTurns, pendingJumpTarget.seq);
    if (match.index < 0) return;
    const jumpNonce = pendingJumpTarget.nonce;
    const stageLatestSeq = stageView?.latest_seq ?? 0;

    if (!match.exact && stageLatestSeq < pendingJumpTarget.seq && jumpFallbackNonce !== jumpNonce) {
      stickToBottomRef.current = false;
      sessionEntryLockUntilRef.current = 0;
      if (jumpFallbackTimerRef.current === null) {
        jumpFallbackTimerRef.current = window.setTimeout(() => {
          jumpFallbackTimerRef.current = null;
          setJumpFallbackNonce(jumpNonce);
        }, 1500);
      }
      return;
    }

    if (jumpFallbackTimerRef.current !== null) {
      window.clearTimeout(jumpFallbackTimerRef.current);
      jumpFallbackTimerRef.current = null;
    }

    const turnIdx = match.index;
    const targetTurn = stageTurns[turnIdx];

    stickToBottomRef.current = false;
    sessionEntryLockUntilRef.current = 0;
    programmaticScrollRef.current = true;
    if (programmaticScrollTimerRef.current !== null) {
      window.clearTimeout(programmaticScrollTimerRef.current);
    }
    programmaticScrollTimerRef.current = window.setTimeout(() => {
      programmaticScrollRef.current = false;
      programmaticScrollTimerRef.current = null;
      syncScrollMetrics();
    }, 250);

    const performJump = () => {
      virtualizerRef.current?.scrollToIndex(turnIdx, {
        align: "center",
        behavior: "auto",
      });
    };

    if (jumpRafRef.current.raf1 !== null) {
      window.cancelAnimationFrame(jumpRafRef.current.raf1);
      jumpRafRef.current.raf1 = null;
    }
    if (jumpRafRef.current.raf2 !== null) {
      window.cancelAnimationFrame(jumpRafRef.current.raf2);
      jumpRafRef.current.raf2 = null;
    }

    performJump();
    jumpRafRef.current.raf1 = window.requestAnimationFrame(() => {
      jumpRafRef.current.raf1 = null;
      performJump();
      jumpRafRef.current.raf2 = window.requestAnimationFrame(() => {
        jumpRafRef.current.raf2 = null;
        performJump();
        syncScrollMetrics();
      });
    });

    setHighlightedTurnKey(targetTurn.key);
    if (highlightTimerRef.current !== null) {
      window.clearTimeout(highlightTimerRef.current);
    }
    highlightTimerRef.current = window.setTimeout(() => {
      setHighlightedTurnKey((cur) => (cur === targetTurn.key ? null : cur));
      highlightTimerRef.current = null;
    }, 1800);

    orbitActions.clearPendingJumpTarget(jumpNonce);
  }, [
    activeSessionScope,
    selectedHostId,
    selectedAgentId,
    pendingJumpTarget,
    stageTurns,
    windowStartTurnIndex,
    hasOlderHistory,
    loadingEarlierTurns,
    stageView?.latest_seq,
    jumpFallbackNonce,
    syncScrollMetrics,
    triggerLoadEarlierTurns,
  ]);

  useEffect(() => {
    return () => {
      if (highlightTimerRef.current !== null) {
        window.clearTimeout(highlightTimerRef.current);
      }
      if (jumpFallbackTimerRef.current !== null) {
        window.clearTimeout(jumpFallbackTimerRef.current);
      }
      if (jumpRafRef.current.raf1 !== null) {
        window.cancelAnimationFrame(jumpRafRef.current.raf1);
      }
      if (jumpRafRef.current.raf2 !== null) {
        window.cancelAnimationFrame(jumpRafRef.current.raf2);
      }
    };
  }, []);

  useLayoutEffect(() => {
    if (!activeSessionScope) return;
    const prevCount = prevTurnsCountRef.current;
    prevTurnsCountRef.current = stageTurns.length;

    if (prevCount === 0 && stageTurns.length > 0 && stickToBottomRef.current) {
      sessionEntryLockUntilRef.current = performance.now() + 600;
      virtualizerRef.current?.scrollToIndex(stageTurns.length - 1, { align: "end" });
      snapScrollerToBottom();
      const raf = window.requestAnimationFrame(() => {
        if (stickToBottomRef.current && !programmaticScrollRef.current && turnsCountRef.current > 0) {
          virtualizerRef.current?.scrollToIndex(turnsCountRef.current - 1, { align: "end" });
          snapScrollerToBottom();
          syncScrollMetrics();
        }
      });
      return () => window.cancelAnimationFrame(raf);
    }

    if (stickToBottomRef.current && !programmaticScrollRef.current) {
      snapScrollerToBottom();
    }
    scheduleBottomSync();
  }, [
    activeSessionScope,
    stageView?.latest_seq,
    stageTurns.length,
    stageMaxContentWidthPx,
    isAnimating,
    snapScrollerToBottom,
    scheduleBottomSync,
    syncScrollMetrics,
  ]);

  if (!selectedHostId) {
    if (!fleetHydrated) {
      return <StageHydratingFallback bootstrapError={bootstrapError} />;
    }
    return (
      <div className="text-muted-foreground flex flex-1 items-center justify-center text-[13.5px]">
        Pair or select an Orbit host to begin.
      </div>
    );
  }

  if (isDraft || !selectedAgentId) {
    mountedBaselineRef.current = { agentId: null, maxSeq: 0 };
    return (
      <ScrollArea
        className="min-h-0 flex-1"
        viewportClassName="[&>div]:h-full"
        style={
          stageMaxContentWidthPx
            ? ({ "--orbit-stage-max-w": `${stageMaxContentWidthPx}px` } as CSSProperties)
            : undefined
        }
      >
        <div className="h-full w-full" style={stagePushStyle}>
          <NewSessionLaunchpad />
        </div>
      </ScrollArea>
    );
  }

  if (mountedBaselineRef.current.agentId !== selectedAgentId) {
    const isFreshFirstTurn = stageTurns.length === 1 && isLatestTurnActive;
    mountedBaselineRef.current = {
      agentId: selectedAgentId,
      maxSeq: isFreshFirstTurn
        ? 0
        : (stageView?.latest_seq ?? Number.MAX_SAFE_INTEGER),
    };
  } else if (
    stageView &&
    stageView.latest_seq < mountedBaselineRef.current.maxSeq
  ) {
    mountedBaselineRef.current = {
      agentId: selectedAgentId,
      maxSeq: stageView.latest_seq,
    };
  }
  const baselineMaxSeq = mountedBaselineRef.current.maxSeq;

  const effectiveCwd = selectedCwd || currentAgent?.cwd || "";

  return (
    <div
      ref={stageContainerRef}
      style={
        stageMaxContentWidthPx
          ? ({ "--orbit-stage-max-w": `${stageMaxContentWidthPx}px` } as CSSProperties)
          : undefined
      }
      className="relative flex min-h-0 flex-1 flex-col overflow-hidden"
    >
      {/* Peripheral Megathread Outline Rail (clean left gutter >= 800px / < 776px) */}
      {outlinePrompts.length >= 2 && (
        <StageOutlineRailBridge
          visible={isStageWide}
          offsetPx={outlineOffsetPx}
          offsetTransition={outlineTransition}
          isAnimating={isAnimating}
          scrollUiStore={scrollUiStore}
          prompts={outlinePrompts}
          onJumpToSeq={jumpToPromptSeq}
        />
      )}

      {/* Parent Back Banner when inspecting a managed subagent */}
      {selectedHostId && currentAgent?.parent_agent_id && (
        <div
          style={{
            paddingRight: `${24 + overviewReservePx}px`,
            transition: overviewReserveTransition,
          }}
          className="shrink-0 pl-[24px]"
        >
          <div style={stagePushStyle}>
            <ParentSubagentBanner hostId={selectedHostId} agent={currentAgent} />
          </div>
        </div>
      )}

      {/* Subtle 1-line provenance chip when viewing a forked conversation */}
      {selectedHostId && currentAgent?.forked_from_agent_id && (
        <div
          style={{
            paddingRight: `${24 + overviewReservePx}px`,
            transition: overviewReserveTransition,
          }}
          className="shrink-0 pl-[24px]"
        >
          <div style={stagePushStyle}>
            <ForkedFromBanner hostId={selectedHostId} agent={currentAgent} />
          </div>
        </div>
      )}

      {/* 1. Unified Scrollable Conversation Viewport + Floating Overlays */}
      <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
        {loadingEarlierTurns && (
          <div
            style={{
              paddingRight: `${overviewReservePx}px`,
              transition: overviewReserveTransition,
            }}
            className="pointer-events-none absolute top-2.5 right-0 left-0 z-10 flex justify-center"
          >
            <div
              style={stagePushStyle}
              className="border-border/70 bg-elevated/90 text-muted-foreground inline-flex h-[24px] items-center justify-center rounded-full border px-2.5 shadow-xs backdrop-blur-sm"
            >
              <OrbitSpinner size={12} className="text-foreground/70" />
            </div>
          </div>
        )}

        <ScrollArea
          className="min-h-0 flex-1"
          viewportRef={scrollRef}
          onViewportScroll={handleScroll}
          viewportStyle={{
            paddingRight: `${24 + overviewReservePx}px`,
            transition: overviewReserveTransition,
          }}
          onViewportTransitionEnd={onStageTransitionEnd}
          viewportClassName="pt-6 pb-[56px] pl-[24px]"
          contentClassName={stageTurns.length === 0 ? "flex min-h-full flex-col" : undefined}
        >
          <div
            key={selectedAgentId}
            ref={contentRef}
            onTransitionEnd={onStageTransitionEnd}
            style={stagePushStyle}
            className={cn(
              "mx-auto w-full min-w-0 max-w-[var(--orbit-stage-max-w,740px)]",
              stageTurns.length === 0 && "flex flex-1 flex-col",
            )}
          >
            {stageTurns.length === 0 &&
              wasSessionRewound &&
              stageView !== null &&
              (stageView.agent?.latest_seq ?? 0) === 0 &&
              !isAgentRunning && (() => {
                const cleanDir = (effectiveCwd || "").replace(/\/+$/, "");
                const folderLabel =
                  cleanDir && cleanDir !== "~" ? cleanDir.split("/").pop() || cleanDir : null;
                const wsBranch =
                  currentAgent && hostDetail
                    ? (hostDetail.workspaces.find(
                        (w) => w.workspace_id === currentAgent.workspace_id,
                      )?.git_branch ?? null)
                    : null;
                return (
                  <div className="flex flex-1 flex-col items-center justify-center pb-12 text-center select-none">
                    <div className="bg-foreground/[0.035] text-muted-foreground/70 mb-3 flex size-9 items-center justify-center rounded-full ring-1 ring-inset ring-black/[0.06] dark:bg-white/[0.045] dark:ring-white/[0.08]">
                      <RotateCcw className="size-4 stroke-[1.7]" />
                    </div>
                    <p className="text-foreground/85 text-[14px] font-medium tracking-[-0.012em]">
                      Back to the start
                    </p>
                    {(folderLabel || wsBranch) && (
                      <div className="text-muted-foreground/70 mt-1 flex items-center justify-center gap-1.5 text-[12px]">
                        {folderLabel && <span className="max-w-[220px] truncate">{folderLabel}</span>}
                        {folderLabel && wsBranch && <span className="opacity-45">·</span>}
                        {wsBranch && (
                          <span className="inline-flex max-w-[180px] items-center gap-1 truncate font-mono text-[11.5px]">
                            <GitBranch className="size-3 shrink-0 opacity-75" />
                            <span className="truncate">{wsBranch}</span>
                          </span>
                        )}
                      </div>
                    )}
                  </div>
                );
              })()}

            {stageTurns.length > 0 && (
              <>
                <VirtualizedTurnList
                  key={activeSessionScope ?? selectedAgentId}
                  selectedHostId={selectedHostId}
                  selectedAgentId={selectedAgentId}
                  stageTurns={stageTurns}
                  highlightedTurnKey={highlightedTurnKey}
                  hasPendingJump={hasPendingJumpForActive}
                  isLatestTurnActive={isLatestTurnActive}
                  agentStatusLabel={isLatestTurnActive ? (currentAgent?.status_label ?? null) : null}
                  hasLiveBackgroundTasks={stageInsights.background_tasks.length > 0}
                  baselineMaxSeq={baselineMaxSeq}
                  isNetworkOffline={isNetworkOffline}
                  autoFoldCompleted={coalesceExploration}
                  effectiveCwd={effectiveCwd}
                  stageMaxContentWidthPx={stageMaxContentWidthPx}
                  hostArtifacts={stableHostArtifacts}
                  timingTick={timingTick}
                  scrollRef={scrollRef}
                  stickToBottomRef={stickToBottomRef}
                  holdActiveRef={holdActiveRef}
                  virtualizerRef={virtualizerRef}
                  turnTimingsRef={turnTimingsRef}
                  thinkingTimingsRef={thinkingTimingsRef}
                  getThinkingDurationMs={getThinkingDurationMs}
                  holdViewport={holdViewport}
                  onRequestRewind={handleRequestRewind}
                  onRequestFork={handleRequestFork}
                  canForkWorktree={canForkWorktree}
                  onPreviewImage={setImagePreviewTarget}
                  sessionSubagents={sessionSubagents}
                  sessionSubagentMatcher={sessionSubagentMatcher}
                  onOpenSubagent={handleOpenSubagent}
                  queuedNotificationCount={queuedNotificationCount}
                  onInterruptAndContinue={handleInterruptAndContinue}
                />

                {/* Dynamic Reading Runway: gives active streaming output a calm centered eye-zone cushion above Composer */}
                <div
                  aria-hidden="true"
                  className={cn(
                    "w-full shrink-0 transition-[height] duration-300 ease-[var(--ease-out-expo)]",
                    isLatestTurnActive ? "h-[clamp(120px,26vh,220px)]" : "h-0",
                  )}
                />
              </>
            )}
          </div>
        </ScrollArea>

        <StageBottomViewportOverlay
          scrollUiStore={scrollUiStore}
          stagePushStyle={stagePushStyle}
          overviewReservePx={overviewReservePx}
          overviewReserveTransition={overviewReserveTransition}
          onJumpLatest={scrollToLatest}
        />
      </div>

      {/* 2. Docked Composer Shelf — Pure Input & Unified Decision Bay (no Subagent/Goal pileup!) */}
      <div ref={composerShelfRef} className="bg-surface-base relative z-20 shrink-0">
        <div
          style={{
            paddingRight: `${overviewReservePx}px`,
            transition: overviewReserveTransition,
          }}
        >
          <div style={stagePushStyle}>
            <DecisionDock
              hostId={selectedHostId}
              agentId={selectedAgentId}
              cwd={effectiveCwd}
              permissions={pendingPermissions}
              questions={pendingQuestions}
              subagentPermissions={subagentPermissions}
            />
            <StageDockedComposer scrollUiStore={scrollUiStore} />
          </div>
        </div>
      </div>

      {/* 3. Top-Right Viewport-Anchored Overview Card Overlay (to the left of the far-right scrollbar) */}
      <StageOverviewPanel
        canDock={canDockOverview}
        hostId={selectedHostId}
        agentId={selectedAgentId}
        cwd={effectiveCwd}
        insights={stageInsights}
        sessionTurnDiffs={sessionTurnDiffs}
        currentAgent={currentAgent}
        stageContainerRef={stageContainerRef}
      />

      <RewindTurnDialog
        target={rewindTarget}
        isAgentRunning={isAgentRunning}
        submitting={rewindSubmitting}
        onOpenChange={handleRewindOpenChange}
        onConfirm={handleConfirmRewindSync}
      />

      <PromptImageLightbox
        target={imagePreviewTarget}
        onClose={handleCloseImagePreview}
      />
    </div>
  );
}
