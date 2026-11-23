import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowRight,
  ArrowUpRight,
  Blocks,
  Check,
  ChevronRight,
  Copy,
  RotateCw,
  Scissors,
} from "lucide-react";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ScrollArea } from "@/components/ui/scroll-area";
import { sharedDisclosureStateStore } from "@/lib/disclosure-state";
import type { TimelineItemView, WorkSummaryResultView } from "@/lib/orbit-ipc";
import {
  type ResolvedSubagentItem,
  extractSubagentMessageCall,
  resolveToolCallSubagent,
} from "@/lib/subagents";
import { formatDurationCompact } from "@/lib/timeline-coalesce";
import {
  type ToolCallTimelineItem,
  extractFallbackDispatchCall,
} from "@/lib/turn-delegation-split";
import { cn } from "@/lib/utils";
import { orbitActions } from "@/state/orbit-store";
import {
  EMPTY_SUBAGENTS,
  type InlineArtifactRef,
  type ThinkingDurationLookupFn,
  type ViewportAnchorHoldFn,
  formatJsonPreview,
  stableStepItemKey,
} from "./stage-helpers";
import { InlineLiveElapsed, StepItemRow } from "./stage-tool-renderers";

/** Vertical step connector rail for expanded turn activity steps. */
export const StepTimelineRail = memo(function StepTimelineRail({
  items,
  hostId,
  cwd,
  holdViewport,
  indented = false,
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
  indented?: boolean;
  isLiveActive?: boolean;
  autoExpandErrors?: boolean;
  agentId: string;
  thinkingSignature: string;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  sessionSubagents?: readonly ResolvedSubagentItem[];
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
}) {
  if (items.length === 0) return null;

  const lastSeq = items[items.length - 1]?.seq;

  return (
    <div
      className={cn(
        "space-y-0.5",
        indented && "border-border mt-1 ml-[7px] border-l pl-3.5",
      )}
    >
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
    </div>
  );
});

type DelegationMilestoneEntry =
  | {
      kind: "subagent";
      key: string;
      subagent: ResolvedSubagentItem;
      action: "dispatch" | "message";
      callStatus: "running" | "completed" | "failed" | "denied";
    }
  | {
      kind: "expandable";
      key: string;
      variant: "message" | "dispatch";
      label: string;
      detailText: string;
      detailIsMono: boolean;
      status: "running" | "completed" | "failed" | "denied";
    };

/**
 * First-class chronological milestone row for subagent dispatches (`Ran an agent`) and
 * inter-agent messages (`Sent to parent` / `Sent to @handle`), rendered at their exact
 * real-time timeline position rather than duplicated inside folded work disclosures.
 */
export const DelegationMilestoneRow = memo(function DelegationMilestoneRow({
  items,
  hostId,
  agentId,
  sessionSubagents,
  onOpenSubagent,
  parentMessageLabel,
}: {
  items: readonly ToolCallTimelineItem[];
  hostId?: string;
  agentId?: string;
  sessionSubagents?: readonly ResolvedSubagentItem[];
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
  parentMessageLabel?: string | null;
}) {
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  const entries = useMemo(() => {
    const out: DelegationMilestoneEntry[] = [];
    const subagentIndexByKey = new Map<string, number>();

    for (const it of items) {
      const resolved = resolveToolCallSubagent(it, sessionSubagents ?? EMPTY_SUBAGENTS, hostId);
      if (resolved) {
        const existingIdx = subagentIndexByKey.get(resolved.subagent.key);
        if (existingIdx !== undefined) {
          const prev = out[existingIdx];
          if (prev?.kind === "subagent") {
            const nextAction =
              prev.action === "dispatch" || resolved.action === "dispatch"
                ? "dispatch"
                : "message";
            out[existingIdx] = {
              ...prev,
              subagent: resolved.subagent,
              action: nextAction,
              callStatus: it.status,
            };
          }
        } else {
          subagentIndexByKey.set(resolved.subagent.key, out.length);
          out.push({
            kind: "subagent",
            key: resolved.subagent.key,
            subagent: resolved.subagent,
            action: resolved.action,
            callStatus: it.status,
          });
        }
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
          ? `Sent to @${msgCall.recipient.replace(/^@+/, "")}`
          : parentMessageLabel || "Send message";
        const isFailed = msgCall.status === "failed" || msgCall.status === "denied";
        const rawArgs =
          it.detail.kind === "generic" ? it.detail.arguments_json.trim() : "";
        const hasFallbackArgs =
          isFailed && rawArgs.length > 0 && rawArgs !== "{}" && rawArgs !== "null";
        const detailText =
          msgCall.message || (hasFallbackArgs ? formatJsonPreview(rawArgs) : "");
        out.push({
          kind: "expandable",
          key: `msg:${it.call_id || it.seq}`,
          variant: "message",
          label,
          detailText,
          detailIsMono: !msgCall.message && hasFallbackArgs,
          status: msgCall.status,
        });
        continue;
      }

      const fallbackDispatch = extractFallbackDispatchCall(it);
      if (fallbackDispatch) {
        out.push({
          kind: "expandable",
          key: `dispatch:${fallbackDispatch.callId}`,
          variant: "dispatch",
          label: fallbackDispatch.label,
          detailText: fallbackDispatch.preview,
          detailIsMono: fallbackDispatch.preview.startsWith("{"),
          status: fallbackDispatch.status,
        });
      }
    }

    return out;
  }, [items, sessionSubagents, hostId, parentMessageLabel]);

  if (entries.length === 0) return null;

  const activeExpandable = expandedKey
    ? (entries.find(
        (e): e is Extract<DelegationMilestoneEntry, { kind: "expandable" }> =>
          e.kind === "expandable" && e.key === expandedKey,
      ) ?? null)
    : null;

  return (
    <div className="space-y-1.5">
      <div className="flex flex-wrap items-center gap-1.5">
        {entries.map((entry) => {
          if (entry.kind === "subagent") {
            const { subagent, action, callStatus } = entry;
            const isFailed =
              callStatus === "failed" ||
              callStatus === "denied" ||
              subagent.status === "failed";
            return (
              <button
                key={entry.key}
                type="button"
                title={subagent.promptPreview || `Open ${subagent.title}`}
                onClick={() => {
                  if (onOpenSubagent) {
                    onOpenSubagent(subagent);
                  } else {
                    orbitActions.openSubagentTab({
                      callId: subagent.callId,
                      role: subagent.title,
                      promptPreview: subagent.promptPreview,
                      childAgentId: subagent.childAgentId,
                      childHostId: subagent.childHostId || undefined,
                      providerSubagentId: subagent.providerSubagentId,
                      parentAgentId: agentId || undefined,
                    });
                  }
                }}
                className={cn(
                  "group/sachip text-foreground/82 hover:text-foreground inline-flex h-[24px] max-w-[260px] cursor-pointer items-center gap-1.5 rounded-full border px-2.5 text-[12px] leading-none transition-colors",
                  isFailed
                    ? "border-rose-500/30 bg-rose-500/[0.04] hover:border-rose-500/45 hover:bg-rose-500/[0.08]"
                    : "border-black/[0.07] bg-black/[0.02] hover:border-black/[0.14] hover:bg-black/[0.045] dark:border-white/[0.09] dark:bg-white/[0.03] dark:hover:border-white/[0.16] dark:hover:bg-white/[0.06]",
                )}
              >
                <span className="flex size-3 shrink-0 items-center justify-center">
                  {action === "message" ? (
                    callStatus === "running" ? (
                      <OrbitSpinner size={11} className="text-foreground/70" />
                    ) : callStatus === "failed" || callStatus === "denied" ? (
                      <AlertTriangle className="size-3 text-rose-500" />
                    ) : (
                      <ArrowRight className="text-muted-foreground/85 size-3 stroke-[1.9]" />
                    )
                  ) : subagent.status === "awaiting_permission" ? (
                    <span className="size-[6px] rounded-full bg-amber-500 shadow-[0_0_0_2px_rgba(245,158,11,0.18)]" />
                  ) : subagent.status === "running" || callStatus === "running" ? (
                    <OrbitSpinner size={11} className="text-foreground/70" />
                  ) : isFailed ? (
                    <AlertTriangle className="size-3 text-rose-500" />
                  ) : subagent.status === "canceled" ? (
                    <span className="bg-muted-foreground/45 size-[6px] rounded-full" />
                  ) : (
                    <Check className="size-3 stroke-[2.2] text-emerald-600/85 dark:text-emerald-400/85" />
                  )}
                </span>
                <span className="min-w-0 truncate font-medium">{subagent.title}</span>
                <ArrowUpRight className="text-muted-foreground/55 group-hover/sachip:text-foreground/85 size-3 shrink-0 opacity-45 transition-opacity group-hover/sachip:opacity-100" />
              </button>
            );
          }

          const isOpen = expandedKey === entry.key;
          const canToggle = entry.detailText.length > 0;
          const isFailed = entry.status === "failed" || entry.status === "denied";
          return (
            <button
              key={entry.key}
              type="button"
              disabled={!canToggle}
              title={entry.detailText ? entry.detailText.slice(0, 180) : entry.label}
              onClick={() => {
                if (!canToggle) return;
                setExpandedKey((prev) => (prev === entry.key ? null : entry.key));
              }}
              className={cn(
                "group/sachip text-foreground/82 inline-flex h-[24px] max-w-[260px] items-center gap-1.5 rounded-full border px-2.5 text-[12px] leading-none transition-colors",
                isFailed
                  ? "border-rose-500/30 bg-rose-500/[0.04]"
                  : "border-black/[0.07] bg-black/[0.02] dark:border-white/[0.09] dark:bg-white/[0.03]",
                canToggle &&
                  (isFailed
                    ? "hover:text-foreground cursor-pointer hover:border-rose-500/45 hover:bg-rose-500/[0.08]"
                    : "hover:text-foreground cursor-pointer hover:border-black/[0.14] hover:bg-black/[0.045] dark:hover:border-white/[0.16] dark:hover:bg-white/[0.06]"),
                isOpen &&
                  (isFailed
                    ? "border-rose-500/45 bg-rose-500/[0.08] text-foreground"
                    : "border-black/[0.16] bg-black/[0.045] text-foreground dark:border-white/[0.18] dark:bg-white/[0.07]"),
              )}
            >
              <span className="flex size-3 shrink-0 items-center justify-center">
                {entry.status === "running" ? (
                  <OrbitSpinner size={11} className="text-foreground/70" />
                ) : isFailed ? (
                  <AlertTriangle className="size-3 text-rose-500" />
                ) : entry.variant === "dispatch" ? (
                  <Check className="size-3 stroke-[2.2] text-emerald-600/85 dark:text-emerald-400/85" />
                ) : (
                  <ArrowRight className="text-muted-foreground/85 size-3 stroke-[1.9]" />
                )}
              </span>
              <span className="min-w-0 truncate font-medium">{entry.label}</span>
              {canToggle && (
                <ChevronRight
                  className={cn(
                    "text-muted-foreground/60 size-3 shrink-0 transition-transform duration-150",
                    isOpen && "rotate-90",
                  )}
                />
              )}
            </button>
          );
        })}
      </div>

      {activeExpandable && activeExpandable.detailText && (
        <div
          className={cn(
            "orbit-stage-enter relative rounded-xl border",
            activeExpandable.status === "failed" || activeExpandable.status === "denied"
              ? "border-rose-500/30 bg-rose-500/[0.04]"
              : "border-border bg-muted/35 dark:bg-muted/20",
          )}
        >
          <div className="border-border/60 flex items-center justify-between gap-2 border-b px-3 py-1.5">
            <span className="text-muted-foreground text-[11.5px] font-medium">
              {activeExpandable.label}
            </span>
            <button
              type="button"
              onClick={() => {
                void navigator.clipboard.writeText(activeExpandable.detailText);
                setCopiedKey(activeExpandable.key);
                window.setTimeout(() => {
                  setCopiedKey((cur) => (cur === activeExpandable.key ? null : cur));
                }, 1400);
              }}
              className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 rounded px-1.5 py-0.5 text-[11px] transition-colors"
            >
              {copiedKey === activeExpandable.key ? (
                <>
                  <Check className="size-3 text-emerald-500" />
                  <span>Copied</span>
                </>
              ) : (
                <>
                  <Copy className="size-3" />
                  <span>Copy</span>
                </>
              )}
            </button>
          </div>
          <ScrollArea viewportClassName="max-h-72 overscroll-contain px-3.5 py-2.5">
            <pre
              className={cn(
                "selectable-text text-foreground/88 leading-relaxed whitespace-pre-wrap break-words",
                activeExpandable.detailIsMono ? "font-mono text-[12px]" : "font-sans text-[12.5px]",
              )}
            >
              {activeExpandable.detailText}
            </pre>
          </ScrollArea>
        </div>
      )}
    </div>
  );
});

/**
 * Unified All-or-Nothing Turn Activity Disclosure:
 * - Folds ALL intermediate steps (thinking, tool calls, resolved gates, interim narration)
 *   behind one clickable summary header (`Reading stream-stage.tsx… · 4 steps · 14s ›` ->
 *   `Read 3 files, edited stream-stage.tsx · 14s ›`).
 * - Strictly respects manual user toggle during and after streaming.
 */
export const TurnActivityDisclosure = memo(function TurnActivityDisclosure({
  items,
  isWorkActive,
  liveLabel,
  liveDetail,
  settledLabel,
  summaryCounts,
  labelIncludesFailure,
  defaultCollapsed,
  autoOpenForError,
  liveStartedAtMs,
  latestActivityMs,
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
  summaryCounts: WorkSummaryResultView["counts"];
  labelIncludesFailure: boolean;
  defaultCollapsed: boolean;
  autoOpenForError: boolean;
  liveStartedAtMs: number | null;
  latestActivityMs?: number | null;
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
  sessionSubagents?: readonly ResolvedSubagentItem[];
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
}) {
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
  const pointerToggledRef = useRef(false);
  const fallbackStartRef = useRef<number>(Date.now());
  const effectiveLiveStartedAtMs = liveStartedAtMs ?? fallbackStartRef.current;

  useEffect(() => {
    if (!canPersist) return;
    const saved = sharedDisclosureStateStore.get(agentId, turnIndex, workOrdinal, turnAnchorSeq);
    setUserExpanded((prev) => (prev === saved ? prev : saved));
  }, [canPersist, agentId, turnIndex, workOrdinal, turnAnchorSeq]);

  if (items.length === 0) {
    if (!isWorkActive) return null;
    return (
      <div className="text-muted-foreground flex max-w-full items-center gap-1.5 py-0.5 text-[13.5px] font-normal">
        <span className="agent-text-shimmer truncate">{liveLabel || "Working…"}</span>
        <InlineLiveElapsed
          startedAtMs={effectiveLiveStartedAtMs}
          lastActivityMs={latestActivityMs}
          isNetworkOffline={isNetworkOffline}
          stallLabel={isBackgroundWait ? null : undefined}
        />
      </div>
    );
  }

  const defaultOpen = !defaultCollapsed || autoOpenForError;
  const isExpanded = userExpanded !== null ? userExpanded : defaultOpen;
  const toggleExpanded = () => {
    holdViewport(foldBtnRef.current, () => {
      setUserExpanded((prev) => {
        const next = !(prev !== null ? prev : defaultOpen);
        if (canPersist) {
          sharedDisclosureStateStore.set(agentId, turnIndex, workOrdinal, turnAnchorSeq, next);
        }
        return next;
      });
    });
  };
  const showFailureBadge =
    !isWorkActive && summaryCounts.unrecovered_failure_count > 0 && !labelIncludesFailure;
  const showDiffNumbers =
    isWorkActive && (summaryCounts.additions > 0 || summaryCounts.deletions > 0);
  const showSettledDuration =
    !isWorkActive &&
    settledDurationMs !== null &&
    settledDurationMs >= 1000 &&
    !settledLabel.startsWith("Thought for ");

  return (
    <div>
      <button
        ref={foldBtnRef}
        type="button"
        aria-expanded={isExpanded}
        onPointerDown={(e) => {
          if (e.button !== 0 || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
          pointerToggledRef.current = true;
          e.preventDefault();
          toggleExpanded();
          const clearGuard = () => {
            window.removeEventListener("pointerup", clearGuard);
            window.removeEventListener("pointercancel", clearGuard);
            window.setTimeout(() => {
              pointerToggledRef.current = false;
            }, 0);
          };
          window.addEventListener("pointerup", clearGuard, { once: true });
          window.addEventListener("pointercancel", clearGuard, { once: true });
        }}
        onClick={() => {
          if (pointerToggledRef.current) {
            pointerToggledRef.current = false;
            return;
          }
          toggleExpanded();
        }}
        className="group/fold text-muted-foreground hover:text-foreground hover:bg-accent/55 -ml-1.5 inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[13.5px] font-normal transition-colors"
      >
        {isWorkActive ? (
          <>
            <span className={cn("agent-text-shimmer", liveDetail ? "shrink-0" : "truncate")}>
              {liveLabel}
            </span>
            {liveDetail && (
              <span className="agent-text-shimmer truncate text-[13px] opacity-85">
                {liveDetail}
              </span>
            )}
            {items.length >= 2 && (
              <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[12.5px]">
                · {items.length} steps
              </span>
            )}
            <InlineLiveElapsed
              startedAtMs={effectiveLiveStartedAtMs}
              lastActivityMs={latestActivityMs}
              isNetworkOffline={isNetworkOffline}
              stallLabel={isBackgroundWait ? "running in background" : undefined}
            />
            {showDiffNumbers && (
              <span className="tabular-nums flex shrink-0 items-center gap-1 font-mono text-[11.5px]">
                {summaryCounts.additions > 0 && (
                  <span className="text-emerald-600 dark:text-emerald-400">
                    +{summaryCounts.additions}
                  </span>
                )}
                {summaryCounts.deletions > 0 && (
                  <span className="text-rose-600 dark:text-rose-400">
                    -{summaryCounts.deletions}
                  </span>
                )}
              </span>
            )}
          </>
        ) : (
          <>
            <span
              className={cn(
                "truncate",
                labelIncludesFailure && "text-rose-600 dark:text-rose-400",
              )}
            >
              {settledLabel}
            </span>
            {showFailureBadge && (
              <span className="shrink-0 text-[12.5px] text-rose-600 dark:text-rose-400">
                · {summaryCounts.unrecovered_failure_count} failed
              </span>
            )}
            {showSettledDuration && (
              <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[12.5px]">
                · {formatDurationCompact(settledDurationMs)}
              </span>
            )}
          </>
        )}

        <ChevronRight
          className={cn(
            "size-3.5 shrink-0 opacity-70 transition-transform duration-150 group-hover/fold:opacity-100",
            isExpanded && "rotate-90",
          )}
        />
      </button>

      {isExpanded && (
        <div className="orbit-stage-enter">
          <StepTimelineRail
            items={items}
            hostId={hostId}
            cwd={cwd}
            holdViewport={holdViewport}
            indented
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
        </div>
      )}
    </div>
  );
});

export const CompactionMilestoneRow = memo(function CompactionMilestoneRow({
  label,
}: {
  label?: string;
}) {
  return (
    <div className="py-0.5 select-none">
      <span className="text-muted-foreground/85 border-border/75 bg-muted/45 dark:bg-muted/30 inline-flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[12px] font-normal">
        <Scissors className="size-3 shrink-0 opacity-75" />
        <span className="truncate">{label || "Context compacted"}</span>
      </span>
    </div>
  );
});

export const WakeupEventRow = memo(function WakeupEventRow({ label }: { label?: string }) {
  return (
    <div className="text-muted-foreground/80 flex items-center gap-1.5 py-0.5 text-[12.5px] font-normal select-none">
      <RotateCw className="size-3 shrink-0 opacity-75" />
      <span className="truncate">{label || "Resumed from background task"}</span>
    </div>
  );
});

export const InlineArtifactCard = memo(function InlineArtifactCard({
  hostId,
  artifact,
}: {
  hostId: string;
  artifact: InlineArtifactRef;
}) {
  return (
    <button
      type="button"
      onClick={() =>
        orbitActions.openArtifactsTab({
          hostId,
          artifactId: artifact.artifactId,
        })
      }
      className="border-border/80 bg-elevated/45 hover:bg-elevated/85 group inline-flex max-w-full cursor-pointer items-center gap-2 rounded-xl border px-3 py-1.5 text-left text-[12.5px] transition-colors"
    >
      <Blocks className="text-muted-foreground group-hover:text-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
      <span className="text-muted-foreground shrink-0 text-[11.5px]">Artifact</span>
      <span className="text-muted-foreground/55 shrink-0">·</span>
      <span className="text-foreground truncate font-medium">{artifact.title}</span>
      {artifact.version != null && (
        <>
          <span className="text-muted-foreground/55 shrink-0">·</span>
          <span className="text-muted-foreground shrink-0 font-mono text-[11.5px]">
            v{artifact.version}
          </span>
        </>
      )}
      <ChevronRight className="text-muted-foreground/55 group-hover:text-foreground h-[13px] w-[13px] shrink-0 transition-transform group-hover:translate-x-0.5" />
    </button>
  );
});
