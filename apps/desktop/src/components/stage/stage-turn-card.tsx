import { memo, useCallback, useMemo, useRef } from "react";
import { AlertTriangle, ChevronRight } from "lucide-react";
import { MarkdownBlocksView, MarkdownLinkProvider } from "@/components/markdown-blocks";
import { AgentMessageCard } from "@/components/stage/subagents-track";
import { ContextBanner } from "@/components/ui/context-banner";
import { DiffStat } from "@/components/ui/diff-stat";
import { useLiveSecondClock } from "@/lib/live-clock";
import type {
  ArtifactSummary,
  PromptBadge,
  StageTurnView,
  TimelineItemView,
} from "@/lib/orbit-ipc";
import {
  type ResolvedSubagentItem,
  type SubagentNameMatcher,
  resolveDelegationDispatches,
} from "@/lib/subagents";
import { useI18n } from "@/lib/i18n";
import { formatDurationCompact } from "@/lib/timeline-coalesce";
import {
  coalescePeerFinishes,
  computeWorkChunkDurationMs,
  splitStepGroupChronologically,
  summarizeWorkChunk,
} from "@/lib/turn-delegation-split";
import { useOrbitSelector } from "@/state/orbit-store";
import {
  EMPTY_ARTIFACTS,
  EMPTY_SUBAGENTS,
  EMPTY_TIMELINE_ITEMS,
  EMPTY_WORK_COUNTS,
  type PromptImagePreviewTarget,
  type ThinkingDurationLookupFn,
  type TurnTimingRecord,
  type ViewportAnchorHoldFn,
  computeTotalThinkingDurationMs,
  extractUserPromptPresentation,
  getTurnFooter,
  isCompactionMarkerTurn,
  isHiddenWakeupLabel,
  isWaitingWakeupLabel,
  parseArtifactRefsFromMessage,
  parseArtifactRefsFromTurnTools,
} from "./stage-helpers";
import {
  TurnChangesBar,
  TurnFooter,
  UserPromptBubble,
} from "./stage-prompt-controls";
import {
  CompactionMilestoneRow,
  DelegationMilestoneRow,
  InlineArtifactCard,
  TurnActivityDisclosure,
  WakeupEventRow,
} from "./stage-step-group";

const StageNoticeSegment = memo(function StageNoticeSegment({
  item,
}: {
  item: Extract<TimelineItemView, { item_type: "notice" }>;
}) {
  const tone =
    item.level === "error" ? "danger" : item.level === "warning" ? "warning" : "neutral";
  return (
    <ContextBanner
      variant="inline"
      tone={tone}
      icon={<AlertTriangle className="size-3.5 shrink-0" />}
      className="items-start py-2 text-[13px]"
    >
      <span className="selectable-text min-w-0 whitespace-pre-wrap break-words">
        {item.message}
      </span>
    </ContextBanner>
  );
});

const StageAssistantMessageSegment = memo(function StageAssistantMessageSegment({
  item,
  hostId,
  cwd,
  hostArtifacts,
  sessionSubagents,
  sessionSubagentMatcher,
  onOpenSubagent,
}: {
  item: Extract<TimelineItemView, { item_type: "assistant_message" }>;
  hostId: string;
  cwd: string;
  hostArtifacts: readonly ArtifactSummary[];
  sessionSubagents?: readonly ResolvedSubagentItem[];
  sessionSubagentMatcher?: SubagentNameMatcher | null;
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
}) {
  const { locale } = useI18n();
  const msgArtifacts = useMemo(
    () => (!item.streaming ? parseArtifactRefsFromMessage(item.text, hostArtifacts, locale) : []),
    [item.streaming, item.text, hostArtifacts, locale],
  );

  return (
    <div className="space-y-2">
      <MarkdownLinkProvider
        hostId={hostId}
        cwd={cwd}
        subagents={sessionSubagents}
        subagentMatcher={sessionSubagentMatcher}
        onOpenSubagent={onOpenSubagent}
      >
        <MarkdownBlocksView blocks={item.blocks} streaming={item.streaming} />
      </MarkdownLinkProvider>
      {msgArtifacts.length > 0 && (
        <div className="flex flex-wrap items-center gap-2 pt-0.5">
          {msgArtifacts.map((ref) => (
            <InlineArtifactCard key={ref.key} hostId={hostId} artifact={ref} />
          ))}
        </div>
      )}
    </div>
  );
});

export const STREAMING_TAIL_SILENCE_THRESHOLD_MS = 10_000;

export function shouldShowStreamingTailActivity(
  nowMs: number,
  lastActivityMs: number | null | undefined,
  thresholdMs = STREAMING_TAIL_SILENCE_THRESHOLD_MS,
): boolean {
  if (typeof lastActivityMs !== "number" || !Number.isFinite(lastActivityMs) || lastActivityMs <= 0) {
    return false;
  }
  return Math.max(0, nowMs - lastActivityMs) >= thresholdMs;
}

const StreamingTrailingActivity = memo(function StreamingTrailingActivity({
  liveStartedAtMs,
  latestActivityMs,
  latestVisibleProgressMs,
  queuedNotificationCount,
  onInterruptAndContinue,
  isNetworkOffline,
  autoFoldCompleted,
  cwd,
  agentId,
  thinkingSignature,
  getThinkingDurationMs,
  holdViewport,
}: {
  liveStartedAtMs: number | null;
  latestActivityMs: number | null | undefined;
  latestVisibleProgressMs: number | null | undefined;
  queuedNotificationCount: number;
  onInterruptAndContinue?: (() => void) | null;
  isNetworkOffline: boolean;
  autoFoldCompleted: boolean;
  cwd: string;
  agentId: string;
  thinkingSignature: string;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  holdViewport: ViewportAnchorHoldFn;
}) {
  const { t } = useI18n();
  const fallbackStartRef = useRef<number>(Date.now());
  const nowMs = useLiveSecondClock(true);
  const effectiveLastActivityMs = latestActivityMs ?? liveStartedAtMs ?? fallbackStartRef.current;

  if (!shouldShowStreamingTailActivity(nowMs, effectiveLastActivityMs)) {
    return null;
  }

  return (
    <TurnActivityDisclosure
      items={EMPTY_TIMELINE_ITEMS}
      isWorkActive
      liveLabel={t("stage.working")}
      liveDetail={null}
      settledLabel=""
      summaryCounts={EMPTY_WORK_COUNTS}
      labelIncludesFailure={false}
      defaultCollapsed={autoFoldCompleted}
      autoOpenForError={false}
      liveStartedAtMs={liveStartedAtMs}
      latestActivityMs={effectiveLastActivityMs}
      latestVisibleProgressMs={latestVisibleProgressMs ?? effectiveLastActivityMs}
      queuedNotificationCount={queuedNotificationCount}
      onInterruptAndContinue={onInterruptAndContinue}
      isNetworkOffline={isNetworkOffline}
      isBackgroundWait={false}
      settledDurationMs={null}
      cwd={cwd}
      agentId={agentId}
      thinkingSignature={thinkingSignature}
      getThinkingDurationMs={getThinkingDurationMs}
      holdViewport={holdViewport}
    />
  );
});

/**
 * Renders a complete Conversation Turn as chronological interleaved segments:
 * - Consecutive thinking, tool calls, and resolved gates fold cleanly inside per-batch
 *   `TurnActivityDisclosure` disclosures with live elapsed / stall / offline indicators.
 * - Every assistant reply (`assistant_message`) renders inline as top-level Markdown prose and
 *   is never swallowed into step disclosures — including interim updates and replies prior to
 *   post-turn autonomous wakeups.
 * - Completed historical turns are strictly memoized and receive stabilized `hostArtifacts`
 *   so live streaming deltas on the active tail turn never reconcile historical turns.
 */
export const TurnStageSection = memo(function TurnStageSection({
  turn,
  dispatches,
  isTurnActive,
  isLastTurn,
  agentStatusLabel = null,
  hasLiveBackgroundTasks = false,
  hasPlanSteps = false,
  isNetworkOffline,
  animatePromptEntrance = false,
  autoFoldCompleted,
  hostId,
  cwd,
  agentId,
  hostArtifacts: providedHostArtifacts,
  timing,
  thinkingSignature: _thinkingSignature,
  getThinkingDurationMs,
  holdViewport,
  onRequestRewind,
  onRequestFork,
  canForkWorktree = false,
  onPreviewImage,
  sessionSubagents,
  sessionSubagentMatcher,
  onOpenSubagent,
  parentMessageLabel,
  queuedNotificationCount = 0,
  onInterruptAndContinue = null,
  isCollapsed = false,
  onToggleCollapsed,
}: {
  turn: StageTurnView;
  dispatches?: ReturnType<typeof resolveDelegationDispatches>;
  isTurnActive: boolean;
  isLastTurn?: boolean;
  agentStatusLabel?: string | null;
  hasLiveBackgroundTasks?: boolean;
  hasPlanSteps?: boolean;
  isNetworkOffline: boolean;
  animatePromptEntrance?: boolean;
  autoFoldCompleted: boolean;
  hostId: string;
  cwd: string;
  agentId: string;
  hostArtifacts?: readonly ArtifactSummary[];
  timing?: TurnTimingRecord;
  thinkingSignature: string;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  holdViewport: ViewportAnchorHoldFn;
  onRequestRewind: (seq: number, promptText: string, badges: readonly PromptBadge[]) => void;
  onRequestFork?: (upToSeq: number | null, worktree: boolean) => void;
  canForkWorktree?: boolean;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
  sessionSubagents?: readonly ResolvedSubagentItem[];
  sessionSubagentMatcher?: SubagentNameMatcher | null;
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
  parentMessageLabel?: string | null;
  queuedNotificationCount?: number;
  onInterruptAndContinue?: (() => void) | null;
  isCollapsed?: boolean;
  onToggleCollapsed?: (turnKey: string) => void;
}) {
  const { t, locale } = useI18n();
  const rawSegments = useMemo(() => {
    if (!hasPlanSteps) return turn.live_segments;
    return turn.live_segments.filter((seg) => {
      if (seg.kind !== "notice" || seg.item.item_type !== "notice") return true;
      return !/^Plan:\n\[[x~ !-]\] /.test(seg.item.message);
    });
  }, [hasPlanSteps, turn.live_segments]);
  // When `hostArtifacts` is passed from StreamStage, avoid touching `s.hostDetails` so
  // historical turns register 0 topic subscriptions and never wake on `host_changed` bursts.
  const fallbackHostArtifacts = useOrbitSelector((s) =>
    providedHostArtifacts !== undefined
      ? EMPTY_ARTIFACTS
      : (s.hostDetails[hostId]?.artifacts ?? EMPTY_ARTIFACTS),
  );
  const hostArtifacts = providedHostArtifacts ?? fallbackHostArtifacts;

  const turnToolArtifactRefs = useMemo(
    () => parseArtifactRefsFromTurnTools(turn, hostArtifacts, locale),
    [turn, hostArtifacts, locale],
  );
  const lastRawSeg = rawSegments[rawSegments.length - 1] ?? null;

  const { segments, finishStatuses } = useMemo(() => {
    const out: typeof rawSegments = [];
    for (const seg of rawSegments) {
      if (seg.kind === "wakeup") {
        if (isHiddenWakeupLabel(seg.label)) continue;
        const prev = out[out.length - 1];
        if (prev?.kind === "wakeup" && prev.label === seg.label) continue;
      }
      out.push(seg);
    }
    return coalescePeerFinishes(turn.agent_message, out, hostId);
  }, [rawSegments, turn.agent_message, hostId]);

  const isWaitingOnlyOnBackgroundTasks =
    hasLiveBackgroundTasks &&
    ((lastRawSeg?.kind === "wakeup" && isWaitingWakeupLabel(lastRawSeg.label)) ||
      (Boolean(agentStatusLabel && /background task/i.test(agentStatusLabel)) &&
        (lastRawSeg === null ||
          (lastRawSeg.kind === "assistant_message" &&
            lastRawSeg.item.item_type === "assistant_message" &&
            !lastRawSeg.item.streaming))));

  const stepGroupSplits = useMemo(() => {
    const map = new Map<string, ReturnType<typeof splitStepGroupChronologically>>();
    for (const seg of segments) {
      if (seg.kind === "step_group") {
        map.set(seg.key, splitStepGroupChronologically(seg.items));
      }
    }
    return map;
  }, [segments]);

  const isTrailingSettledDelegation = useMemo(() => {
    if (!isTurnActive || lastRawSeg?.kind !== "step_group") return false;
    const lastGroup = segments.at(-1);
    const split = lastGroup && stepGroupSplits.get(lastGroup.key);
    const lastChunk = split ? (split.chunks[split.chunks.length - 1] ?? null) : null;
    if (!lastChunk || lastChunk.kind !== "delegation") return false;
    const matches = dispatches ?? resolveDelegationDispatches(lastChunk.items, sessionSubagents ?? EMPTY_SUBAGENTS, hostId);
    return !lastChunk.items.some((it) => {
      if (it.status === "running") return true;
      const resolved = matches.get(it);
      return (
        resolved?.subagent.status === "running" ||
        resolved?.subagent.status === "awaiting_permission"
      );
    });
  }, [isTurnActive, lastRawSeg, segments, stepGroupSplits, sessionSubagents, hostId, dispatches]);

  const needsTrailingActivity =
    isTurnActive &&
    !isWaitingOnlyOnBackgroundTasks &&
    (lastRawSeg === null ||
      (lastRawSeg.kind === "assistant_message" &&
        lastRawSeg.item.item_type === "assistant_message" &&
        !lastRawSeg.item.streaming) ||
      lastRawSeg.kind === "notice" ||
      lastRawSeg.kind === "wakeup" ||
      lastRawSeg.kind === "compaction" ||
      lastRawSeg.kind === "agent_message" ||
      isTrailingSettledDelegation);

  const isStreamingTrailingAssistant =
    isTurnActive &&
    !isWaitingOnlyOnBackgroundTasks &&
    lastRawSeg !== null &&
    lastRawSeg.kind === "assistant_message" &&
    lastRawSeg.item.item_type === "assistant_message" &&
    lastRawSeg.item.streaming;

  const hasBody = segments.length > 0 || finishStatuses.size > 0 || needsTrailingActivity;
  const hasBodyContent = segments.length > 0 || needsTrailingActivity ||
    turnToolArtifactRefs.length > 0 || turn.turn_diffs.length > 0;
  const liveStartedAtMs = timing?.startedAtMs ?? turn.started_at_ms ?? null;
  const promptItem =
    turn.user_prompt?.item_type === "user_prompt" ? turn.user_prompt : null;
  const footer = getTurnFooter(turn, { isActive: isTurnActive, isLastTurn, canFork: !!onRequestFork, timing });
  const handleToggleCollapse = useCallback(() => {
    onToggleCollapsed?.(turn.key);
  }, [onToggleCollapsed, turn.key]);
  const collapsedSummary = useMemo(() => {
    if (!isCollapsed || isTurnActive) return null;
    let preview = "";
    if (promptItem) {
      const pres = extractUserPromptPresentation(promptItem);
      preview = pres.cleanText || pres.badges.map((b) => b.label).join(" ");
    } else if (turn.agent_message?.item_type === "agent_message") {
      preview = turn.agent_message.text;
    } else if (turn.terminal_reply?.item_type === "assistant_message") {
      preview = turn.terminal_reply.text;
    }
    const cleanPreview = preview.replace(/\s+/g, " ").trim() || t("stage.prompt");

    let toolCount = 0;
    for (const seg of turn.live_segments) {
      if (seg.kind !== "step_group") continue;
      for (const item of seg.items) {
        if (item.item_type === "tool_call") toolCount += 1;
      }
    }
    if (toolCount === 0) {
      for (const step of turn.folded_steps) {
        if (step.item_type === "tool_call") toolCount += 1;
      }
    }
    let additions = 0;
    let deletions = 0;
    for (const diff of turn.turn_diffs) {
      additions += diff.additions;
      deletions += diff.deletions;
    }
    const durationMs =
      timing?.durationMs ??
      timing?.workDurationMs ??
      turn.work_duration_ms ??
      (turn.completed_at_ms != null && turn.started_at_ms != null
        ? Math.max(0, turn.completed_at_ms - turn.started_at_ms)
        : null);
    return { cleanPreview, toolCount, additions, deletions, durationMs };
  }, [isCollapsed, isTurnActive, promptItem, turn, timing, t]);

  if (isCompactionMarkerTurn(turn)) {
    return (
      <div className="min-w-0 px-[10px]">
        <CompactionMilestoneRow label={turn.compactions[0]?.label} />
      </div>
    );
  }

  if (collapsedSummary) {
    return (
      <div className="min-w-0 px-[6px]">
        <button
          type="button"
          onClick={handleToggleCollapse}
          aria-expanded={false}
          title={t("stage.expandTurnTooltip")}
          className="border-border-subtle bg-surface-raised/65 hover:bg-surface-raised hover:border-border-default flex w-full cursor-pointer items-center gap-2.5 rounded-[10px] border px-3 py-2 text-left transition-colors"
        >
          <ChevronRight className="text-ink-muted size-3.5 shrink-0" />
          <span className="text-ink-secondary min-w-0 flex-1 truncate text-[13px] leading-[18px] font-medium">
            {collapsedSummary.cleanPreview}
          </span>
          <span className="text-ink-tertiary flex shrink-0 items-center gap-2 text-[11px] leading-[15px] tabular-nums">
            {collapsedSummary.toolCount > 0 && (
              <span>{t("stage.collapsedTurnTools", { count: collapsedSummary.toolCount })}</span>
            )}
            {(collapsedSummary.additions > 0 || collapsedSummary.deletions > 0) && (
              <DiffStat
                additions={collapsedSummary.additions}
                deletions={collapsedSummary.deletions}
              />
            )}
            {collapsedSummary.durationMs != null && collapsedSummary.durationMs >= 1000 && (
              <span>{formatDurationCompact(collapsedSummary.durationMs)}</span>
            )}
          </span>
        </button>
      </div>
    );
  }

  const collapseAction = !isTurnActive && onToggleCollapsed ? handleToggleCollapse : undefined;

  return (
    <div
      style={!isTurnActive ? { contain: "layout style" } : undefined}
      className={hasBody && !hasBodyContent ? "group/turn min-w-0" : "group/turn min-w-0 space-y-4"}
    >
      {promptItem && (
        <UserPromptBubble
          item={promptItem}
          hostId={hostId}
          agentId={agentId}
          cwd={cwd}
          holdViewport={holdViewport}
          animateEntrance={animatePromptEntrance}
          onRequestRewind={onRequestRewind}
          onCollapseTurn={collapseAction}
          onPreviewImage={onPreviewImage}
        />
      )}

      {turn.agent_message?.item_type === "agent_message" && (
        <AgentMessageCard
          hostId={hostId}
          agentId={agentId}
          item={turn.agent_message}
          holdViewport={holdViewport}
          coalescedFinishStatus={finishStatuses.get(turn.agent_message)}
          sessionSubagents={sessionSubagents}
          isParentTurnRunning={isTurnActive}
          onInterruptAndContinue={onInterruptAndContinue}
        />
      )}

      {hasBody && (
        <div className="flow-root min-w-0 px-[10px]">
          <div className="min-w-0 space-y-3">
            {(() => {
              let nextWorkOrdinal = 0;
              return segments.map((seg, idx) => {
                const isLastSeg = idx === segments.length - 1;

                if (seg.kind === "compaction") {
                  return <CompactionMilestoneRow key={seg.key} label={seg.label} />;
                }

                if (seg.kind === "wakeup") {
                  return <WakeupEventRow key={seg.key} label={seg.label} />;
                }

                if (seg.kind === "notice") {
                  if (seg.item.item_type !== "notice") return null;
                  return <StageNoticeSegment key={seg.key} item={seg.item} />;
                }

                if (seg.kind === "assistant_message") {
                  if (seg.item.item_type !== "assistant_message") return null;
                  return (
                    <StageAssistantMessageSegment
                      key={seg.key}
                      item={seg.item}
                      hostId={hostId}
                      cwd={cwd}
                      hostArtifacts={hostArtifacts}
                      sessionSubagents={sessionSubagents}
                      sessionSubagentMatcher={sessionSubagentMatcher}
                      onOpenSubagent={onOpenSubagent}
                    />
                  );
                }

                if (seg.kind === "agent_message") {
                  if (seg.item.item_type !== "agent_message") return null;
                  return (
                    <AgentMessageCard
                      key={seg.key}
                      hostId={hostId}
                      agentId={agentId}
                      item={seg.item}
                      holdViewport={holdViewport}
                      coalescedFinishStatus={finishStatuses.get(seg.item)}
                      inline
                      sessionSubagents={sessionSubagents}
                      isParentTurnRunning={isTurnActive}
                      onInterruptAndContinue={onInterruptAndContinue}
                    />
                  );
                }

                if (seg.kind !== "step_group") return null;

                const split =
                  stepGroupSplits.get(seg.key) ?? splitStepGroupChronologically(seg.items);

                if (!split.hasDelegations) {
                  const workOrdinal = nextWorkOrdinal++;
                  const isTrailingFailedGroup =
                    !seg.has_running_step && seg.summary.counts.unrecovered_failure_count > 0;
                  const hasRunningBgTask = seg.items.some(
                    (it) =>
                      it.item_type === "tool_call" &&
                      it.status === "running" &&
                      Boolean(it.background_task),
                  );
                  const hasRunningNonBgTool = seg.items.some(
                    (it) =>
                      it.item_type === "tool_call" &&
                      it.status === "running" &&
                      !it.background_task,
                  );
                  const isBackgroundWait = hasRunningBgTask && !hasRunningNonBgTool;
                  const isGroupActive =
                    isTurnActive &&
                    !seg.has_following_content &&
                    (seg.has_running_step || (isLastSeg && !isTrailingFailedGroup));
                  const groupThoughtMs =
                    computeTotalThinkingDurationMs(seg.items, agentId, getThinkingDurationMs) ??
                    seg.thought_duration_ms ??
                    null;
                  const groupSettledDurationMs =
                    seg.work_duration_ms ??
                    (segments.length === 1
                      ? (timing?.workDurationMs ?? turn.work_duration_ms ?? timing?.durationMs ?? null)
                      : null);
                  const groupLiveStartMs = seg.started_at_ms ?? liveStartedAtMs;

                  return (
                    <TurnActivityDisclosure
                      key={seg.key}
                      items={seg.items}
                      isWorkActive={isGroupActive}
                      liveLabel={seg.summary.live_label}
                      liveDetail={seg.summary.live_detail ?? null}
                      settledLabel={seg.summary.label}
                      hasProjectedSummary
                      thoughtDurationMs={groupThoughtMs}
                      summaryCounts={seg.summary.counts}
                      labelIncludesFailure={seg.summary.label_includes_failure}
                      defaultCollapsed={autoFoldCompleted}
                      autoOpenForError={
                        !isGroupActive &&
                        !seg.has_following_content &&
                        seg.summary.counts.unrecovered_failure_count > 0
                      }
                      liveStartedAtMs={groupLiveStartMs}
                      latestActivityMs={seg.latest_activity_ms ?? groupLiveStartMs}
                      latestVisibleProgressMs={turn.latest_visible_progress_ms ?? groupLiveStartMs}
                      queuedNotificationCount={isGroupActive ? queuedNotificationCount : 0}
                      onInterruptAndContinue={isGroupActive ? onInterruptAndContinue : null}
                      isNetworkOffline={isNetworkOffline}
                      isBackgroundWait={isBackgroundWait}
                      settledDurationMs={groupSettledDurationMs}
                      hostId={hostId}
                      cwd={cwd}
                      agentId={agentId}
                      turnIndex={turn.turn_index}
                      turnAnchorSeq={turn.anchor_seq}
                      workOrdinal={workOrdinal}
                      thinkingSignature={_thinkingSignature}
                      getThinkingDurationMs={getThinkingDurationMs}
                      holdViewport={holdViewport}
                      sessionSubagents={sessionSubagents}
                      onOpenSubagent={onOpenSubagent}
                    />
                  );
                }

                return split.chunks.map((chunk, chunkIdx) => {
                  const chunkKey =
                    chunkIdx === 0 && chunk.kind === "work"
                      ? seg.key
                      : `${seg.key}::${chunk.kind}::${chunk.anchorKey}`;

                  if (chunk.kind === "delegation") {
                    return (
                      <DelegationMilestoneRow
                        key={chunkKey}
                        items={chunk.items}
                        hostId={hostId}
                        agentId={agentId}
                        sessionSubagents={sessionSubagents}
                        dispatches={dispatches}
                        parentMessageLabel={parentMessageLabel}
                        holdViewport={holdViewport}
                      />
                    );
                  }

                  const workOrdinal = nextWorkOrdinal++;
                  const hasFollowingChunk = chunkIdx < split.chunks.length - 1;
                  const chunkHasFollowingContent = seg.has_following_content || hasFollowingChunk;
                  const chunkThoughtMs = computeTotalThinkingDurationMs(
                    chunk.items,
                    agentId,
                    getThinkingDurationMs,
                  );
                  const chunkSummary = summarizeWorkChunk(chunk.items, chunkThoughtMs, locale);
                  const chunkHasRunningStep = chunk.items.some(
                    (it) => it.item_type === "tool_call" && it.status === "running",
                  );
                  const chunkIsTrailingFailed =
                    !chunkHasRunningStep && chunkSummary.counts.unrecovered_failure_count > 0;
                  const chunkHasRunningBgTask = chunk.items.some(
                    (it) =>
                      it.item_type === "tool_call" &&
                      it.status === "running" &&
                      Boolean(it.background_task),
                  );
                  const chunkHasRunningNonBgTool = chunk.items.some(
                    (it) =>
                      it.item_type === "tool_call" &&
                      it.status === "running" &&
                      !it.background_task,
                  );
                  const chunkIsBackgroundWait = chunkHasRunningBgTask && !chunkHasRunningNonBgTool;
                  const isChunkActive =
                    isTurnActive &&
                    !chunkHasFollowingContent &&
                    (chunkHasRunningStep || (isLastSeg && !chunkIsTrailingFailed));
                  const chunkDurationMs = computeWorkChunkDurationMs(chunk.items);
                  const firstChunkTs = chunk.items[0]?.timestamp_ms ?? null;
                  const lastChunkTs = chunk.items[chunk.items.length - 1]?.timestamp_ms ?? null;
                  const chunkLiveStartMs =
                    typeof firstChunkTs === "number" && firstChunkTs > 0
                      ? firstChunkTs
                      : (seg.started_at_ms ?? liveStartedAtMs);
                  const chunkLatestActivityMs =
                    typeof lastChunkTs === "number" && lastChunkTs > 0
                      ? lastChunkTs
                      : (seg.latest_activity_ms ?? chunkLiveStartMs);

                  return (
                    <TurnActivityDisclosure
                      key={chunkKey}
                      items={chunk.items}
                      isWorkActive={isChunkActive}
                      liveLabel={chunkSummary.live_label}
                      liveDetail={chunkSummary.live_detail ?? null}
                      settledLabel={chunkSummary.label}
                      thoughtDurationMs={chunkThoughtMs}
                      summaryCounts={chunkSummary.counts}
                      labelIncludesFailure={chunkSummary.label_includes_failure}
                      defaultCollapsed={autoFoldCompleted}
                      autoOpenForError={
                        !isChunkActive &&
                        !chunkHasFollowingContent &&
                        chunkSummary.counts.unrecovered_failure_count > 0
                      }
                      liveStartedAtMs={chunkLiveStartMs}
                      latestActivityMs={chunkLatestActivityMs}
                      latestVisibleProgressMs={turn.latest_visible_progress_ms ?? chunkLiveStartMs}
                      queuedNotificationCount={isChunkActive ? queuedNotificationCount : 0}
                      onInterruptAndContinue={isChunkActive ? onInterruptAndContinue : null}
                      isNetworkOffline={isNetworkOffline}
                      isBackgroundWait={chunkIsBackgroundWait}
                      settledDurationMs={chunkDurationMs}
                      hostId={hostId}
                      cwd={cwd}
                      agentId={agentId}
                      turnIndex={turn.turn_index}
                      turnAnchorSeq={turn.anchor_seq}
                      workOrdinal={workOrdinal}
                      thinkingSignature={_thinkingSignature}
                      getThinkingDurationMs={getThinkingDurationMs}
                      holdViewport={holdViewport}
                      sessionSubagents={sessionSubagents}
                      onOpenSubagent={onOpenSubagent}
                    />
                  );
                });
              });
            })()}

            {turnToolArtifactRefs.length > 0 && (
              <div className="flex flex-wrap items-center gap-2 pt-0.5">
                {turnToolArtifactRefs.map((ref) => (
                  <InlineArtifactCard key={ref.key} hostId={hostId} artifact={ref} />
                ))}
              </div>
            )}

            {needsTrailingActivity && (() => {
              const trailingLabel =
                lastRawSeg === null
                  ? t("stage.working")
                  : lastRawSeg.kind === "wakeup" &&
                      !isHiddenWakeupLabel(lastRawSeg.label) &&
                      lastRawSeg.label.trim()
                    ? lastRawSeg.label.trim().endsWith("…")
                      ? lastRawSeg.label.trim()
                      : `${lastRawSeg.label.trim()}…`
                    : agentStatusLabel &&
                        /waking/i.test(agentStatusLabel) &&
                        !/background[_\s]task/i.test(agentStatusLabel)
                      ? agentStatusLabel.trim().endsWith("…")
                        ? agentStatusLabel.trim()
                        : `${agentStatusLabel.trim()}…`
                      : t("stage.thinkingPending");
              const isTrailingBackgroundWait = /background task/i.test(trailingLabel);
              return (
                <TurnActivityDisclosure
                  items={EMPTY_TIMELINE_ITEMS}
                  isWorkActive
                  liveLabel={trailingLabel}
                  liveDetail={null}
                  settledLabel=""
                  summaryCounts={EMPTY_WORK_COUNTS}
                  labelIncludesFailure={false}
                  defaultCollapsed={autoFoldCompleted}
                  autoOpenForError={false}
                  liveStartedAtMs={liveStartedAtMs}
                  latestActivityMs={
                    turn.latest_activity_ms ?? turn.completed_at_ms ?? liveStartedAtMs
                  }
                  latestVisibleProgressMs={turn.latest_visible_progress_ms ?? liveStartedAtMs}
                  queuedNotificationCount={queuedNotificationCount}
                  onInterruptAndContinue={onInterruptAndContinue}
                  isNetworkOffline={isNetworkOffline}
                  isBackgroundWait={isTrailingBackgroundWait}
                  settledDurationMs={null}
                  cwd={cwd}
                  agentId={agentId}
                  thinkingSignature={_thinkingSignature}
                  getThinkingDurationMs={getThinkingDurationMs}
                  holdViewport={holdViewport}
                />
              );
            })()}

            {!needsTrailingActivity && isStreamingTrailingAssistant && (
              <StreamingTrailingActivity
                liveStartedAtMs={liveStartedAtMs}
                latestActivityMs={
                  turn.latest_activity_ms ?? turn.completed_at_ms ?? liveStartedAtMs
                }
                latestVisibleProgressMs={turn.latest_visible_progress_ms ?? liveStartedAtMs}
                queuedNotificationCount={queuedNotificationCount}
                onInterruptAndContinue={onInterruptAndContinue}
                isNetworkOffline={isNetworkOffline}
                autoFoldCompleted={autoFoldCompleted}
                cwd={cwd}
                agentId={agentId}
                thinkingSignature={_thinkingSignature}
                getThinkingDurationMs={getThinkingDurationMs}
                holdViewport={holdViewport}
              />
            )}

            {!isTurnActive && <TurnChangesBar diffs={turn.turn_diffs} cwd={cwd} />}
          </div>
          {footer && (
            <TurnFooter
              {...footer}
              isLastTurn={isLastTurn}
              canForkWorktree={canForkWorktree}
              onRequestFork={onRequestFork}
              onCollapseTurn={collapseAction}
            />
          )}
        </div>
      )}
    </div>
  );
}, (prev, next) =>
  prev.turn === next.turn &&
  prev.isTurnActive === next.isTurnActive &&
  prev.isLastTurn === next.isLastTurn &&
  prev.agentStatusLabel === next.agentStatusLabel &&
  prev.hasLiveBackgroundTasks === next.hasLiveBackgroundTasks &&
  prev.hasPlanSteps === next.hasPlanSteps &&
  prev.isNetworkOffline === next.isNetworkOffline &&
  prev.animatePromptEntrance === next.animatePromptEntrance &&
  prev.autoFoldCompleted === next.autoFoldCompleted &&
  prev.hostId === next.hostId &&
  prev.cwd === next.cwd &&
  prev.agentId === next.agentId &&
  prev.hostArtifacts === next.hostArtifacts &&
  prev.thinkingSignature === next.thinkingSignature &&
  prev.timing?.startedAtMs === next.timing?.startedAtMs &&
  prev.timing?.durationMs === next.timing?.durationMs &&
  prev.timing?.workDurationMs === next.timing?.workDurationMs &&
  prev.timing?.completedAtMs === next.timing?.completedAtMs &&
  prev.getThinkingDurationMs === next.getThinkingDurationMs &&
  prev.holdViewport === next.holdViewport &&
  prev.onRequestRewind === next.onRequestRewind &&
  prev.onRequestFork === next.onRequestFork &&
  prev.canForkWorktree === next.canForkWorktree &&
  prev.onPreviewImage === next.onPreviewImage &&
  prev.dispatches === next.dispatches &&
  prev.sessionSubagents === next.sessionSubagents &&
  prev.sessionSubagentMatcher === next.sessionSubagentMatcher &&
  prev.onOpenSubagent === next.onOpenSubagent &&
  prev.parentMessageLabel === next.parentMessageLabel &&
  prev.queuedNotificationCount === next.queuedNotificationCount &&
  prev.onInterruptAndContinue === next.onInterruptAndContinue &&
  prev.isCollapsed === next.isCollapsed &&
  prev.onToggleCollapsed === next.onToggleCollapsed,
);
