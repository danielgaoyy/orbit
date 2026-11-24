import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowLeftRight,
  ArrowRight,
  ArrowUp,
  Blocks,
  ChevronRight,
  ListChecks,
  RotateCw,
  Scissors,
  Sparkles,
  Timer,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { DiffStat } from "@/components/ui/diff-stat";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ScrollArea } from "@/components/ui/scroll-area";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace";
import { useI18n } from "@/lib/i18n";
import { sharedDisclosureStateStore } from "@/lib/disclosure-state";
import type { TimelineItemView, WorkSummaryResultView } from "@/lib/orbit-ipc";
import {
  type ResolvedSubagentItem,
  extractSubagentMessageCall,
  resolveDelegationDispatches,
} from "@/lib/subagents";
import {
  type SubagentCharacterSpec,
  resolveSubagentCharacter,
} from "@/lib/subagent-avatar";
import { formatDurationCompact, formatThoughtSummaryLabel } from "@/lib/timeline-coalesce";
import {
  type ToolCallTimelineItem,
  extractStateChangeMilestoneCall,
  summarizeWorkChunk,
} from "@/lib/turn-delegation-split";
import { orbitActions } from "@/state/orbit-store";
import {
  EMPTY_SUBAGENTS,
  type InlineArtifactRef,
  type ThinkingDurationLookupFn,
  type ViewportAnchorHoldFn,
  formatJsonPreview,
  stableStepItemKey,
} from "./stage-helpers";
import { openSubagent, SubagentPanelAction } from "./subagent-crew-dock";
import {
  LazyMarkdownMessageBody,
  PeerMessageRow,
  SubagentDispatchRow,
} from "./subagents-track";
import { StageCopyButton } from "./stage-prompt-controls";
import {
  InlineLiveElapsed,
  LiveStallInterruptButton,
  StepItemRow,
} from "./stage-tool-renderers";

type SubagentMaybeWithAvatar = ResolvedSubagentItem & {
  readonly character?: SubagentCharacterSpec;
};

/** Vertical step connector rail for expanded turn activity steps. */
export const StepTimelineRail = memo(function StepTimelineRail({
  items,
  hostId,
  cwd,
  holdViewport,
  isLiveActive = false,
  autoExpandErrors = false,
  agentId,
  thinkingSignature: _thinkingSignature,
  getThinkingDurationMs,
  sessionSubagents,
  onOpenSubagent,
}: {
  items: TimelineItemView[];
  hostId?: string;
  cwd: string;
  holdViewport: ViewportAnchorHoldFn;
  isLiveActive?: boolean;
  autoExpandErrors?: boolean;
  agentId: string;
  thinkingSignature: string;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  sessionSubagents?: readonly SubagentMaybeWithAvatar[];
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
}) {
  if (items.length === 0) return null;

  const lastSeq = items[items.length - 1]?.seq;

  return (
    <Trace className="gap-1">
      {items.map((sub, i) => {
        const isLastInLiveGroup = isLiveActive && sub.seq === lastSeq;
        const thinkingKey =
          sub.item_type === "assistant_thinking"
            ? `${agentId}::think::${sub.item_id || sub.seq}`
            : null;
        const thoughtDurationMs = thinkingKey ? getThinkingDurationMs(thinkingKey) : null;

        return (
          <StepItemRow
            key={stableStepItemKey(sub, i)}
            item={sub}
            hostId={hostId}
            agentId={agentId}
            cwd={cwd}
            holdViewport={holdViewport}
            isActivelyThinking={
              sub.item_type === "assistant_thinking" && isLastInLiveGroup
            }
            thoughtDurationMs={thoughtDurationMs}
            autoExpandError={autoExpandErrors}
            sessionSubagents={sessionSubagents}
            onOpenSubagent={onOpenSubagent}
          />
        );
      })}
    </Trace>
  );
});

type DelegationMilestoneEntry =
  | {
      kind: "subagent_dispatch";
      key: string;
      subagent: SubagentMaybeWithAvatar;
      character: SubagentCharacterSpec;
      linked: boolean;
      startedAtMs: number | null;
    }
  | {
      kind: "inter_agent_message";
      key: string;
      label: string;
      preview: string;
      detailText: string;
      detailIsMono: boolean;
      status: "running" | "completed" | "failed" | "denied";
      subagent?: SubagentMaybeWithAvatar;
      character?: SubagentCharacterSpec;
    }
  | {
      kind: "state_change";
      key: string;
      milestoneKind: "timer" | "handoff";
      label: string;
      detailText: string;
      status: "running" | "completed" | "failed" | "denied";
    };

/**
 * First-class chronological milestone row rendered outside folded step groups:
 * 1. Subagent dispatches open the child directly from the inline status row.
 * 2. Inter-agent messages (`send_message`, `message_peer`, `send_agent_prompt`) — compact expandable
 *    left-aligned rows matching `AgentMessageCard` with clean left-rail body (never card-inside-card).
 * 3. Session state milestones (Timers, Worktree Handoffs) — inline disclosures.
 */
export const DelegationMilestoneRow = memo(function DelegationMilestoneRow({
  items,
  hostId,
  agentId,
  sessionSubagents,
  dispatches,
  parentMessageLabel,
  holdViewport,
}: {
  items: readonly ToolCallTimelineItem[];
  hostId?: string;
  agentId?: string;
  sessionSubagents?: readonly SubagentMaybeWithAvatar[];
  dispatches?: ReturnType<typeof resolveDelegationDispatches>;
  parentMessageLabel?: string | null;
  holdViewport?: ViewportAnchorHoldFn;
}) {
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  const { t, locale } = useI18n();
  const subagentFallback = t("approval.subagent");
  const sentMessageLabel = parentMessageLabel || t("stage.sentMessage");
  const entries = useMemo(() => {
    const out: DelegationMilestoneEntry[] = [];

    const matches = dispatches ?? resolveDelegationDispatches(items, sessionSubagents ?? EMPTY_SUBAGENTS, hostId);
    for (const it of items) {
      const resolved = matches.get(it);
      if (resolved) {
        const enriched = resolved.subagent as SubagentMaybeWithAvatar;
        const character =
          enriched.character ??
          resolveSubagentCharacter(
            enriched.childAgentId || enriched.providerSubagentId || enriched.callId || enriched.key,
          );

        if (resolved.action === "message") {
          const msgCall = extractSubagentMessageCall(it);
          const rawArgs = it.detail.kind === "generic" ? it.detail.arguments_json.trim() : "";
          const detailText =
            msgCall?.message ||
            (rawArgs && rawArgs !== "{}" && rawArgs !== "null" ? formatJsonPreview(rawArgs) : "");
          const preview = detailText.replace(/\s+/g, " ").trim();
          out.push({
            kind: "inter_agent_message",
            key: `msg:${it.call_id || it.seq}`,
            label: `→ ${enriched.title === "Subagent" ? subagentFallback : enriched.title}`,
            preview,
            detailText,
            detailIsMono: !msgCall?.message && detailText.length > 0,
            status: it.status,
            subagent: enriched,
            character,
          });
          continue;
        }

        out.push({
          kind: "subagent_dispatch",
          key: `dispatch:${it.call_id || it.seq}`,
          subagent: enriched,
          character,
          linked: resolved.linked,
          startedAtMs: enriched.activeStartedAtMs ?? it.timestamp_ms ?? (enriched.createdAtMs || null),
        });
        continue;
      }

      const msgCall = extractSubagentMessageCall(it);
      if (msgCall) {
        const isNamedHandle =
          msgCall.recipient.length > 0 &&
          msgCall.recipient.length <= 24 &&
          !/^[0-9a-f-]{16,}$/i.test(msgCall.recipient) &&
          !msgCall.recipient.startsWith("ag_");
        const label = isNamedHandle
          ? `→ @${msgCall.recipient.replace(/^@+/, "")}`
          : sentMessageLabel;
        const isFailed = msgCall.status === "failed" || msgCall.status === "denied";
        const rawArgs = it.detail.kind === "generic" ? it.detail.arguments_json.trim() : "";
        const hasFallbackArgs =
          isFailed && rawArgs.length > 0 && rawArgs !== "{}" && rawArgs !== "null";
        const detailText =
          msgCall.message || (hasFallbackArgs ? formatJsonPreview(rawArgs) : "");
        out.push({
          kind: "inter_agent_message",
          key: `msg:${it.call_id || it.seq}`,
          label,
          preview: detailText.replace(/\s+/g, " ").trim(),
          detailText,
          detailIsMono: !msgCall.message && hasFallbackArgs,
          status: msgCall.status,
        });
        continue;
      }

      const stateMilestone = extractStateChangeMilestoneCall(it, locale);
      if (stateMilestone) {
        out.push({
          kind: "state_change",
          key: `state:${stateMilestone.callId}`,
          milestoneKind: stateMilestone.kind,
          label: stateMilestone.label,
          detailText: stateMilestone.preview,
          status: stateMilestone.status,
        });
        continue;
      }
    }

    return out;
  }, [items, sessionSubagents, hostId, sentMessageLabel, dispatches, locale, subagentFallback]);

  if (entries.length === 0) return null;

  const openSubagentRecord = (subagent: ResolvedSubagentItem) =>
    openSubagent(subagent.parentAgentId || agentId, subagent);

  // Map each direct child agent ID to its recursive descendants (depth > 1) so milestone rows
  // surface multi-level Parent -> Child -> Grandchild lineage cleanly inline.
  const nestedByRootChildId = useMemo(() => {
    const map = new Map<string, SubagentMaybeWithAvatar[]>();
    if (!sessionSubagents || sessionSubagents.length === 0) return map;
    for (const sa of sessionSubagents) {
      const depth = sa.depth ?? 1;
      if (depth <= 1) continue;
      // Find the top-level child ancestor in lineage (index 1 is depth 1 after index 0 root)
      const rootChildId =
        sa.lineage && sa.lineage.length >= 3
          ? sa.lineage[1]?.agent_id
          : sa.parentAgentId;
      if (!rootChildId) continue;
      const bucket = map.get(rootChildId);
      if (bucket) bucket.push(sa);
      else map.set(rootChildId, [sa]);
    }
    return map;
  }, [sessionSubagents]);

  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      {entries.map((entry) => {
        if (entry.kind === "subagent_dispatch") {
          const { subagent, character } = entry;
          const name = subagent.title === "Subagent" ? subagentFallback : subagent.title;
          const effectiveStatus = subagent.status;
          const starting = effectiveStatus === "running" && !entry.linked;
          const statusKey = {
            running: starting ? "stage.subagentStarting" : "overview.subagentWorking",
            awaiting_permission: "overview.subagentApprovalRequired",
            failed: "overview.subagentFailed",
            completed: "stage.subagentDone",
            canceled: "overview.subagentStopped",
          } as const;
          const isFailed = effectiveStatus === "failed";
          const isAwaiting = effectiveStatus === "awaiting_permission";
          const nestedDescendants = subagent.childAgentId
            ? (nestedByRootChildId.get(subagent.childAgentId) ?? [])
            : [];

          return (
            <div key={entry.key} className="flex min-w-0 flex-col gap-0.5">
              <SubagentDispatchRow
                title={subagent.promptPreview || t("stage.openTargetInPanel", { target: name })}
                onClick={() => openSubagentRecord(subagent)}
                tone={isFailed ? "danger" : isAwaiting ? "warning" : "normal"}
                name={name}
                statusLabel={t(statusKey[effectiveStatus])}
                avatar={
                  <SubagentAvatar
                    character={character}
                    status={effectiveStatus}
                    size="sm"
                    showStatusBadge={
                      effectiveStatus === "completed" || isFailed || effectiveStatus === "canceled"
                    }
                  />
                }
                trailing={
                  (effectiveStatus === "running" || isAwaiting) && entry.startedAtMs != null ? (
                    <InlineLiveElapsed startedAtMs={entry.startedAtMs} stallLabel={null} />
                  ) : undefined
                }
              />
              {nestedDescendants.length > 0 && (
                <div className="ml-3.5 flex min-w-0 flex-col gap-0.5 border-l border-border-subtle/80 pl-2.5">
                  {nestedDescendants.map((desc) => {
                    const descName = desc.title === "Subagent" ? subagentFallback : desc.title;
                    const descStatus = desc.status;
                    const descFailed = descStatus === "failed";
                    const descAwaiting = descStatus === "awaiting_permission";
                    const descChar =
                      desc.character ??
                      resolveSubagentCharacter(
                        desc.childAgentId || desc.providerSubagentId || desc.callId || desc.key,
                      );
                    const extraIndent = Math.min(Math.max(0, (desc.depth ?? 2) - 2), 4) * 12;
                    const descStatusKey = {
                      running: "overview.subagentWorking",
                      awaiting_permission: "overview.subagentApprovalRequired",
                      failed: "overview.subagentFailed",
                      completed: "stage.subagentDone",
                      canceled: "overview.subagentStopped",
                    } as const;
                    const descTooltip = [
                      desc.lineage && desc.lineage.length > 1
                        ? desc.lineage.map((step) => step.title).join(" → ")
                        : null,
                      desc.promptPreview || t("stage.openTargetInPanel", { target: descName }),
                    ]
                      .filter(Boolean)
                      .join("\n");
                    return (
                      <div
                        key={desc.key}
                        style={extraIndent > 0 ? { paddingLeft: `${extraIndent}px` } : undefined}
                        className="min-w-0"
                      >
                        <SubagentDispatchRow
                          title={descTooltip}
                          onClick={() => openSubagentRecord(desc)}
                          tone={descFailed ? "danger" : descAwaiting ? "warning" : "normal"}
                          name={descName}
                          statusLabel={t(descStatusKey[descStatus])}
                          avatar={
                            <SubagentAvatar
                              character={descChar}
                              status={descStatus}
                              size="xs"
                              showStatusBadge={
                                descStatus === "completed" || descFailed || descStatus === "canceled"
                              }
                            />
                          }
                        />
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          );
        }

        const isOpen = expandedKey === entry.key;
        const changeOpen = (open: boolean) => setExpandedKey(open ? entry.key : null);
        const isFailed = entry.status === "failed" || entry.status === "denied";
        if (entry.kind === "state_change") {
          const Icon = entry.milestoneKind === "timer" ? Timer : ArrowLeftRight;
          const line = (
            <TraceLine status={isFailed ? "error" : entry.status === "running" ? "running" : undefined} className="min-h-[28px]">
              <TraceIcon>{entry.status === "running" ? <OrbitSpinner /> : <Icon />}</TraceIcon>
              <TraceTitle>{entry.label}</TraceTitle>
            </TraceLine>
          );
          return (
            <Trace key={entry.key}>
              {entry.detailText ? (
                <TraceCollapsible open={isOpen} onOpenChange={changeOpen}>
                  <TraceCollapsibleTrigger>{line}</TraceCollapsibleTrigger>
                  <TraceCollapsibleContent className="flex items-start gap-2">
                    <ScrollArea className="min-w-0 flex-1" viewportClassName="max-h-56 overscroll-contain">
                      <p className="selectable-text text-[13px] leading-[1.5] whitespace-pre-wrap break-words text-ink-secondary">
                        {entry.detailText}
                      </p>
                    </ScrollArea>
                    <StageCopyButton text={entry.detailText} className="shrink-0" />
                  </TraceCollapsibleContent>
                </TraceCollapsible>
              ) : line}
            </Trace>
          );
        }

        const recipient = entry.subagent;
        return (
          <PeerMessageRow
            key={entry.key}
            open={isOpen}
            onOpenChange={changeOpen}
            holdViewport={holdViewport}
            label={entry.label}
            preview={entry.preview}
            failed={isFailed}
            statusLabel={isFailed ? t("stage.failed") : undefined}
            avatar={entry.character ? (
              <SubagentAvatar
                character={entry.character}
                status={recipient?.status ?? (entry.status === "running" ? "running" : "completed")}
                size="sm"
              />
            ) : entry.status === "running" ? (
              <OrbitSpinner size={14} />
            ) : isFailed ? (
              <AlertTriangle className="size-3.5 shrink-0 text-signal-danger" />
            ) : (
              <ArrowRight className="size-3.5 shrink-0 text-ink-tertiary" />
            )}
            action={recipient && (
              <SubagentPanelAction
                name={recipient.title === "Subagent" ? subagentFallback : recipient.title}
                onClick={() => openSubagentRecord(recipient)}
              />
            )}
          >
            {entry.detailText && (
              <ScrollArea viewportClassName="max-h-64 overscroll-contain">
                {entry.detailIsMono ? (
                  <pre className="selectable-text text-ink-secondary whitespace-pre-wrap break-words">{entry.detailText}</pre>
                ) : (
                  <LazyMarkdownMessageBody text={entry.detailText} />
                )}
              </ScrollArea>
            )}
          </PeerMessageRow>
        );
      })}
    </div>
  );
});

export const TurnActivityDisclosure = memo(function TurnActivityDisclosure({
  items,
  isWorkActive,
  liveLabel,
  liveDetail,
  settledLabel,
  hasProjectedSummary = false,
  thoughtDurationMs,
  summaryCounts,
  labelIncludesFailure,
  defaultCollapsed,
  autoOpenForError,
  liveStartedAtMs,
  latestActivityMs,
  latestVisibleProgressMs,
  queuedNotificationCount = 0,
  onInterruptAndContinue = null,
  isNetworkOffline = false,
  isBackgroundWait = false,
  settledDurationMs,
  hostId,
  cwd,
  agentId,
  turnIndex,
  turnAnchorSeq,
  workOrdinal,
  thinkingSignature,
  getThinkingDurationMs,
  holdViewport,
  sessionSubagents,
  onOpenSubagent,
}: {
  items: TimelineItemView[];
  isWorkActive: boolean;
  liveLabel: string;
  liveDetail: string | null;
  settledLabel: string;
  hasProjectedSummary?: boolean;
  thoughtDurationMs?: number | null;
  summaryCounts: WorkSummaryResultView["counts"];
  labelIncludesFailure: boolean;
  defaultCollapsed: boolean;
  autoOpenForError: boolean;
  liveStartedAtMs: number | null;
  latestActivityMs?: number | null;
  latestVisibleProgressMs?: number | null;
  queuedNotificationCount?: number;
  onInterruptAndContinue?: (() => void) | null;
  isNetworkOffline?: boolean;
  isBackgroundWait?: boolean;
  settledDurationMs: number | null;
  hostId?: string;
  cwd: string;
  agentId: string;
  turnIndex?: number;
  turnAnchorSeq?: number;
  workOrdinal?: number;
  thinkingSignature: string;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  holdViewport: ViewportAnchorHoldFn;
  sessionSubagents?: readonly SubagentMaybeWithAvatar[];
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
}) {
  const { t, locale } = useI18n();
  // Core projects English display labels. Reuse the chunk formatter for other locales
  // while keeping core counts, activity and disclosure decisions authoritative.
  const localizedSummary = useMemo(
    () => hasProjectedSummary && locale !== "en" ? summarizeWorkChunk(items, thoughtDurationMs, locale) : null,
    [hasProjectedSummary, items, thoughtDurationMs, locale],
  );
  const activeLabel = localizedSummary?.live_label ?? liveLabel;
  const thoughtOnly = summaryCounts.action_steps === 0 && summaryCounts.thought_count > 0;
  const finishedLabel = thoughtOnly
    ? formatThoughtSummaryLabel(thoughtDurationMs, summaryCounts.thought_count, locale)
    : localizedSummary?.label ?? settledLabel;
  const canPersist =
    typeof turnIndex === "number" &&
    typeof turnAnchorSeq === "number" &&
    typeof workOrdinal === "number" &&
    Boolean(agentId);

  const [userExpanded, setUserExpanded] = useState<boolean | null>(() =>
    canPersist
      ? sharedDisclosureStateStore.get(agentId, turnIndex, workOrdinal, turnAnchorSeq)
      : null,
  );
  const foldBtnRef = useRef<HTMLButtonElement | null>(null);
  const fallbackStartRef = useRef<number>(Date.now());
  const effectiveLiveStartedAtMs = liveStartedAtMs ?? fallbackStartRef.current;

  useEffect(() => {
    if (!canPersist) return;
    const saved = sharedDisclosureStateStore.get(agentId, turnIndex, workOrdinal, turnAnchorSeq);
    setUserExpanded((prev) => (prev === saved ? prev : saved));
  }, [canPersist, agentId, turnIndex, workOrdinal, turnAnchorSeq]);

  const defaultOpen = !defaultCollapsed || autoOpenForError;
  const isExpanded = userExpanded !== null ? userExpanded : defaultOpen;

  if (items.length === 0 && !isWorkActive) return null;

  const handleOpenChange = (open: boolean) => {
    holdViewport(
      foldBtnRef.current,
      () => {
        setUserExpanded(open);
        if (canPersist) {
          sharedDisclosureStateStore.set(agentId, turnIndex, workOrdinal, turnAnchorSeq, open);
        }
      },
      "top",
      { intent: open ? "expand" : "collapse" },
    );
  };

  const showFailureBadge =
    !isWorkActive && summaryCounts.unrecovered_failure_count > 0 && !labelIncludesFailure;
  const showDiffNumbers =
    isWorkActive && items.length > 0 && (summaryCounts.additions > 0 || summaryCounts.deletions > 0);
  const showSettledDuration =
    !isWorkActive &&
    settledDurationMs !== null &&
    settledDurationMs >= 1000 &&
    !(thoughtOnly && Math.round((thoughtDurationMs ?? 0) / 1000) >= 1);

  const hasFailure = !isWorkActive && (labelIncludesFailure || showFailureBadge);
  const SummaryIcon = hasFailure ? AlertTriangle : thoughtOnly || items.length === 0 ? Sparkles : ListChecks;
  const activeStallLabel = isBackgroundWait
    ? items.length > 0
      ? t("stage.runningInBackground")
      : null
    : undefined;
  const line = (
    <TraceLine status={isWorkActive ? "running" : hasFailure ? "error" : undefined} className="text-[13px]">
      <TraceIcon><SummaryIcon /></TraceIcon>
      <TraceTitle>
        {isWorkActive ? activeLabel || t("stage.working") : finishedLabel}
        {isWorkActive && liveDetail && items.length > 0 && <> {liveDetail}</>}
      </TraceTitle>
      {isWorkActive && items.length >= 2 && (
        <TraceDescription>· {t("stage.stepCount", { count: items.length })}</TraceDescription>
      )}
      {isWorkActive && (
        <InlineLiveElapsed
          startedAtMs={effectiveLiveStartedAtMs}
          lastActivityMs={latestActivityMs}
          lastVisibleProgressMs={latestVisibleProgressMs}
          isNetworkOffline={isNetworkOffline}
          stallLabel={activeStallLabel}
          queuedNotificationCount={queuedNotificationCount}
        />
      )}
      {showDiffNumbers && (
        <TraceDescription>
          <DiffStat size="xs" additions={summaryCounts.additions} deletions={summaryCounts.deletions} />
        </TraceDescription>
      )}
      {showFailureBadge && (
        <TraceDescription className="text-destructive">
          · {t("stage.failedCount", { count: summaryCounts.unrecovered_failure_count })}
        </TraceDescription>
      )}
      {showSettledDuration && (
        <TraceDescription>· {formatDurationCompact(settledDurationMs, locale)}</TraceDescription>
      )}
    </TraceLine>
  );

  const interruptButton = isWorkActive ? (
    <LiveStallInterruptButton
      startedAtMs={effectiveLiveStartedAtMs}
      lastActivityMs={latestActivityMs}
      lastVisibleProgressMs={latestVisibleProgressMs}
      isNetworkOffline={isNetworkOffline}
      stallLabel={activeStallLabel}
      queuedNotificationCount={queuedNotificationCount}
      onInterruptAndContinue={onInterruptAndContinue}
    />
  ) : null;

  if (items.length === 0) {
    return (
      <Trace>
        <div className="flex flex-wrap items-center gap-1.5">
          {line}
          {interruptButton}
        </div>
      </Trace>
    );
  }

  return (
    <Trace>
      <TraceCollapsible open={isExpanded} onOpenChange={handleOpenChange}>
        <div className="flex flex-wrap items-center gap-1.5">
          <TraceCollapsibleTrigger ref={foldBtnRef}>{line}</TraceCollapsibleTrigger>
          {interruptButton}
        </div>
        <TraceCollapsibleContent>
          <StepTimelineRail
            items={items}
            hostId={hostId}
            cwd={cwd}
            holdViewport={holdViewport}
            isLiveActive={isWorkActive}
            autoExpandErrors={
              !isWorkActive && (summaryCounts.failed_count > 0 || summaryCounts.denied_count > 0)
            }
            agentId={agentId}
            thinkingSignature={thinkingSignature}
            getThinkingDurationMs={getThinkingDurationMs}
            sessionSubagents={sessionSubagents}
            onOpenSubagent={onOpenSubagent}
          />
          {items.length >= 10 && (
            <Button
              type="button"
              variant="ghost"
              size="xs"
              className="mt-1"
              onClick={() => {
                foldBtnRef.current?.focus({ preventScroll: true });
                foldBtnRef.current?.scrollIntoView({ block: "nearest" });
                handleOpenChange(false);
              }}
            >
              <ArrowUp data-icon="inline-start" />
              {t("stage.collapseSteps", { count: items.length })}
            </Button>
          )}
        </TraceCollapsibleContent>
      </TraceCollapsible>
    </Trace>
  );
});

export const CompactionMilestoneRow = memo(function CompactionMilestoneRow({
  label,
}: {
  label?: string;
}) {
  const { t } = useI18n();
  return (
    <Marker variant="separator" className="py-1">
      <MarkerIcon><Scissors /></MarkerIcon>
      <MarkerContent className="group-data-[variant=separator]/marker:shrink">
        {!label || label === "Context compacted" ? t("stage.compacted") : label}
      </MarkerContent>
    </Marker>
  );
});

export const WakeupEventRow = memo(function WakeupEventRow({ label }: { label?: string }) {
  const { t } = useI18n();
  return (
    <Marker className="py-1">
      <MarkerIcon><RotateCw /></MarkerIcon>
      <MarkerContent>
        {!label || label === "Resumed from background task" ? t("stage.resumedFromBackground") : label}
      </MarkerContent>
    </Marker>
  );
});

export const InlineArtifactCard = memo(function InlineArtifactCard({
  hostId,
  artifact,
}: {
  hostId: string;
  artifact: InlineArtifactRef;
}) {
  const { t } = useI18n();
  return (
    <button
      type="button"
      onClick={() =>
        orbitActions.openArtifactsTab({
          hostId,
          artifactId: artifact.artifactId,
        })
      }
      className="border-border-subtle bg-surface-raised/70 hover:bg-surface-recessed hover:border-border-default group inline-flex h-[30px] max-w-full cursor-pointer items-center gap-2 rounded-lg border px-2.5 text-left text-[13px] leading-[18px] transition-colors"
    >
      <Blocks className="text-ink-tertiary group-hover:text-ink-primary size-[13px] shrink-0 stroke-[1.75]" />
      <span className="text-ink-tertiary shrink-0 text-[11px]">{t("stage.artifact")}</span>
      <span className="text-ink-muted shrink-0">·</span>
      <span className="text-ink-primary truncate font-medium">{artifact.title}</span>
      {artifact.version != null && (
        <>
          <span className="text-ink-muted shrink-0">·</span>
          <span className="text-ink-tertiary shrink-0 font-mono text-[11px]">
            v{artifact.version}
          </span>
        </>
      )}
      <ChevronRight className="text-ink-muted group-hover:text-ink-primary size-[13px] shrink-0 transition-transform group-hover:translate-x-0.5" />
    </button>
  );
});
