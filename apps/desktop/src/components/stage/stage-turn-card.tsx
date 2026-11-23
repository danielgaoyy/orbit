import { memo, useMemo } from "react";
import { AlertTriangle } from "lucide-react";
import { MarkdownBlocksView, MarkdownLinkProvider } from "@/components/markdown-blocks";
import { AgentMessageCard } from "@/components/stage/subagents-track";
import type {
  ArtifactSummary,
  PromptBadge,
  StageTurnView,
  TimelineItemView,
} from "@/lib/orbit-ipc";
import {
  type ResolvedSubagentItem,
  type SubagentNameMatcher,
  resolveToolCallSubagent,
} from "@/lib/subagents";
import { formatThoughtSummaryLabel } from "@/lib/timeline-coalesce";
import {
  computeWorkChunkDurationMs,
  splitStepGroupChronologically,
  summarizeWorkChunk,
} from "@/lib/turn-delegation-split";
import { cn } from "@/lib/utils";
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
  isHiddenWakeupLabel,
  isWaitingWakeupLabel,
  parseArtifactRefsFromMessage,
  parseArtifactRefsFromTurnTools,
  resolveTurnForkSeq,
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

interface InlineAgentMessageSegment {
  kind: "agent_message";
  key: string;
  item: Extract<TimelineItemView, { item_type: "agent_message" }>;
}

/**
 * Type-safe runtime guard for inline `agent_message` segments emitted by the stage
 * builder (compatible both before and after `TurnLiveSegmentView` includes `"agent_message"`).
 */
function asInlineAgentMessageSegment(seg: unknown): InlineAgentMessageSegment | null {
  if (!seg || typeof seg !== "object") return null;
  const candidate = seg as { kind?: unknown; key?: unknown; item?: TimelineItemView };
  if (
    candidate.kind === "agent_message" &&
    typeof candidate.key === "string" &&
    candidate.item?.item_type === "agent_message"
  ) {
    return {
      kind: "agent_message",
      key: candidate.key,
      item: candidate.item,
    };
  }
  return null;
}

const StageNoticeSegment = memo(function StageNoticeSegment({
  item,
}: {
  item: Extract<TimelineItemView, { item_type: "notice" }>;
}) {
  const isError = item.level === "error";
  const isWarning = item.level === "warning";
  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-xl border px-3.5 py-2 text-[13px] leading-[1.45]",
        isError
          ? "border-rose-500/30 bg-rose-500/[0.07] text-rose-700 dark:text-rose-300"
          : isWarning
            ? "border-amber-500/30 bg-amber-500/[0.07] text-amber-800 dark:text-amber-200"
            : "text-muted-foreground border-border bg-muted/50",
      )}
    >
      <AlertTriangle
        className={cn(
          "mt-[2px] size-4 shrink-0",
          isError
            ? "text-rose-600 dark:text-rose-400"
            : isWarning
              ? "text-amber-600 dark:text-amber-400"
              : "text-muted-foreground",
        )}
      />
      <span className="selectable-text min-w-0 whitespace-pre-wrap break-words">
        {item.message}
      </span>
    </div>
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
  const msgArtifacts = useMemo(
    () => (!item.streaming ? parseArtifactRefsFromMessage(item.text, hostArtifacts) : []),
    [item.streaming, item.text, hostArtifacts],
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
  isTurnActive,
  isLastTurn = false,
  agentStatusLabel = null,
  hasLiveBackgroundTasks = false,
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
}: {
  turn: StageTurnView;
  isTurnActive: boolean;
  isLastTurn?: boolean;
  agentStatusLabel?: string | null;
  hasLiveBackgroundTasks?: boolean;
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
}) {
  const rawSegments = turn.live_segments;
  // When `hostArtifacts` is passed from StreamStage, avoid touching `s.hostDetails` so
  // historical turns register 0 topic subscriptions and never wake on `host_changed` bursts.
  const fallbackHostArtifacts = useOrbitSelector((s) =>
    providedHostArtifacts !== undefined
      ? EMPTY_ARTIFACTS
      : (s.hostDetails[hostId]?.artifacts ?? EMPTY_ARTIFACTS),
  );
  const hostArtifacts = providedHostArtifacts ?? fallbackHostArtifacts;

  const turnToolArtifactRefs = useMemo(
    () => parseArtifactRefsFromTurnTools(turn, hostArtifacts),
    [turn, hostArtifacts],
  );
  const lastRawSeg = rawSegments[rawSegments.length - 1] ?? null;

  const segments = useMemo(() => {
    const out: typeof rawSegments = [];
    for (const seg of rawSegments) {
      if (seg.kind === "wakeup") {
        if (isHiddenWakeupLabel(seg.label)) continue;
        const prev = out[out.length - 1];
        if (prev?.kind === "wakeup" && prev.label === seg.label) continue;
      }
      out.push(seg);
    }
    return out;
  }, [rawSegments]);

  if (
    turn.is_compaction_command &&
    turn.folded_steps.length === 0 &&
    turn.terminal_reply === null &&
    turn.notices.length === 0
  ) {
    return (
      <div className="min-w-0 px-[10px]">
        <CompactionMilestoneRow label={turn.compactions[0]?.label || "Context compacted"} />
      </div>
    );
  }

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
    const split = stepGroupSplits.get(lastRawSeg.key);
    const lastChunk = split ? (split.chunks[split.chunks.length - 1] ?? null) : null;
    if (!lastChunk || lastChunk.kind !== "delegation") return false;
    return !lastChunk.items.some((it) => {
      if (it.status === "running") return true;
      const resolved = resolveToolCallSubagent(it, sessionSubagents ?? EMPTY_SUBAGENTS, hostId);
      return (
        resolved?.subagent.status === "running" ||
        resolved?.subagent.status === "awaiting_permission"
      );
    });
  }, [isTurnActive, lastRawSeg, stepGroupSplits, sessionSubagents, hostId]);

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
      asInlineAgentMessageSegment(lastRawSeg) !== null ||
      isTrailingSettledDelegation);

  const hasBody = segments.length > 0 || needsTrailingActivity;
  const liveStartedAtMs = timing?.startedAtMs ?? turn.started_at_ms ?? null;
  const promptItem =
    turn.user_prompt?.item_type === "user_prompt" ? turn.user_prompt : null;
  const lastReplyText =
    !isTurnActive && turn.last_assistant_reply?.item_type === "assistant_message"
      ? turn.last_assistant_reply.text.trim() || null
      : null;
  const turnForkSeq = useMemo(() => resolveTurnForkSeq(turn), [turn]);

  return (
    <div
      style={!isTurnActive ? { contain: "layout style" } : undefined}
      className="min-w-0 space-y-4"
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
          onPreviewImage={onPreviewImage}
        />
      )}

      {turn.agent_message?.item_type === "agent_message" && (
        <AgentMessageCard hostId={hostId} agentId={agentId} item={turn.agent_message} />
      )}

      {hasBody && (
        <div className="group/turn min-w-0 px-[10px]">
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

                const inlineAgentMsg = asInlineAgentMessageSegment(seg);
                if (inlineAgentMsg) {
                  return (
                    <AgentMessageCard
                      key={inlineAgentMsg.key}
                      hostId={hostId}
                      agentId={agentId}
                      item={inlineAgentMsg.item}
                      inline
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
                  const groupSettledLabel =
                    seg.summary.counts.action_steps === 0 && seg.summary.counts.thought_count > 0
                      ? formatThoughtSummaryLabel(groupThoughtMs, seg.summary.counts.thought_count)
                      : seg.summary.label;
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
                      settledLabel={groupSettledLabel}
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
                        onOpenSubagent={onOpenSubagent}
                        parentMessageLabel={parentMessageLabel}
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
                  const chunkSummary = summarizeWorkChunk(chunk.items, chunkThoughtMs);
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
                  const chunkSettledLabel =
                    chunkSummary.counts.action_steps === 0 && chunkSummary.counts.thought_count > 0
                      ? formatThoughtSummaryLabel(chunkThoughtMs, chunkSummary.counts.thought_count)
                      : chunkSummary.label;
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
                      settledLabel={chunkSettledLabel}
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
                  ? "Working…"
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
                      : "Thinking…";
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
                  latestActivityMs={turn.completed_at_ms ?? liveStartedAtMs}
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

            {!isTurnActive && <TurnChangesBar diffs={turn.turn_diffs} cwd={cwd} />}

            <TurnFooter
              isTurnActive={isTurnActive}
              timing={timing}
              completedAtMs={turn.completed_at_ms}
              replyText={lastReplyText}
              forkUpToSeq={turnForkSeq}
              isLastTurn={isLastTurn}
              canForkWorktree={canForkWorktree}
              onRequestFork={onRequestFork}
            />
          </div>
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
  prev.sessionSubagents === next.sessionSubagents &&
  prev.sessionSubagentMatcher === next.sessionSubagentMatcher &&
  prev.onOpenSubagent === next.onOpenSubagent &&
  prev.parentMessageLabel === next.parentMessageLabel,
);
