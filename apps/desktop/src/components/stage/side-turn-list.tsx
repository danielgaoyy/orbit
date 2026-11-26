import { memo, useCallback, useLayoutEffect, useMemo, useRef, type RefObject } from "react";
import { observeElementOffset, useVirtualizer, type Virtualizer } from "@tanstack/react-virtual";
import type { ArtifactSummary, StageTurnView } from "@/lib/orbit-ipc";
import type { ResolvedSubagentItem, SubagentNameMatcher } from "@/lib/subagents";
import type { resolveDelegationDispatches } from "@/lib/subagents";
import { resolveTypography } from "@/lib/typography";
import { useOrbitSelector } from "@/state/orbit-store";
import {
  EMPTY_TURN_TIMINGS,
  type PromptImagePreviewTarget,
  type StageTimingHistory,
  type ThinkingDurationLookupFn,
  type TurnListRow,
  type TurnTimingRecord,
  type ViewportAnchorHoldFn,
  buildTurnHeightCachePrefix,
  createTurnProseMatcherLookup,
  getCachedHistoricalThinkingSignature,
  getCachedTurnHeight,
  getTurnItemHeight,
  getTurnListRows,
  setCachedTurnHeight,
  turnHasSubagentReferences,
} from "./stage-helpers";
import { TurnStageSection } from "./stage-turn-card";

const NOOP_REWIND = () => {};
const NOOP_THINKING_DURATION: ThinkingDurationLookupFn = () => null;
const EMPTY_TIMING_HISTORY: StageTimingHistory = new Map();

export interface SidePanelTurnListProps {
  scopeKey: string;
  hostId: string;
  agentId: string;
  cwd: string;
  turns: StageTurnView[];
  isRunning: boolean;
  liveStatusLabel: string | null;
  hasLiveBackgroundTasks: boolean;
  isNetworkOffline: boolean;
  autoFoldCompleted: boolean;
  hostArtifacts: readonly ArtifactSummary[];
  scrollRef: RefObject<HTMLDivElement | null>;
  stickToBottomRef: RefObject<boolean>;
  holdViewport: ViewportAnchorHoldFn;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
  dispatches: ReturnType<typeof resolveDelegationDispatches>;
  sessionSubagents: readonly ResolvedSubagentItem[];
  sessionSubagentMatcher: SubagentNameMatcher | null;
  onOpenSubagent: (subagent: ResolvedSubagentItem) => void;
  parentMessageLabel?: string | null;
  thinkingTimings?: Record<string, TurnTimingRecord>;
  thinkingTimingsVersion?: number;
  getThinkingDurationMs?: ThinkingDurationLookupFn;
}

/**
 * Shared virtualized turn list for secondary conversation viewports (`SubagentTab` & `AgentStagePane`):
 * - Virtualizes long multi-turn subagent/peer transcripts via `@tanstack/react-virtual` so opening
 *   deep side-panel threads never mounts thousands of offscreen DOM nodes.
 * - Gates `dispatches`, `sessionSubagents`, `sessionSubagentMatcher`, and `hostArtifacts` per turn
 *   matching `StreamStage`'s `VirtualizedTurnList` so historical `TurnStageSection` memos never
 *   break on live streaming deltas or sibling subagent/host updates.
 */
export const SidePanelTurnList = memo(function SidePanelTurnList({
  scopeKey,
  hostId,
  agentId,
  cwd,
  turns,
  isRunning,
  liveStatusLabel,
  hasLiveBackgroundTasks,
  isNetworkOffline,
  autoFoldCompleted,
  hostArtifacts,
  scrollRef,
  stickToBottomRef,
  holdViewport,
  onPreviewImage,
  dispatches,
  sessionSubagents,
  sessionSubagentMatcher,
  onOpenSubagent,
  parentMessageLabel,
  thinkingTimings,
  thinkingTimingsVersion = 0,
  getThinkingDurationMs = NOOP_THINKING_DURATION,
}: SidePanelTurnListProps) {
  const turnsRef = useRef(turns);
  turnsRef.current = turns;

  const rowsRef = useRef<readonly TurnListRow[]>([]);
  const rows = useMemo(() => {
    const next = getTurnListRows(turns, scopeKey, EMPTY_TIMING_HISTORY, isRunning, rowsRef.current);
    rowsRef.current = next;
    return next;
  }, [turns, scopeKey, isRunning]);

  const proseMatcherForTurn = useMemo(
    () => createTurnProseMatcherLookup(sessionSubagentMatcher),
    [sessionSubagentMatcher],
  );

  const conversationFontSize = useOrbitSelector((s) => s.conversationFontSize);
  const codeFontSize = useOrbitSelector((s) => s.codeFontSize);
  const uiFontId = useOrbitSelector((s) => s.uiFontId);
  const contentFontId = useOrbitSelector((s) => s.contentFontId);
  const codeFontId = useOrbitSelector((s) => s.codeFontId);
  const uiFontCustom = useOrbitSelector((s) => s.uiFontCustom);
  const contentFontCustom = useOrbitSelector((s) => s.contentFontCustom);
  const codeFontCustom = useOrbitSelector((s) => s.codeFontCustom);
  const { typographyKey, readScale, codeLinePx } = useMemo(
    () =>
      resolveTypography({
        conversationFontSize,
        codeFontSize,
        uiFontId,
        contentFontId,
        codeFontId,
        uiFontCustom,
        contentFontCustom,
        codeFontCustom,
      }),
    [
      conversationFontSize,
      codeFontSize,
      uiFontId,
      contentFontId,
      codeFontId,
      uiFontCustom,
      contentFontCustom,
      codeFontCustom,
    ],
  );
  const turnSizeMetrics = useMemo(() => ({ readScale, codeLinePx }), [readScale, codeLinePx]);

  const heightCachePrefix = buildTurnHeightCachePrefix("side", scopeKey, "panel", typographyKey);
  const getItemKey = useCallback((index: number) => rows[index].key, [rows]);
  const estimateSize = useCallback(
    (index: number) => {
      const row = rows[index];
      const cached = getCachedTurnHeight(heightCachePrefix + row.key);
      return getTurnItemHeight(turnsRef.current[index], row, cached, turnSizeMetrics);
    },
    [rows, heightCachePrefix, turnSizeMetrics],
  );

  const initialRect = useMemo(() => {
    const el = scrollRef.current;
    return {
      width: el && el.clientWidth > 0 ? el.clientWidth : 480,
      height: el && el.clientHeight > 0 ? el.clientHeight : 720,
    };
    // Compute initial estimate only when mounting a fresh scope
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopeKey]);

  const initialBottomOffset = useMemo(() => {
    if (turns.length === 0) return 0;
    let total = 0;
    for (let i = 0; i < turns.length; i += 1) {
      total += estimateSize(i);
    }
    return Math.max(0, total - initialRect.height);
    // Compute initial bottom anchor only when mounting a fresh scope
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scopeKey, initialRect.height]);

  const observeOffsetWithBottomSeed = useCallback(
    (
      instance: Virtualizer<HTMLDivElement, HTMLDivElement>,
      cb: (offset: number, isScrolling: boolean) => void,
    ) => {
      const el = instance.scrollElement;
      if (
        el &&
        stickToBottomRef.current &&
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

  const measuredSpacing = useRef(new Map<string, number>());
  const measureTurn = useCallback(
    (
      el: HTMLDivElement,
      entry: ResizeObserverEntry | undefined,
      instance: Virtualizer<HTMLDivElement, HTMLDivElement>,
    ) => {
      const row = rows[instance.indexFromElement(el)];
      if (!row) return 0;
      const oldSpacing = measuredSpacing.current.get(row.key);
      const cached =
        !entry && oldSpacing === row.spacingPx ? instance.itemSizeCache.get(row.key) : undefined;
      const size = entry?.borderBoxSize?.[0]?.blockSize ?? cached ?? el.getBoundingClientRect().height;
      measuredSpacing.current.set(row.key, row.spacingPx);
      if (size > 0 || el.getClientRects().length > 0) {
        setCachedTurnHeight(heightCachePrefix + row.key, size - row.spacingPx);
      }
      return size;
    },
    [heightCachePrefix, rows],
  );

  const virtualizer = useVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: turns.length,
    getScrollElement: () => scrollRef.current,
    getItemKey,
    estimateSize,
    measureElement: measureTurn,
    initialRect,
    initialOffset: initialBottomOffset,
    observeElementOffset: observeOffsetWithBottomSeed,
    gap: 0,
    overscan: 4,
    useScrollendEvent: true,
    directDomUpdates: true,
  });

  useLayoutEffect(() => {
    rows.forEach((row, index) => {
      const oldSpacing = measuredSpacing.current.get(row.key);
      if (oldSpacing === undefined || oldSpacing === row.spacingPx) return;
      measuredSpacing.current.set(row.key, row.spacingPx);
      const size = virtualizer.itemSizeCache.get(row.key);
      if (size !== undefined) {
        virtualizer.resizeItem(
          index,
          getTurnItemHeight(turnsRef.current[index], row, size - oldSpacing, turnSizeMetrics),
        );
      }
    });
  }, [rows, virtualizer, turnSizeMetrics]);

  const prevTypographyKeyRef = useRef(typographyKey);
  useLayoutEffect(() => {
    if (prevTypographyKeyRef.current === typographyKey) return;
    prevTypographyKeyRef.current = typographyKey;
    const el = scrollRef.current;
    const wasStuck = stickToBottomRef.current;
    let anchor: { index: number; offsetInItem: number } | null = null;
    if (el && !wasStuck) {
      const scrollTop = el.scrollTop;
      const vItems = virtualizer.getVirtualItems();
      const firstVisible = vItems.find((item) => item.end > scrollTop) ?? vItems[0];
      if (firstVisible) {
        anchor = {
          index: firstVisible.index,
          offsetInItem: scrollTop - firstVisible.start,
        };
      }
    }
    virtualizer.measure();
    const rafId = window.requestAnimationFrame(() => {
      const currentEl = scrollRef.current;
      if (!currentEl) return;
      if (wasStuck) {
        currentEl.scrollTop = Math.max(0, currentEl.scrollHeight - currentEl.clientHeight);
        return;
      }
      if (anchor) {
        virtualizer.scrollToIndex(anchor.index, { align: "start" });
        if (Math.abs(anchor.offsetInItem) > 0.5) {
          const nextTop = currentEl.scrollTop + anchor.offsetInItem;
          const maxTop = Math.max(0, currentEl.scrollHeight - currentEl.clientHeight);
          currentEl.scrollTop = Math.max(0, Math.min(maxTop, nextTop));
        }
      }
    });
    return () => window.cancelAnimationFrame(rafId);
  }, [typographyKey, virtualizer, scrollRef, stickToBottomRef]);

  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = (item, _delta, instance) => {
    const scrollOffset = instance.scrollOffset ?? 0;
    if (stickToBottomRef.current) {
      return item.end <= scrollOffset;
    }
    const key = String(item.key);
    const alreadyMeasured = instance.itemSizeCache.has(key);
    return alreadyMeasured ? item.end <= scrollOffset : item.start < scrollOffset;
  };

  const virtualItems = virtualizer.getVirtualItems();
  const timingsRecord = thinkingTimings ?? EMPTY_TURN_TIMINGS.thinking;

  return (
    <div ref={virtualizer.containerRef} className="relative w-full min-w-0">
      {virtualItems.map((vItem) => {
        const turn = turns[vItem.index];
        if (!turn) return null;
        const isLastTurn = vItem.index === turns.length - 1;
        const isTurnActive = isLastTurn && isRunning;
        const needsSubagents = isTurnActive || turnHasSubagentReferences(turn);
        const thinkingSignature = thinkingTimings
          ? getCachedHistoricalThinkingSignature(
              turn,
              agentId,
              timingsRecord,
              thinkingTimingsVersion,
              isTurnActive,
            )
          : "";

        return (
          <div
            key={vItem.key}
            data-index={vItem.index}
            ref={virtualizer.measureElement}
            style={{ paddingTop: rows[vItem.index].topGapPx }}
            className="absolute top-0 left-0 w-full min-w-0"
          >
            <TurnStageSection
              turn={turn}
              dispatches={needsSubagents ? dispatches : undefined}
              isTurnActive={isTurnActive}
              isLastTurn={isLastTurn}
              agentStatusLabel={isTurnActive ? liveStatusLabel : null}
              hasLiveBackgroundTasks={isTurnActive && hasLiveBackgroundTasks}
              isNetworkOffline={isNetworkOffline}
              autoFoldCompleted={autoFoldCompleted}
              hostId={hostId}
              cwd={cwd}
              agentId={agentId}
              hostArtifacts={hostArtifacts}
              thinkingSignature={thinkingSignature}
              getThinkingDurationMs={getThinkingDurationMs}
              holdViewport={holdViewport}
              onRequestRewind={NOOP_REWIND}
              onPreviewImage={onPreviewImage}
              sessionSubagents={needsSubagents ? sessionSubagents : undefined}
              sessionSubagentMatcher={needsSubagents ? sessionSubagentMatcher : proseMatcherForTurn(turn)}
              onOpenSubagent={onOpenSubagent}
              parentMessageLabel={parentMessageLabel}
            />
          </div>
        );
      })}
    </div>
  );
});
