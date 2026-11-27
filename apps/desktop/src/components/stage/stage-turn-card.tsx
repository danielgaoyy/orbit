import { memo, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, ArrowUpRight, ChevronRight } from "lucide-react";
import { MarkdownBlocksView, MarkdownLinkProvider } from "@/components/markdown-blocks";
import { AgentMessageCard } from "@/components/stage/subagents-track";
import { ContextBanner } from "@/components/ui/context-banner";
import { useLiveSecondClock } from "@/lib/live-clock";
import {
  orbitIpc,
  type ArtifactSummary,
  type PromptBadge,
  type RichBlockView,
  type StageTurnView,
  type TimelineItemView,
} from "@/lib/orbit-ipc";
import {
  type ResolvedSubagentItem,
  type SubagentNameMatcher,
  resolveDelegationDispatches,
} from "@/lib/subagents";
import { useI18n } from "@/lib/i18n";
import { extractCleanAgentMessageBody } from "@/lib/agent-message-clean";
import {
  coalescePeerFinishes,
  isDirectAgentPromptBubble,
  isTimelineAgentMessageHidden,
  resolveCompletedTurnWorkFold,
  splitStepGroupChronologically,
  summarizeWorkChunk,
} from "@/lib/turn-delegation-split";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
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
  getTurnFooter,
  isCompactionMarkerTurn,
  isHiddenWakeupLabel,
  isLiveCompactionSegment,
  isWaitingWakeupLabel,
  parseArtifactRefsFromMessage,
  parseArtifactRefsFromTurnTools,
} from "./stage-helpers";
import {
  type PromptSourceMeta,
  TurnChangesBar,
  TurnFooter,
  UserPromptBubble,
} from "./stage-prompt-controls";
import {
  CompactingStatusRow,
  CompactionMilestoneRow,
  DelegationMilestoneRow,
  InlineArtifactCard,
  StepTimelineRail,
  TurnActivityDisclosure,
  TurnWorkedDisclosure,
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
  autoFoldCompleted,
  cwd,
  agentId,
  thinkingSignature,
  getThinkingDurationMs,
  holdViewport,
}: {
  liveStartedAtMs: number | null;
  latestActivityMs: number | null | undefined;
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
const PersonalAgentReportHeader = memo(function PersonalAgentReportHeader({
  item,
  fallbackHostId,
}: {
  item: Extract<TimelineItemView, { item_type: "agent_message" }>;
  fallbackHostId: string;
}) {
  const { t } = useI18n();
  const targetHostId = item.source_host_id ?? fallbackHostId;
  const targetAgentId = item.source_agent_id ?? null;
  const hostName = useOrbitSelector((s) => {
    const h = s.fleet.hosts.find((x) => x.host_id === targetHostId);
    if (!h) return null;
    return s.fleet.hosts.length > 1 ? (h.display_name?.trim() || h.hostname) : null;
  });
  const liveTitle = useOrbitSelector((s) =>
    targetAgentId
      ? (s.hostDetails[targetHostId]?.agents.find((a) => a.agent_id === targetAgentId)?.title ?? null)
      : null,
  );
  const sourceLabel = liveTitle?.trim() || item.source_title?.trim() || targetAgentId || "Worker";
  const isError =
    item.event === "errored" ||
    item.event === "failed" ||
    /^Agent\s+\S+(?:\s+\(.*\))?\s+(?:failed|errored)\./im.test(item.text);

  const labelText = hostName
    ? `${t("personalAgent.reportFrom", { source: sourceLabel })} · ${hostName}`
    : t("personalAgent.reportFrom", { source: sourceLabel });

  if (!targetAgentId) {
    return (
      <div className="flex items-center gap-1.5 px-[10px] pb-0.5 text-[11px] font-medium text-ink-muted">
        <span
          aria-hidden="true"
          className={cn(
            "size-1.5 shrink-0 rounded-full",
            isError ? "bg-signal-danger" : "bg-ink-faint",
          )}
        />
        <span className="truncate">{labelText}</span>
      </div>
    );
  }

  return (
    <div className="flex items-center px-[10px] pb-0.5">
      <button
        type="button"
        onClick={() => orbitActions.selectAgent(targetHostId, targetAgentId)}
        className="group inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-md py-0.5 text-[11px] font-medium text-ink-muted transition-colors hover:text-ink-primary"
      >
        <span
          aria-hidden="true"
          className={cn(
            "size-1.5 shrink-0 rounded-full",
            isError ? "bg-signal-danger" : "bg-ink-faint group-hover:bg-ink-secondary",
          )}
        />
        <span className="truncate">{labelText}</span>
        <ArrowUpRight className="size-3 shrink-0 opacity-0 transition-opacity group-hover:opacity-80" />
      </button>
    </div>
  );
});

const PersonalAgentReportDetailCard = memo(function PersonalAgentReportDetailCard({
  item,
  fallbackHostId,
  cwd,
  showSourcePrefix = false,
}: {
  item: Extract<TimelineItemView, { item_type: "agent_message" }>;
  fallbackHostId: string;
  cwd: string;
  showSourcePrefix?: boolean;
}) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const [parsedBlocks, setParsedBlocks] = useState<RichBlockView[] | null>(null);
  const cleanDetail = useMemo(() => extractCleanAgentMessageBody(item.text), [item.text]);
  const targetHostId = item.source_host_id ?? fallbackHostId;
  const targetAgentId = item.source_agent_id ?? null;
  const sourceLabel = item.source_title?.trim() || targetAgentId || "Worker";

  useEffect(() => {
    if (!expanded || !cleanDetail) return;
    let active = true;
    void orbitIpc
      .parseMarkdownPreview(cleanDetail, false)
      .then((blocks) => {
        if (active && blocks && blocks.length > 0) setParsedBlocks(blocks);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [expanded, cleanDetail]);

  if (!cleanDetail) return null;

  return (
    <div className="pt-1.5">
      <button
        type="button"
        aria-expanded={expanded}
        onClick={() => setExpanded((v) => !v)}
        className="inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-lg border border-border-subtle/70 bg-surface-recessed/50 px-2 py-1 text-[11px] font-medium text-ink-muted transition-colors hover:border-border-subtle hover:bg-surface-recessed hover:text-ink-primary"
      >
        <ChevronRight
          className={cn(
            "size-3 shrink-0 transition-transform duration-150 ease-[var(--ease-out-expo)]",
            expanded && "rotate-90",
          )}
        />
        <span className="truncate">
          {showSourcePrefix ? `${sourceLabel} · ` : ""}
          {expanded ? t("personalAgent.reportHideDetails") : t("personalAgent.reportDetails")}
        </span>
      </button>
      {expanded && (
        <div className="mt-1.5 rounded-xl border border-border-subtle bg-surface-recessed px-3 py-2.5 text-[13px] leading-[1.55] text-ink-secondary selectable-text">
          {parsedBlocks && parsedBlocks.length > 0 ? (
            <MarkdownLinkProvider hostId={targetHostId} cwd={cwd}>
              <MarkdownBlocksView blocks={parsedBlocks} streaming={false} />
            </MarkdownLinkProvider>
          ) : (
            <div className="whitespace-pre-wrap break-words">{cleanDetail}</div>
          )}
        </div>
      )}
    </div>
  );
});

export const TurnStageSection = memo(function TurnStageSection({
  turn,
  dispatches,
  isTurnActive,
  isLastTurn,
  agentStatusLabel = null,
  hasLiveBackgroundTasks = false,
  hasPlanSteps = false,
  isNetworkOffline: _isNetworkOffline,
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
  isPersonalAgentSession = false,
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
  isPersonalAgentSession?: boolean;
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
    return coalescePeerFinishes(turn.agent_message, out, hostId, {
      preserveFinishReports: isPersonalAgentSession,
    });
  }, [rawSegments, turn.agent_message, hostId, isPersonalAgentSession]);

  const isWaitingOnlyOnBackgroundTasks =
    hasLiveBackgroundTasks &&
    ((lastRawSeg?.kind === "wakeup" &&
      (isWaitingWakeupLabel(lastRawSeg.label) || /background task/i.test(lastRawSeg.label))) ||
      (Boolean(
        agentStatusLabel &&
          (isWaitingWakeupLabel(agentStatusLabel) ||
            /background task|\btimers?\b/i.test(agentStatusLabel)),
      ) &&
        (lastRawSeg === null ||
          lastRawSeg.kind === "wakeup" ||
          lastRawSeg.kind === "notice" ||
          lastRawSeg.kind === "agent_message" ||
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

  const hasBody =
    segments.length > 0 ||
    finishStatuses.size > 0 ||
    needsTrailingActivity ||
    (isPersonalAgentSession &&
      turn.agent_message?.item_type === "agent_message" &&
      !turn.agent_message.permission_context &&
      turn.agent_message.kind !== "heartbeat");
  const hasBodyContent = segments.length > 0 || needsTrailingActivity ||
    turnToolArtifactRefs.length > 0 || turn.turn_diffs.length > 0;
  const liveStartedAtMs = timing?.startedAtMs ?? turn.started_at_ms ?? null;
  const promptItem =
    turn.user_prompt?.item_type === "user_prompt" ? turn.user_prompt : null;
  const needsHistoricalParentSource =
    turn.turn_index === 0 &&
    promptItem !== null &&
    (!promptItem.badges || promptItem.badges.length === 0);
  const historicalParentAgentId = useOrbitSelector((s) =>
    needsHistoricalParentSource
      ? (s.hostDetails?.[hostId]?.agents?.find((a) => a.agent_id === agentId)?.parent_agent_id ?? null)
      : null,
  );
  const historicalParentHostId = useOrbitSelector((s) =>
    needsHistoricalParentSource
      ? (s.hostDetails?.[hostId]?.agents?.find((a) => a.agent_id === agentId)?.parent_host_id ?? null)
      : null,
  );
  const historicalParentSource = useMemo<PromptSourceMeta | null>(
    () =>
      historicalParentAgentId
        ? { agentId: historicalParentAgentId, hostId: historicalParentHostId ?? hostId }
        : null,
    [historicalParentAgentId, historicalParentHostId, hostId],
  );
  const personalAgentReport =
    isPersonalAgentSession &&
    turn.agent_message?.item_type === "agent_message" &&
    !turn.agent_message.permission_context &&
    turn.agent_message.kind !== "heartbeat"
      ? turn.agent_message
      : null;
  // When multiple background workers finish in one coalesced batch, hoist leading
  // finish_notification segments so all source tags sit together above PA's prose
  // and all default-collapsed detail cards sit cleanly below PA's prose.
  const { personalAgentReports, displaySegments } = useMemo(() => {
    if (!personalAgentReport) {
      return {
        personalAgentReports: [] as Extract<TimelineItemView, { item_type: "agent_message" }>[],
        displaySegments: segments,
      };
    }
    const reports: Extract<TimelineItemView, { item_type: "agent_message" }>[] = [
      personalAgentReport,
    ];
    let splitIdx = 0;
    while (splitIdx < segments.length) {
      const seg = segments[splitIdx];
      if (
        seg.kind === "agent_message" &&
        seg.item.item_type === "agent_message" &&
        !seg.item.permission_context &&
        seg.item.kind !== "heartbeat"
      ) {
        reports.push(seg.item);
        splitIdx += 1;
      } else {
        break;
      }
    }
    return {
      personalAgentReports: reports,
      displaySegments: splitIdx > 0 ? segments.slice(splitIdx) : segments,
    };
  }, [personalAgentReport, segments]);
  const openingDirectBubble =
    !isPersonalAgentSession &&
    turn.agent_message?.item_type === "agent_message" &&
    isDirectAgentPromptBubble(turn.agent_message)
      ? turn.agent_message
      : null;
  const visibleOpeningMessage =
    !personalAgentReport &&
    !openingDirectBubble &&
    turn.agent_message?.item_type === "agent_message" &&
    !isTimelineAgentMessageHidden(turn.agent_message)
      ? turn.agent_message
      : null;
  const footer = getTurnFooter(turn, { isActive: isTurnActive, isLastTurn, canFork: !!onRequestFork, timing });

  if (isCompactionMarkerTurn(turn)) {
    const hasCompletedCompaction = turn.compactions.length > 0;
    return (
      <div className="min-w-0 px-[10px]">
        {isTurnActive && !hasCompletedCompaction ? (
          <CompactingStatusRow startedAtMs={liveStartedAtMs} />
        ) : (
          <CompactionMilestoneRow
            label={turn.compactions[0]?.label}
            isManual={turn.is_compaction_command}
          />
        )}
      </div>
    );
  }

  if (!promptItem && !personalAgentReport && !openingDirectBubble && !visibleOpeningMessage && !hasBody && !footer) {
    return null;
  }

  return (
    <div
      className={
        (hasBody && !hasBodyContent
          ? "group/turn min-w-0"
          : promptItem || openingDirectBubble
            ? "group/turn min-w-0 space-y-1"
            : "group/turn min-w-0 space-y-3") +
        (!isTurnActive ? " [contain:layout_style]" : "")
      }
    >
      {promptItem && (
        <UserPromptBubble
          item={promptItem}
          hostId={hostId}
          agentId={agentId}
          cwd={cwd}
          holdViewport={holdViewport}
          animateEntrance={animatePromptEntrance}
          source={historicalParentSource}
          onRequestRewind={onRequestRewind}
          onPreviewImage={onPreviewImage}
        />
      )}

      {personalAgentReports.length > 0 && (
        <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5">
          {personalAgentReports.map((rep) => (
            <PersonalAgentReportHeader
              key={rep.message_id || rep.seq}
              item={rep}
              fallbackHostId={hostId}
            />
          ))}
        </div>
      )}

      {openingDirectBubble && (
        <UserPromptBubble
          item={openingDirectBubble}
          hostId={hostId}
          agentId={agentId}
          cwd={cwd}
          holdViewport={holdViewport}
          animateEntrance={animatePromptEntrance}
          onPreviewImage={onPreviewImage}
        />
      )}

      {visibleOpeningMessage && (
        <AgentMessageCard
          hostId={hostId}
          agentId={agentId}
          item={visibleOpeningMessage}
          holdViewport={holdViewport}
          coalescedFinishStatus={finishStatuses.get(visibleOpeningMessage)}
          sessionSubagents={sessionSubagents}
        />
      )}

      {hasBody && (
        <div className="flow-root min-w-0 px-[10px]">
          <div className="min-w-0 space-y-3">
            {(() => {
              let nextWorkOrdinal = 0;
              const renderSegment = (seg: (typeof displaySegments)[number], idx: number) => {
                const isLastSeg = idx === displaySegments.length - 1;

                if (seg.kind === "compaction") {
                  return isTurnActive && isLiveCompactionSegment(seg) ? (
                    <CompactingStatusRow key={seg.key} startedAtMs={liveStartedAtMs} />
                  ) : (
                    <CompactionMilestoneRow
                      key={seg.key}
                      label={seg.label}
                      isManual={turn.is_compaction_command}
                    />
                  );
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
                  if (isPersonalAgentSession && !seg.item.permission_context && seg.item.kind !== "heartbeat") {
                    return (
                      <div key={seg.key} className="pt-1">
                        <div className="-mx-[10px]">
                          <PersonalAgentReportHeader item={seg.item} fallbackHostId={hostId} />
                        </div>
                        <PersonalAgentReportDetailCard
                          item={seg.item}
                          fallbackHostId={hostId}
                          cwd={cwd}
                        />
                      </div>
                    );
                  }
                  if (!isPersonalAgentSession && isDirectAgentPromptBubble(seg.item)) {
                    return (
                      <div key={seg.key} className="-mx-[10px]">
                        <UserPromptBubble
                          item={seg.item}
                          hostId={hostId}
                          agentId={agentId}
                          cwd={cwd}
                          holdViewport={holdViewport}
                          onPreviewImage={onPreviewImage}
                        />
                      </div>
                    );
                  }
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
                  const isGroupActive =
                    isTurnActive &&
                    !seg.has_following_content &&
                    (seg.has_running_step || (isLastSeg && !isTrailingFailedGroup));
                  const groupThoughtMs =
                    computeTotalThinkingDurationMs(seg.items, agentId, getThinkingDurationMs) ??
                    seg.thought_duration_ms ??
                    null;
                  const groupLiveStartMs = seg.started_at_ms ?? liveStartedAtMs;

                  const isTrailingLatest =
                    Boolean(isLastTurn) &&
                    isLastSeg &&
                    !seg.has_following_content &&
                    !needsTrailingActivity;

                  return (
                    <TurnActivityDisclosure
                      key={seg.key}
                      items={seg.items}
                      isWorkActive={isGroupActive}
                      isTrailingLatest={isTrailingLatest}
                      liveLabel={seg.summary.live_label}
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
                  const isChunkActive =
                    isTurnActive &&
                    !chunkHasFollowingContent &&
                    (chunkHasRunningStep || (isLastSeg && !chunkIsTrailingFailed));
                  const firstChunkTs = chunk.items[0]?.timestamp_ms ?? null;
                  const chunkLiveStartMs =
                    typeof firstChunkTs === "number" && firstChunkTs > 0
                      ? firstChunkTs
                      : (seg.started_at_ms ?? liveStartedAtMs);

                  const isChunkTrailingLatest =
                    Boolean(isLastTurn) &&
                    isLastSeg &&
                    !chunkHasFollowingContent &&
                    !needsTrailingActivity;

                  return (
                    <TurnActivityDisclosure
                      key={chunkKey}
                      items={chunk.items}
                      isWorkActive={isChunkActive}
                      isTrailingLatest={isChunkTrailingLatest}
                      liveLabel={chunkSummary.live_label}
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
              };

              const lastSeg = displaySegments[displaySegments.length - 1] ?? null;
              const workFold = resolveCompletedTurnWorkFold({
                segments: displaySegments,
                isTurnActive,
                turnHasUnrecoveredError: turn.has_unrecovered_error,
                locale,
                stepGroupSplits,
              });

              if (workFold.shouldWrap && lastSeg) {
                const turnWorkedDurationMs =
                  timing?.workDurationMs ??
                  turn.work_duration_ms ??
                  timing?.durationMs ??
                  turn.turn_duration_ms ??
                  (workFold.sumWorkDurationMs >= 1000 ? workFold.sumWorkDurationMs : null);
                return (
                  <>
                    <TurnWorkedDisclosure
                      durationMs={turnWorkedDurationMs}
                      fallbackLabel={workFold.fallbackLabel}
                      unrecoveredFailureCount={workFold.unrecoveredFailureCount}
                      defaultCollapsed={autoFoldCompleted}
                      autoOpenForError={
                        turn.has_unrecovered_error && workFold.unrecoveredFailureCount > 0
                      }
                      agentId={agentId}
                      turnIndex={turn.turn_index}
                      turnAnchorSeq={turn.anchor_seq}
                      holdViewport={holdViewport}
                    >
                      {() => (
                        <StepTimelineRail
                          items={workFold.items}
                          hostId={hostId}
                          cwd={cwd}
                          holdViewport={holdViewport}
                          isLiveActive={false}
                          autoExpandErrors={
                            turn.has_unrecovered_error && workFold.unrecoveredFailureCount > 0
                          }
                          agentId={agentId}
                          turnIndex={turn.turn_index}
                          turnAnchorSeq={turn.anchor_seq}
                          workOrdinal={-1}
                          thinkingSignature={_thinkingSignature}
                          getThinkingDurationMs={getThinkingDurationMs}
                          sessionSubagents={sessionSubagents}
                          onOpenSubagent={onOpenSubagent}
                        />
                      )}
                    </TurnWorkedDisclosure>
                    {renderSegment(lastSeg, displaySegments.length - 1)}
                  </>
                );
              }

              return displaySegments.map((seg, idx) => renderSegment(seg, idx));
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
          {personalAgentReports.length > 0 && (
            <div className="flex flex-col gap-1">
              {personalAgentReports.map((rep) => (
                <PersonalAgentReportDetailCard
                  key={rep.message_id || rep.seq}
                  item={rep}
                  fallbackHostId={hostId}
                  cwd={cwd}
                  showSourcePrefix={personalAgentReports.length > 1}
                />
              ))}
            </div>
          )}
          {footer && (
            <TurnFooter
              {...footer}
              isLastTurn={isLastTurn}
              hasChangesBar={!isTurnActive && turn.turn_diffs.length > 0}
              canForkWorktree={canForkWorktree}
              onRequestFork={onRequestFork}
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
  prev.isPersonalAgentSession === next.isPersonalAgentSession &&
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
  prev.parentMessageLabel === next.parentMessageLabel,
);
