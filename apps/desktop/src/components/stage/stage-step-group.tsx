import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowLeftRight,
  ArrowRight,
  ArrowUp,
  Blocks,
  Check,
  ChevronRight,
  Copy,
  Maximize2,
  Minimize2,
  RotateCw,
  Scissors,
  Timer,
} from "lucide-react";
import { DiffStat } from "@/components/ui/diff-stat";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ScrollArea } from "@/components/ui/scroll-area";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
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
import { formatDurationCompact } from "@/lib/timeline-coalesce";
import {
  type ToolCallTimelineItem,
  extractStateChangeMilestoneCall,
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
import { openSubagent } from "./subagent-crew-dock";
import { InlineLiveElapsed, StepItemRow } from "./stage-tool-renderers";

type SubagentMaybeWithAvatar = ResolvedSubagentItem & {
  readonly character?: SubagentCharacterSpec;
};

/** Step count threshold above which an expanded work group uses bounded internal scroll + sticky label. */
const LONG_DISCLOSURE_STEP_THRESHOLD = 9;

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
  sessionSubagents?: readonly SubagentMaybeWithAvatar[];
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
}) {
  if (items.length === 0) return null;

  const lastSeq = items[items.length - 1]?.seq;

  return (
    <div
      className={cn(
        "space-y-0.5",
        indented && "border-border-subtle mt-1 ml-[7px] border-l pl-3.5",
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
      kind: "subagent_dispatch";
      key: string;
      subagent: SubagentMaybeWithAvatar;
      character: SubagentCharacterSpec;
      linked: boolean;
      callStatus: "running" | "completed" | "failed" | "denied";
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
      detailIsMono: boolean;
      status: "running" | "completed" | "failed" | "denied";
    };

/**
 * First-class chronological milestone row rendered outside folded step groups:
 * 1. Subagent dispatches (`invoke_subagent`, `create_agent`, `spawn_workspace_peer`) — compact horizontal
 *    chips featuring each subagent's animal avatar (bouncing while working, quiet when done).
 * 2. Inter-agent messages (`send_message`, `message_peer`, `send_agent_prompt`) — compact expandable
 *    left-aligned rows matching `AgentMessageCard` with clean left-rail body (never card-inside-card).
 * 3. Session state milestones (Timers, Worktree Handoffs) — subtle inline chips.
 */
export const DelegationMilestoneRow = memo(function DelegationMilestoneRow({
  items,
  hostId,
  agentId,
  sessionSubagents,
  dispatches,
  parentMessageLabel,
}: {
  items: readonly ToolCallTimelineItem[];
  hostId?: string;
  agentId?: string;
  sessionSubagents?: readonly SubagentMaybeWithAvatar[];
  dispatches?: ReturnType<typeof resolveDelegationDispatches>;
  parentMessageLabel?: string | null;
}) {
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  const { t } = useI18n();
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
            label: `→ ${enriched.title}`,
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
          callStatus: it.status,
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

      const stateMilestone = extractStateChangeMilestoneCall(it);
      if (stateMilestone) {
        out.push({
          kind: "state_change",
          key: `state:${stateMilestone.callId}`,
          milestoneKind: stateMilestone.kind,
          label: stateMilestone.label,
          detailText: stateMilestone.preview,
          detailIsMono: false,
          status: stateMilestone.status,
        });
        continue;
      }
    }

    return out;
  }, [items, sessionSubagents, hostId, sentMessageLabel, dispatches]);

  if (entries.length === 0) return null;

  const chipEntries = entries.filter(
    (e): e is Exclude<DelegationMilestoneEntry, { kind: "inter_agent_message" }> =>
      e.kind !== "inter_agent_message",
  );
  const messageEntries = entries.filter(
    (e): e is Extract<DelegationMilestoneEntry, { kind: "inter_agent_message" }> =>
      e.kind === "inter_agent_message",
  );

  const openSubagentRecord = (subagent: ResolvedSubagentItem) => openSubagent(agentId, subagent);

  const activeChipDetail = expandedKey
    ? (chipEntries.find(
        (e): e is Extract<DelegationMilestoneEntry, { kind: "state_change" }> =>
          e.kind === "state_change" && e.key === expandedKey,
      ) ?? null)
    : null;

  return (
    <div className="space-y-1.5">
      {/* 1. Compact horizontal row for Subagent dispatches (with animal avatars) & state chips */}
      {chipEntries.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          {chipEntries.map((entry) => {
            if (entry.kind === "subagent_dispatch") {
              const { subagent, character, callStatus } = entry;
              const effectiveStatus = subagent.status;
              const starting = effectiveStatus === "running" && !entry.linked;
              const actionKey =
                callStatus === "failed" || callStatus === "denied"
                  ? "stage.subagentLaunch"
                  : starting && callStatus === "running"
                    ? "stage.creatingSubagent"
                    : "stage.spawnedSubagent";
              const statusKey = {
                running: starting ? "stage.subagentStarting" : "overview.subagentWorking",
                awaiting_permission: "overview.subagentApprovalRequired",
                failed: "overview.subagentFailed",
                completed: "stage.subagentDone",
                canceled: "overview.subagentStopped",
              } as const;
              const isFailed = effectiveStatus === "failed";
              const isAwaiting = effectiveStatus === "awaiting_permission";

              return (
                <button
                  key={entry.key}
                  type="button"
                  title={subagent.promptPreview || t("overview.openSubagent", { name: subagent.title })}
                  onClick={() => openSubagentRecord(subagent)}
                  className={cn(
                    "group/sachip inline-flex min-h-[34px] min-w-0 max-w-full cursor-pointer items-center gap-2 rounded-[10px] border px-2.5 py-1 text-left text-[12px] leading-none transition-colors",
                    isFailed
                      ? "border-signal-danger-border bg-signal-danger-bg/65 text-ink-primary hover:bg-signal-danger-bg"
                      : isAwaiting
                        ? "border-signal-warning-border bg-signal-warning-bg/75 text-ink-primary hover:bg-signal-warning-bg"
                        : "border-border-subtle bg-surface-raised/85 text-ink-primary hover:border-border-default hover:bg-accent/70",
                  )}
                >
                  <span className="text-ink-tertiary text-[11px]">{t(actionKey)}</span>
                  <SubagentAvatar
                    character={character}
                    status={effectiveStatus}
                    size="sm"
                    showStatusBadge={false}
                  />
                  {(entry.linked || subagent.title !== "Subagent") && (
                    <span className="min-w-0 max-w-[440px] truncate font-medium">{subagent.title}</span>
                  )}
                  <span className="text-ink-secondary ml-auto shrink-0 pl-1 text-[11px]">{t(statusKey[effectiveStatus])}</span>
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
                  "group/sachip inline-flex h-[28px] max-w-[260px] items-center gap-1.5 rounded-[8px] border px-2.5 text-[12px] leading-none transition-colors",
                  isFailed
                    ? "border-signal-danger-border bg-signal-danger-bg/60 text-signal-danger"
                    : "border-border-subtle bg-surface-recessed/65 text-ink-secondary",
                  canToggle && "hover:border-border-default hover:text-ink-primary cursor-pointer",
                  isOpen && "border-border-default bg-accent/70 text-ink-primary",
                )}
              >
                <span className="flex size-3.5 shrink-0 items-center justify-center">
                  {entry.status === "running" ? (
                    <OrbitSpinner size={11} className="text-ink-secondary" />
                  ) : isFailed ? (
                    <AlertTriangle className="text-signal-danger size-3" />
                  ) : entry.milestoneKind === "timer" ? (
                    <Timer className="text-signal-warning size-3" />
                  ) : (
                    <ArrowLeftRight className="text-signal-info size-3" />
                  )}
                </span>
                <span className="min-w-0 truncate font-medium">{entry.label}</span>
                {canToggle && (
                  <ChevronRight
                    className={cn(
                      "text-ink-muted size-3 shrink-0 transition-transform duration-150",
                      isOpen && "rotate-90",
                    )}
                  />
                )}
              </button>
            );
          })}
        </div>
      )}

      {activeChipDetail && activeChipDetail.detailText && (
        <div className="orbit-stage-enter border-border-default ml-2 border-l-2 pl-3">
          <div className="mb-1 flex items-center justify-between gap-2">
            <span className="text-ink-tertiary text-[11px] font-medium">
              {activeChipDetail.label}
            </span>
            <button
              type="button"
              onClick={() => {
                void navigator.clipboard.writeText(activeChipDetail.detailText);
                setCopiedKey(activeChipDetail.key);
                window.setTimeout(() => {
                  setCopiedKey((cur) => (cur === activeChipDetail.key ? null : cur));
                }, 1400);
              }}
              className="text-ink-tertiary hover:text-ink-primary inline-flex cursor-pointer items-center gap-1 rounded-[6px] px-1.5 py-0.5 text-[11px] transition-colors"
            >
              {copiedKey === activeChipDetail.key ? (
                <>
                  <Check className="text-signal-success size-3" />
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
          <ScrollArea viewportClassName="max-h-56 overscroll-contain">
            <pre
              className={cn(
                "selectable-text text-ink-secondary leading-relaxed whitespace-pre-wrap break-words",
                activeChipDetail.detailIsMono ? "font-mono text-[12px]" : "font-sans text-[12px]",
              )}
            >
              {activeChipDetail.detailText}
            </pre>
          </ScrollArea>
        </div>
      )}

      {/* 2. Inter-agent message rows (unified compact expandable bar + clean left-rail body) */}
      {messageEntries.map((msg) => {
        const isOpen = expandedKey === msg.key;
        const canExpand = msg.detailText.length > 0;
        const isFailed = msg.status === "failed" || msg.status === "denied";

        return (
          <div key={msg.key} className="min-w-0">
            <div
              className={cn(
                "group/msgrow flex min-h-[34px] items-center gap-1.5 rounded-[10px] border pr-1.5 transition-colors",
                isFailed
                  ? "border-signal-danger-border bg-signal-danger-bg/45"
                  : "border-border-subtle bg-surface-recessed/55 hover:border-border-default",
              )}
            >
              <button
                type="button"
                disabled={!msg.subagent && !canExpand}
                aria-expanded={!msg.subagent && canExpand ? isOpen : undefined}
                title={msg.subagent ? t("overview.openSubagent", { name: msg.subagent.title }) : msg.preview || msg.label}
                onClick={() => {
                  if (msg.subagent) return openSubagentRecord(msg.subagent);
                  if (!canExpand) return;
                  setExpandedKey((prev) => (prev === msg.key ? null : msg.key));
                }}
                className={cn(
                  "flex min-h-[34px] min-w-0 flex-1 items-center gap-2 py-1 pl-2 pr-1 text-left text-[12px]",
                  msg.subagent || canExpand ? "cursor-pointer" : "cursor-default",
                )}
              >
                {msg.character ? (
                  <SubagentAvatar
                    character={msg.character}
                    status={msg.subagent?.status ?? (msg.status === "running" ? "running" : "completed")}
                    size="sm"
                  />
                ) : (
                  <span className="bg-surface-raised border-border-subtle flex size-[20px] shrink-0 items-center justify-center rounded-full border">
                    {msg.status === "running" ? (
                      <OrbitSpinner size={11} className="text-ink-secondary" />
                    ) : isFailed ? (
                      <AlertTriangle className="text-signal-danger size-3" />
                    ) : (
                      <ArrowRight className="text-ink-tertiary size-3 stroke-[1.9]" />
                    )}
                  </span>
                )}
                <span className="text-ink-primary shrink-0 font-medium">{msg.label}</span>
                {!isOpen && msg.preview && (
                  <>
                    <span aria-hidden="true" className="text-ink-muted shrink-0">
                      ·
                    </span>
                    <span className="text-ink-tertiary min-w-0 flex-1 truncate">
                      {msg.preview}
                    </span>
                  </>
                )}
              </button>

              <div className="flex shrink-0 items-center gap-0.5">
                {canExpand && (
                  <IconButton
                    size="xs"
                    aria-expanded={isOpen}
                    aria-label={t(isOpen ? "stage.collapseMessage" : "stage.expandMessage")}
                    onClick={() => setExpandedKey((prev) => (prev === msg.key ? null : msg.key))}
                  >
                    <ChevronRight
                      className={cn(
                        "size-3.5 transition-transform duration-150",
                        isOpen && "rotate-90",
                      )}
                    />
                  </IconButton>
                )}
              </div>
            </div>

            {isOpen && canExpand && (
              <div className="orbit-stage-enter border-border-default mt-1.5 ml-3 border-l-2 pl-3">
                <ScrollArea viewportClassName="max-h-64 overscroll-contain py-0.5">
                  <pre
                    className={cn(
                      "selectable-text text-ink-secondary leading-[1.6] whitespace-pre-wrap break-words",
                      msg.detailIsMono ? "font-mono text-[12px]" : "font-sans text-[12px]",
                    )}
                  >
                    {msg.detailText}
                  </pre>
                </ScrollArea>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
});

/**
 * Unified Turn Activity Disclosure with smart long-run handling:
 * - Collapsed (default): clean quiet single line (`Read 8 files, edited 3 files · 24s ›`).
 * - Short expanded (<= 8 steps): lightweight inline indented vertical timeline rail.
 * - Long expanded (>= 9 steps):
 *   1) Default bounded internal scroll mode (`max-h-[340px]`) with a FIXED 32px Label Bar pinned
 *      above the scroll area so the summary & collapse action never scroll out of view; auto-follows
 *      the latest step while working.
 *   2) Optional Full-Expand toggle (`⤢`) releasing the height cap while pinning the 32px Label Bar
 *      as `position: sticky; top: 0` inside the main reading viewport + a quiet bottom collapse button.
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
  sessionSubagents?: readonly SubagentMaybeWithAvatar[];
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
  const [fullHeightExpanded, setFullHeightExpanded] = useState(false);
  const foldBtnRef = useRef<HTMLButtonElement | null>(null);
  const innerScrollRef = useRef<HTMLDivElement | null>(null);
  const pointerToggledRef = useRef(false);
  const fallbackStartRef = useRef<number>(Date.now());
  const effectiveLiveStartedAtMs = liveStartedAtMs ?? fallbackStartRef.current;

  useEffect(() => {
    if (!canPersist) return;
    const saved = sharedDisclosureStateStore.get(agentId, turnIndex, workOrdinal, turnAnchorSeq);
    setUserExpanded((prev) => (prev === saved ? prev : saved));
  }, [canPersist, agentId, turnIndex, workOrdinal, turnAnchorSeq]);

  const defaultOpen = !defaultCollapsed || autoOpenForError;
  const isExpanded = userExpanded !== null ? userExpanded : defaultOpen;
  const isLongGroup = items.length >= LONG_DISCLOSURE_STEP_THRESHOLD;
  const useBoundedScroll = isExpanded && isLongGroup && !fullHeightExpanded;
  const useStickyFullMode = isExpanded && isLongGroup && fullHeightExpanded;

  // Gently keep the latest step visible inside bounded inner scroll while active
  useEffect(() => {
    if (!useBoundedScroll || !isWorkActive) return;
    const el = innerScrollRef.current;
    if (!el) return;
    const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distFromBottom < 80) {
      el.scrollTop = el.scrollHeight;
    }
  }, [useBoundedScroll, isWorkActive, items.length]);

  if (items.length === 0) {
    if (!isWorkActive) return null;
    return (
      <div className="text-ink-tertiary flex max-w-full items-center gap-1.5 py-0.5 text-[13px] font-normal">
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

  const toggleExpanded = () => {
    holdViewport(foldBtnRef.current, () => {
      setUserExpanded((prev) => {
        const next = !(prev !== null ? prev : defaultOpen);
        if (!next) {
          setFullHeightExpanded(false);
        }
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

  const labelContent = isWorkActive ? (
    <>
      <span className={cn("agent-text-shimmer", liveDetail ? "shrink-0" : "truncate")}>
        {liveLabel}
      </span>
      {liveDetail && (
        <span className="agent-text-shimmer truncate text-[12px] opacity-80">
          {liveDetail}
        </span>
      )}
      {items.length >= 2 && (
        <span className="text-ink-muted tabular-nums shrink-0 text-[12px]">
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
        <DiffStat
          size="xs"
          additions={summaryCounts.additions}
          deletions={summaryCounts.deletions}
        />
      )}
    </>
  ) : (
    <>
      <span className={cn("truncate", labelIncludesFailure && "text-signal-danger")}>
        {settledLabel}
      </span>
      {showFailureBadge && (
        <span className="text-signal-danger shrink-0 text-[12px]">
          · {summaryCounts.unrecovered_failure_count} failed
        </span>
      )}
      {showSettledDuration && (
        <span className="text-ink-muted tabular-nums shrink-0 text-[12px]">
          · {formatDurationCompact(settledDurationMs)}
        </span>
      )}
    </>
  );

  const bindTogglePointer = {
    onPointerDown: (e: React.PointerEvent<HTMLButtonElement>) => {
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
    },
    onClick: () => {
      if (pointerToggledRef.current) {
        pointerToggledRef.current = false;
        return;
      }
      toggleExpanded();
    },
  };

  // Long Group — Bounded Scroll Track (Fixed 32px Top Label Bar + max-h 340px Inner Scroll)
  if (useBoundedScroll) {
    return (
      <div className="orbit-disclosure-track overflow-hidden">
        <div className="border-border-subtle flex h-[32px] items-center justify-between gap-2 border-b px-2.5">
          <button
            ref={foldBtnRef}
            type="button"
            aria-expanded="true"
            {...bindTogglePointer}
            className="group/fold text-ink-secondary hover:text-ink-primary flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left text-[13px] font-normal transition-colors"
          >
            <ChevronRight className="text-ink-muted size-3.5 shrink-0 rotate-90 transition-transform duration-150" />
            {labelContent}
          </button>

          <IconButton
            size="xs"
            tooltip="Expand full list"
            onClick={() => setFullHeightExpanded(true)}
          >
            <Maximize2 className="size-3" />
          </IconButton>
        </div>

        <div
          ref={innerScrollRef}
          className="orbit-native-scroll orbit-disclosure-scroll max-h-[340px] overflow-y-auto px-3 py-2"
        >
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
      </div>
    );
  }

  // Long Group — Full-Height Expanded with Sticky Top Label Bar + Bottom Collapse Anchor
  if (useStickyFullMode) {
    return (
      <div className="min-w-0">
        <div className="orbit-sticky-disclosure-bar border-border-subtle -mx-2.5 flex h-[32px] items-center justify-between gap-2 border-b px-2.5">
          <button
            ref={foldBtnRef}
            type="button"
            aria-expanded="true"
            {...bindTogglePointer}
            className="group/fold text-ink-secondary hover:text-ink-primary flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left text-[13px] font-normal transition-colors"
          >
            <ChevronRight className="text-ink-muted size-3.5 shrink-0 rotate-90 transition-transform duration-150" />
            {labelContent}
          </button>

          <IconButton
            size="xs"
            tooltip="Use compact scroll box"
            onClick={() => setFullHeightExpanded(false)}
          >
            <Minimize2 className="size-3" />
          </IconButton>
        </div>

        <div className="orbit-stage-enter pt-1">
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
          <div className="mt-1.5 pl-4">
            <button
              type="button"
              onClick={toggleExpanded}
              className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-[6px] px-2 py-0.5 text-[11px] font-medium transition-colors"
            >
              <ArrowUp className="size-3" />
              <span>Collapse {items.length} steps</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  // Collapsed or Short Expanded (<= 8 steps) — Crisp, Frameless Inline Disclosure
  return (
    <div>
      <button
        ref={foldBtnRef}
        type="button"
        aria-expanded={isExpanded}
        {...bindTogglePointer}
        className="group/fold text-ink-tertiary hover:text-ink-primary hover:bg-accent/65 -ml-1.5 inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-[6px] px-1.5 py-0.5 text-[13px] font-normal transition-colors"
      >
        {labelContent}

        <ChevronRight
          className={cn(
            "size-3.5 shrink-0 opacity-65 transition-transform duration-150 group-hover/fold:opacity-100",
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
      <span className="text-ink-tertiary border-border-subtle bg-surface-recessed/70 inline-flex h-[26px] max-w-full items-center gap-1.5 rounded-[8px] border px-2.5 text-[12px] font-normal">
        <Scissors className="size-3 shrink-0 opacity-75" />
        <span className="truncate">{label || "Context compacted"}</span>
      </span>
    </div>
  );
});

export const WakeupEventRow = memo(function WakeupEventRow({ label }: { label?: string }) {
  return (
    <div className="text-ink-tertiary flex items-center gap-1.5 py-0.5 text-[12px] font-normal select-none">
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
      className="border-border-subtle bg-surface-raised/70 hover:bg-accent/70 hover:border-border-default group inline-flex h-[30px] max-w-full cursor-pointer items-center gap-2 rounded-[8px] border px-2.5 text-left text-[12px] transition-colors"
    >
      <Blocks className="text-ink-tertiary group-hover:text-ink-primary size-[13px] shrink-0 stroke-[1.75]" />
      <span className="text-ink-tertiary shrink-0 text-[11px]">Artifact</span>
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
