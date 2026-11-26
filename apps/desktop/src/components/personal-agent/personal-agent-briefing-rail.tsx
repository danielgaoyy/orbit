import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type MouseEvent,
  type RefObject,
} from "react";
import {
  AlarmClock,
  ArrowUpRight,
  Bot,
  Check,
  CheckCheck,
  Clock,
  Settings2,
  ShieldAlert,
  Target,
  X,
} from "lucide-react";
import { StageRailShell } from "@/components/stage/stage-rail-shell";
import {
  ArmedTimerRow,
  OverviewGoalRow,
  OverviewSection,
} from "@/components/stage/stage-overview-sections";
import { AgentListItem } from "@/components/ui/agent-list-item";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ContextBanner } from "@/components/ui/context-banner";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SimpleTooltip } from "@/components/ui/overlays";
import {
  StageRailEmptyState,
  StageRailExpandFooter,
  StageRailHeader,
  StageRailScrollBody,
} from "@/components/ui/stage-rail-primitives";
import {
  buildAgentListItemTooltip,
  filterDistinctWorkerStepPreview,
  formatAgentListItemStatusLabel,
  resolveAgentItemRuntime,
} from "@/lib/agent-list-item";
import { useI18n } from "@/lib/i18n";
import { useActionShortcutDisplay } from "@/lib/keybindings";
import { useLiveSecondClock } from "@/lib/live-clock";
import {
  type AgentCardView,
  type BriefingDecision,
  type BriefingHost,
  type BriefingWaitItem,
  type HostDetailView,
  type PersonalAgentActivityEntry,
  formatTimerCountdown,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  type SubagentCharacterSpec,
  assignSubagentCharacters,
} from "@/lib/subagent-avatar";
import { isAlwaysDecision, isDenyDecision } from "@/lib/timeline-coalesce";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import {
  type PartitionedBriefingWorker,
  type SinceLastVisitSummary,
  briefingWorkerKey,
  findLiveBriefingGoal,
  findLiveBriefingTimer,
  formatBriefingElapsed,
  partitionBriefingWorkers,
} from "./personal-agent-briefing-model";
import { usePersonalAgentAwayBanner, usePersonalAgentBriefing } from "./use-personal-agent-briefing";

const DEFAULT_VISIBLE_WORKERS = 5;
const DEFAULT_VISIBLE_WAITING = 3;
const DEFAULT_VISIBLE_ACTIVITY = 5;

interface BriefingAgentContext {
  readonly agent: AgentCardView | null;
  readonly hostDetail: HostDetailView | null;
  readonly character: SubagentCharacterSpec | null;
}

function resolveBriefingAgentContext(
  hostDetails: Readonly<Record<string, HostDetailView>>,
  characterByKey: ReadonlyMap<string, SubagentCharacterSpec>,
  hostId: string,
  agentId: string,
): BriefingAgentContext {
  const hostDetail = hostDetails[hostId] ?? null;
  const agent = hostDetail?.agents.find((a) => a.agent_id === agentId) ?? null;
  const character = characterByKey.get(briefingWorkerKey(hostId, agentId)) ?? null;
  return { agent, hostDetail, character };
}

function openWorkerSurface(
  e: MouseEvent,
  hostId: string,
  agentId: string,
  title?: string,
  parentContext?: { hostId: string; agentId: string },
): void {
  const snap = orbitActions.getSnapshot();
  if (
    snap.workspacePanel === "conversation" &&
    !snap.isDraftNewSession &&
    snap.selectedHostId === hostId &&
    snap.selectedAgentId === agentId
  ) {
    orbitActions.setOverviewPopoverOpen(false);
    orbitActions.focusComposer();
    return;
  }
  if (e.metaKey || e.ctrlKey) {
    orbitActions.selectAgent(hostId, agentId);
    return;
  }
  const agent = snap.hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId);
  const resolvedTitle = title?.trim() || agent?.title || agentId;
  orbitActions.openSubagentTab({
    callId: agentId,
    role: resolvedTitle,
    promptPreview: "",
    childAgentId: agentId,
    childHostId: hostId,
    parentHostId: parentContext?.hostId ?? snap.selectedHostId ?? undefined,
    parentAgentId: parentContext?.agentId ?? snap.selectedAgentId ?? undefined,
  });
}

const LiveElapsedChip = memo(function LiveElapsedChip({
  tsMs,
  active,
  className,
}: {
  tsMs: number;
  active: boolean;
  className?: string;
}) {
  const nowMs = useLiveSecondClock(active);
  const label = formatBriefingElapsed(tsMs, nowMs);
  if (!label) return null;
  return <span className={cn("font-mono text-[11px] tabular-nums", className)}>{label}</span>;
});

const WaitCountdownChip = memo(function WaitCountdownChip({ dueAtMs }: { dueAtMs: number }) {
  const nowMs = useLiveSecondClock(true);
  return (
    <span className="text-ink-primary shrink-0 font-mono text-[11px] font-medium tabular-nums">
      {formatTimerCountdown(dueAtMs, nowMs)}
    </span>
  );
});

const DecisionCard = memo(function DecisionCard({
  decision,
  agentContext,
  paContext,
  showHostBadge,
  busy,
  onResolve,
}: {
  decision: BriefingDecision;
  agentContext: BriefingAgentContext;
  paContext: { hostId: string; agentId: string };
  showHostBadge: boolean;
  busy: boolean;
  onResolve: (decision: BriefingDecision, mode: "allow" | "deny") => void;
}) {
  const { t } = useI18n();
  const isPlan = decision.kind === "plan";
  const isError = decision.kind === "error";
  const canQuickDecide = decision.kind === "permission" && Boolean(decision.request_id);
  const isHighRisk =
    decision.risk_level?.toLowerCase() === "high" ||
    decision.risk_level?.toLowerCase() === "critical";
  const rec = decision.router_recommendation;

  const status = isError ? "failed" : "awaiting_permission";
  const runtime = resolveAgentItemRuntime({
    agent: agentContext.agent,
    hostDetail: agentContext.hostDetail,
    t,
  });
  const decisionStatusLabel = isPlan
    ? t("briefing.planReviewBadge")
    : isError
      ? t("briefing.errorBadge")
      : t("overview.subagentApprovalRequired");
  const tooltip = buildAgentListItemTooltip({
    title: decision.agent_title,
    agentLabel: runtime.agentLabel,
    rawModel: runtime.rawModel,
    modelLabel: runtime.modelLabel,
    effortLabel: runtime.effortLabel,
    statusLabel: decisionStatusLabel,
    hostName: showHostBadge ? decision.host_name : null,
    detail: decision.summary,
    hint: t("briefing.openWorkerHint"),
  });

  return (
    <AgentListItem
      variant="card"
      title={decision.agent_title}
      status={status}
      tone={isError || isHighRisk ? "danger" : "warning"}
      character={agentContext.character}
      avatarSeedKey={decision.agent_id}
      provider={runtime.provider}
      modelLabel={runtime.modelLabel}
      effortLabel={runtime.effortLabel}
      agentLabel={runtime.agentLabel}
      statusLabel={decisionStatusLabel}
      secondaryMeta={showHostBadge ? decision.host_name : null}
      tooltip={tooltip}
      onClick={(e) =>
        openWorkerSurface(
          e,
          decision.host_id,
          decision.agent_id,
          decision.agent_title,
          paContext,
        )
      }
      titleTrailing={
        <LiveElapsedChip
          tsMs={decision.updated_at_ms}
          active={false}
          className="text-ink-faint shrink-0"
        />
      }
      footer={
        <>
          {!isPlan && !isError && isHighRisk && decision.risk_level && (
            <div className="flex flex-wrap items-center gap-1">
              <Badge variant="destructive" className="h-[18px] px-1.5 text-[11px]">
                <ShieldAlert className="size-2.5" />
                {decision.risk_level}
              </Badge>
            </div>
          )}

          <p className="text-ink-secondary line-clamp-2 text-[13px] leading-[1.4] break-words">
            {decision.summary}
          </p>

          {rec && (
            <div className="bg-surface-recessed border-border-subtle flex items-start gap-1.5 rounded-[8px] border px-2 py-1.5 text-[11px] leading-[1.35]">
              <Bot className="text-ink-tertiary mt-[1px] size-[11px] shrink-0" />
              <div className="min-w-0 flex-1">
                <span
                  className={cn(
                    "font-medium",
                    rec.decision === "deny" ? "text-signal-danger" : "text-signal-success",
                  )}
                >
                  {rec.decision === "deny"
                    ? t("briefing.paRecommendDeny")
                    : t("briefing.paRecommendAllow")}
                </span>
                {rec.reason && (
                  <>
                    <span className="text-ink-faint"> · </span>
                    <span className="text-ink-secondary break-words">{rec.reason}</span>
                  </>
                )}
              </div>
            </div>
          )}

          <div className="flex items-center justify-end gap-1">
            {canQuickDecide && (
              <>
                <Button
                  type="button"
                  size="xs"
                  variant="ghost"
                  disabled={busy}
                  onClick={() => onResolve(decision, "deny")}
                  className="text-ink-secondary hover:text-signal-danger h-[24px] px-2 text-[11px]"
                >
                  {t("approval.deny")}
                </Button>
                <Button
                  type="button"
                  size="xs"
                  disabled={busy}
                  onClick={() => onResolve(decision, "allow")}
                  className="h-[24px] gap-1 px-2 text-[11px]"
                >
                  {busy ? <OrbitSpinner size={11} /> : <Check className="size-3" />}
                  <span>{t("approval.allow")}</span>
                </Button>
              </>
            )}
            <Button
              type="button"
              size="xs"
              variant={canQuickDecide ? "outline" : "default"}
              title={t("briefing.openWorkerHint")}
              onClick={(e) =>
                openWorkerSurface(
                  e,
                  decision.host_id,
                  decision.agent_id,
                  decision.agent_title,
                  paContext,
                )
              }
              className="h-[24px] gap-1 px-2 text-[11px]"
            >
              <span>{isPlan ? t("briefing.reviewAction") : t("briefing.viewAction")}</span>
              <ArrowUpRight className="size-3" />
            </Button>
          </div>
        </>
      }
    />
  );
});

const BriefingWorkerRow = memo(function BriefingWorkerRow({
  item,
  agentContext,
  paContext,
  showHostBadge,
}: {
  item: PartitionedBriefingWorker;
  agentContext: BriefingAgentContext;
  paContext: { hostId: string; agentId: string };
  showHostBadge: boolean;
}) {
  const { t } = useI18n();
  const { worker, status, indentLevel, parentHostId, lineage } = item;
  const runtime = resolveAgentItemRuntime({
    provider: worker.provider,
    agent: agentContext.agent,
    hostDetail: agentContext.hostDetail,
    t,
  });
  const stateText = formatAgentListItemStatusLabel(status, t);
  const distinctStep = filterDistinctWorkerStepPreview(worker.step_preview, worker.title);
  const hostMeta =
    showHostBadge && (!parentHostId || worker.host_id !== parentHostId)
      ? worker.host_name
      : null;
  const tooltip = buildAgentListItemTooltip({
    title: worker.title,
    lineage,
    agentLabel: runtime.agentLabel,
    rawModel: runtime.rawModel,
    modelLabel: runtime.modelLabel,
    effortLabel: runtime.effortLabel,
    statusLabel: stateText,
    hostName: hostMeta ?? (showHostBadge ? worker.host_name : null),
    worktreeBranch: worker.worktree_branch,
    detail: distinctStep,
    hint: t("briefing.openWorkerHint"),
  });

  return (
    <AgentListItem
      title={worker.title}
      status={status}
      character={agentContext.character}
      avatarSeedKey={worker.agent_id}
      provider={runtime.provider}
      modelLabel={runtime.modelLabel}
      effortLabel={runtime.effortLabel}
      agentLabel={runtime.agentLabel}
      statusLabel={stateText}
      secondaryMeta={hostMeta}
      indentLevel={indentLevel}
      tooltip={tooltip}
      onClick={(e) =>
        openWorkerSurface(e, worker.host_id, worker.agent_id, worker.title, paContext)
      }
    />
  );
});

const ActivityLogRow = memo(function ActivityLogRow({
  entry,
  paContext,
}: {
  entry: PersonalAgentActivityEntry;
  paContext: { hostId: string; agentId: string };
}) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const isDenied = entry.decision.toLowerCase() === "deny";
  const actorLabel =
    entry.actor === "personal_agent"
      ? t("briefing.actorPersonalAgent")
      : entry.actor === "guardian_tier1"
        ? t("briefing.actorGuardian")
        : t("briefing.actorRule");
  const targetHostId = entry.host_id || paContext.hostId;
  const canOpen = Boolean(entry.agent_id && targetHostId);
  const hasReason = Boolean(entry.reason?.trim());

  return (
    <div
      className={cn(
        "group/actrow rounded-[8px] px-2 py-1.5 transition-colors",
        expanded ? "bg-surface-recessed/75" : "hover:bg-accent/65",
      )}
    >
      <div className="flex items-start gap-2">
        <span className="mt-[3px] flex size-[14px] shrink-0 items-center justify-center">
          {isDenied ? (
            <X className="text-signal-danger size-[12px] stroke-[2.2]" />
          ) : (
            <Check className="text-signal-success size-[12px] stroke-[2.2]" />
          )}
        </span>

        <button
          type="button"
          onClick={() => hasReason && setExpanded((v) => !v)}
          aria-expanded={hasReason ? expanded : undefined}
          className={cn("min-w-0 flex-1 text-left", hasReason ? "cursor-pointer" : "cursor-default")}
        >
          <div className="flex items-center justify-between gap-1.5">
            <span className="text-ink-primary truncate text-[13px] leading-[18px] font-medium">
              {entry.agent_title}
            </span>
            <LiveElapsedChip
              tsMs={entry.timestamp_ms}
              active={false}
              className="text-ink-faint shrink-0"
            />
          </div>
          <div className="text-ink-secondary truncate text-[13px] leading-[17px]">
            {entry.summary}
          </div>
          <div className="text-ink-tertiary mt-0.5 flex flex-wrap items-center gap-1 text-[11px] leading-[15px]">
            <span className={cn("font-medium", isDenied ? "text-signal-danger" : "text-signal-success")}>
              {isDenied ? t("briefing.decisionDenied") : t("briefing.decisionAllowed")}
            </span>
            <span className="opacity-45">·</span>
            <span>{actorLabel}</span>
            {entry.risk_level && (
              <>
                <span className="opacity-45">·</span>
                <span className="uppercase">{entry.risk_level}</span>
              </>
            )}
          </div>
        </button>

        {canOpen && entry.agent_id && (
          <IconButton
            size="xs"
            tooltip={t("briefing.openSession")}
            aria-label={t("briefing.openSession")}
            onClick={(e) =>
              openWorkerSurface(
                e,
                targetHostId,
                entry.agent_id!,
                entry.agent_title,
                paContext,
              )
            }
            className="text-ink-muted hover:text-ink-primary mt-0.5 shrink-0 opacity-75 transition-opacity group-hover/actrow:opacity-100"
          >
            <ArrowUpRight className="size-3" />
          </IconButton>
        )}
      </div>

      {hasReason && (
        <div
          className="orbit-collapse-grid"
          data-open={expanded ? "true" : "false"}
          aria-hidden={!expanded}
          inert={!expanded}
        >
          <div className="orbit-collapse-inner">
            <p className="text-ink-tertiary selectable-text pt-1 pl-[22px] text-[11px] leading-[1.45] break-words">
              {entry.reason}
            </p>
          </div>
        </div>
      )}
    </div>
  );
});

const BriefingWaitRow = memo(function BriefingWaitRow({
  item,
  hostDetails,
  paContext,
  showHostBadge,
}: {
  item: BriefingWaitItem;
  hostDetails: Readonly<Record<string, HostDetailView>>;
  paContext: { hostId: string; agentId: string };
  showHostBadge: boolean;
}) {
  const isOwnAgent = item.host_id === paContext.hostId && item.agent_id === paContext.agentId;
  const hostName = showHostBadge
    ? (hostDetails[item.host_id]?.host.display_name ?? null)
    : null;
  const ownerTitle = !isOwnAgent ? item.title : null;
  const contextLabel = [ownerTitle, hostName].filter(Boolean).join(" · ") || null;

  const liveGoal = findLiveBriefingGoal(item, hostDetails);
  if (liveGoal) {
    return (
      <OverviewGoalRow
        hostId={item.host_id}
        goal={liveGoal}
        contextLabel={contextLabel}
        onSelect={
          isOwnAgent
            ? undefined
            : (e) => openWorkerSurface(e, item.host_id, item.agent_id, item.title, paContext)
        }
      />
    );
  }

  const liveTimer = findLiveBriefingTimer(item, hostDetails);
  if (liveTimer) {
    return (
      <ArmedTimerRow
        hostId={item.host_id}
        timer={liveTimer}
        contextLabel={contextLabel}
        onSelect={(e) => {
          if (isOwnAgent) {
            orbitActions.openAutomations();
          } else {
            openWorkerSurface(e, item.host_id, item.agent_id, item.title, paContext);
          }
        }}
      />
    );
  }

  const isOwnTimer = item.kind === "timer" && isOwnAgent;
  return (
    <button
      type="button"
      onClick={(e) => {
        if (isOwnTimer) {
          orbitActions.openAutomations();
        } else if (!isOwnAgent) {
          openWorkerSurface(e, item.host_id, item.agent_id, item.title, paContext);
        }
      }}
      className="hover:bg-accent/70 flex w-full cursor-pointer items-start gap-2 rounded-[8px] px-2 py-1.5 text-left transition-colors"
    >
      <span className="mt-[2px] flex size-[14px] shrink-0 items-center justify-center">
        {item.kind === "timer" ? (
          <AlarmClock className="text-signal-info size-[13px] stroke-[1.75]" />
        ) : item.kind === "goal" ? (
          <Target className="text-signal-success size-[13px] stroke-[1.75]" />
        ) : (
          <Clock className="text-ink-muted size-[13px] stroke-[1.75]" />
        )}
      </span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center justify-between gap-1.5">
          <span className="text-ink-primary min-w-0 flex-1 truncate text-[13px] leading-[18px]">
            {item.title}
          </span>
          {typeof item.due_at_ms === "number" && item.due_at_ms > 0 && (
            <WaitCountdownChip dueAtMs={item.due_at_ms} />
          )}
        </span>
        {item.detail && (
          <span className="text-ink-tertiary block truncate text-[11px] leading-[15px]">
            {item.detail}
          </span>
        )}
      </span>
    </button>
  );
});

const HostsChipList = memo(function HostsChipList({
  hosts,
}: {
  hosts: readonly BriefingHost[];
}) {
  const { t } = useI18n();
  if (hosts.length === 0) return null;

  return (
    <div className="flex flex-wrap gap-1 px-1 pb-1">
      {hosts.map((host) => (
        <SimpleTooltip
          key={host.host_id}
          label={
            host.is_personal_agent_host
              ? t("briefing.paHostTooltip", { host: host.name })
              : `${host.name} · ${t(host.online ? "common.online" : "common.offline")}`
          }
          side="top"
        >
          <span
            className={cn(
              "border-border-subtle inline-flex max-w-[132px] items-center gap-1.5 rounded-[6px] border px-2 py-0.5 text-[11px] leading-[16px]",
              host.is_personal_agent_host
                ? "bg-surface-recessed text-ink-primary font-medium"
                : "bg-surface-base/60 text-ink-secondary",
            )}
          >
            <span
              aria-hidden="true"
              className={cn(
                "size-[6px] shrink-0 rounded-full",
                host.online ? "bg-signal-success" : "bg-ink-muted/50",
              )}
            />
            <span className="truncate">{host.name}</span>
            {host.is_personal_agent_host && (
              <Bot className="text-ink-tertiary size-[10px] shrink-0" />
            )}
          </span>
        </SimpleTooltip>
      ))}
    </div>
  );
});

const BriefingRailBody = memo(function BriefingRailBody({
  hostId,
  agentId,
}: {
  hostId: string;
  agentId: string;
}) {
  const { t } = useI18n();
  const overviewShortcut = useActionShortcutDisplay("toggle_overview_hud");
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const collapsedSections = useOrbitSelector((s) => s.overviewCollapsedSections);
  const { briefing, activity, refresh } = usePersonalAgentBriefing({
    enabled: true,
    includeActivity: true,
  });

  const [busyDecisionKeys, setBusyDecisionKeys] = useState<Record<string, true>>({});
  const [batchApproving, setBatchApproving] = useState(false);
  const [showAllInProgress, setShowAllInProgress] = useState(false);
  const [showAllWaiting, setShowAllWaiting] = useState(false);
  const [showAllCompleted, setShowAllCompleted] = useState(false);
  const [showAllActivity, setShowAllActivity] = useState(false);

  useEffect(() => {
    setShowAllInProgress(false);
    setShowAllWaiting(false);
    setShowAllCompleted(false);
    setShowAllActivity(false);
  }, [agentId]);

  const needsYou = briefing?.needs_you ?? [];
  const missions = briefing?.missions ?? [];
  const otherActive = briefing?.other_active ?? [];
  const waiting = briefing?.waiting ?? [];
  const hosts = briefing?.hosts ?? [];
  const paContext = useMemo(() => ({ hostId, agentId }), [hostId, agentId]);
  const showHostBadges = hosts.length > 1;
  // A single-host fleet gains nothing from a host strip; show it only for 2+ hosts.
  const showHostsStrip = hosts.length > 1;

  const partitioned = useMemo(
    () =>
      partitionBriefingWorkers({
        missions,
        otherActive,
        needsYou,
        hostDetails,
        archivedThreads,
      }),
    [missions, otherActive, needsYou, hostDetails, archivedThreads],
  );

  const characterByAgentKey = useMemo(() => {
    const unique = new Map<string, { key: string; agentId: string }>();
    const add = (hid: string, aid: string) => {
      const k = briefingWorkerKey(hid, aid);
      if (!unique.has(k)) unique.set(k, { key: k, agentId: aid });
    };
    for (const d of needsYou) add(d.host_id, d.agent_id);
    for (const m of missions) {
      add(m.root.host_id, m.root.agent_id);
      for (const w of m.workers) add(w.host_id, w.agent_id);
    }
    for (const w of otherActive) add(w.host_id, w.agent_id);
    return assignSubagentCharacters([...unique.values()], (entry) => entry.agentId);
  }, [needsYou, missions, otherActive]);

  const batchEligible = useMemo(
    () => needsYou.filter((d) => d.batch_eligible && d.kind === "permission" && Boolean(d.request_id)),
    [needsYou],
  );

  const handleResolveDecision = useCallback(
    async (decision: BriefingDecision, mode: "allow" | "deny") => {
      if (!decision.request_id) return;
      const snap = orbitActions.getSnapshot();
      const pending = snap.hostDetails[decision.host_id]?.pending_permissions?.find(
        (p) => p.agent_id === decision.agent_id && p.request_id === decision.request_id,
      );
      const decisions =
        pending && pending.allowed_decisions.length > 0
          ? pending.allowed_decisions
          : ["allow", "deny"];
      const choice =
        mode === "allow"
          ? (decisions.find((d) => !isDenyDecision(d) && !isAlwaysDecision(d)) ?? "allow")
          : (decisions.find((d) => isDenyDecision(d)) ?? "deny");

      setBusyDecisionKeys((prev) => ({ ...prev, [decision.key]: true }));
      try {
        await orbitIpc.resolvePermission(
          decision.host_id,
          decision.agent_id,
          decision.request_id,
          choice,
        );
        void orbitActions.reloadHostDetail(decision.host_id, true);
        refresh({ includeActivity: true });
      } catch (err) {
        orbitActions.showToast(
          t("approval.permissionFailed", {
            error: err instanceof Error ? err.message : String(err),
          }),
        );
      } finally {
        setBusyDecisionKeys((prev) => {
          const next = { ...prev };
          delete next[decision.key];
          return next;
        });
      }
    },
    [refresh, t],
  );

  const handleBatchApproveLowRisk = useCallback(async () => {
    if (batchApproving || batchEligible.length === 0) return;
    setBatchApproving(true);
    try {
      const byHost = new Map<string, Array<[string, string]>>();
      for (const item of batchEligible) {
        if (!item.request_id) continue;
        const list = byHost.get(item.host_id) ?? [];
        list.push([item.agent_id, item.request_id]);
        byHost.set(item.host_id, list);
      }
      let resolvedTotal = 0;
      let skippedTotal = 0;
      for (const [hid, entries] of byHost.entries()) {
        const res = await orbitIpc.resolvePermissionsBatch(hid, entries, "allow");
        resolvedTotal += res.resolved_count;
        skippedTotal += res.skipped_count;
        void orbitActions.reloadHostDetail(hid, true);
      }
      refresh({ includeActivity: true });
      if (resolvedTotal > 0) {
        orbitActions.showToast(
          skippedTotal > 0
            ? t("briefing.batchApprovedWithSkipped", {
                approved: resolvedTotal,
                skipped: skippedTotal,
              })
            : t("briefing.batchApprovedToast", { count: resolvedTotal }),
        );
      }
    } catch (err) {
      orbitActions.showToast(
        t("approval.permissionFailed", {
          error: err instanceof Error ? err.message : String(err),
        }),
      );
    } finally {
      setBatchApproving(false);
    }
  }, [batchApproving, batchEligible, refresh, t]);

  const handleArchiveCompletedWorkers = useCallback(() => {
    for (const item of partitioned.archivableCompletedWorkers) {
      const liveAg = hostDetails[item.worker.host_id]?.agents.find(
        (a) => a.agent_id === item.worker.agent_id,
      );
      if (liveAg && !liveAg.parent_agent_id && liveAg.labels?.["orbit.session_tier"] !== "subagent") {
        void orbitActions.harvestConversation(item.worker.host_id, item.worker.agent_id, {
          appendToWorkspaceBoard: true,
        });
      } else {
        orbitActions.archiveThread(item.worker.host_id, item.worker.agent_id);
      }
    }
    refresh({ includeActivity: false });
  }, [hostDetails, partitioned.archivableCompletedWorkers, refresh]);

  const allInProgressWorkers = useMemo(
    () => [...partitioned.runningMissionWorkers, ...partitioned.runningOtherWorkers],
    [partitioned.runningMissionWorkers, partitioned.runningOtherWorkers],
  );
  const visibleInProgress = showAllInProgress
    ? allInProgressWorkers
    : allInProgressWorkers.slice(0, DEFAULT_VISIBLE_WORKERS);
  const hiddenInProgressCount = Math.max(
    0,
    allInProgressWorkers.length - visibleInProgress.length,
  );
  const visibleRunningMission = visibleInProgress.filter((w) => !w.fromOtherActive);
  const visibleRunningOther = visibleInProgress.filter((w) => w.fromOtherActive);

  const visibleWaiting = showAllWaiting ? waiting : waiting.slice(0, DEFAULT_VISIBLE_WAITING);
  const hiddenWaitingCount = Math.max(0, waiting.length - visibleWaiting.length);

  const waitingMeta = useMemo(() => {
    let hasActiveGoal = false;
    let hasPausedGoal = false;
    for (const item of waiting) {
      if (item.kind !== "goal") continue;
      const liveGoal = findLiveBriefingGoal(item, hostDetails);
      if (liveGoal?.status === "paused" || (!liveGoal && /\bpaused\b/i.test(item.detail))) {
        hasPausedGoal = true;
      } else {
        hasActiveGoal = true;
      }
    }
    return { hasActiveGoal, hasPausedGoal };
  }, [waiting, hostDetails]);

  const visibleCompleted = showAllCompleted
    ? partitioned.completedWorkers
    : partitioned.completedWorkers.slice(0, DEFAULT_VISIBLE_WORKERS);
  const hiddenCompletedCount = Math.max(
    0,
    partitioned.completedWorkers.length - visibleCompleted.length,
  );

  const visibleActivity = showAllActivity
    ? activity
    : activity.slice(0, DEFAULT_VISIBLE_ACTIVITY);
  const hiddenActivityCount = Math.max(0, activity.length - visibleActivity.length);

  const hasAnyWorkSection =
    needsYou.length > 0 ||
    partitioned.inProgressCount > 0 ||
    waiting.length > 0 ||
    partitioned.completedWorkers.length > 0 ||
    activity.length > 0;

  const hostsOnlineCount =
    briefing?.counts.hosts_online ?? hosts.filter((h) => h.online).length;
  const hostsTotalCount = briefing?.counts.hosts_total ?? hosts.length;

  return (
    <>
      <StageRailHeader
        title={t("briefing.title")}
        leading={<Bot className="text-ink-tertiary size-[13px] stroke-[1.8]" />}
        actions={
          <IconButton
            size="xs"
            tooltip={t("briefing.openSettings")}
            aria-label={t("briefing.openSettings")}
            onClick={() => orbitActions.openSettings("personal_router")}
            className="text-ink-muted hover:text-ink-primary"
          >
            <Settings2 className="size-3 stroke-[1.85]" />
          </IconButton>
        }
        closeTooltip={t("briefing.hideBriefing")}
        closeShortcut={overviewShortcut}
        onClose={() => orbitActions.toggleOverviewPanel()}
      />

      <StageRailScrollBody>
        {/* 1. Needs You (Blocking approvals, plan reviews, and session errors) */}
        {needsYou.length > 0 && (
          <OverviewSection
            id="briefing_needs_you"
            label={t("briefing.needsYouTitle")}
            count={needsYou.length}
            collapsed={Boolean(collapsedSections.briefing_needs_you)}
            tone="warning"
            hasAlert
            headerActionVisibility="always"
            headerAction={
              batchEligible.length > 1 ? (
                <button
                  type="button"
                  disabled={batchApproving}
                  onClick={() => void handleBatchApproveLowRisk()}
                  className="text-ink-secondary hover:text-ink-primary hover:bg-accent inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-[6px] px-1.5 text-[11px] font-medium transition-colors disabled:opacity-45"
                >
                  {batchApproving ? (
                    <OrbitSpinner size={10} />
                  ) : (
                    <CheckCheck className="size-3" />
                  )}
                  <span>{t("briefing.approveLowRiskBatch", { count: batchEligible.length })}</span>
                </button>
              ) : undefined
            }
          >
            <div className="flex flex-col gap-1.5 pb-1">
              {needsYou.map((decision) => (
                <DecisionCard
                  key={decision.key}
                  decision={decision}
                  agentContext={resolveBriefingAgentContext(
                    hostDetails,
                    characterByAgentKey,
                    decision.host_id,
                    decision.agent_id,
                  )}
                  paContext={paContext}
                  showHostBadge={showHostBadges}
                  busy={Boolean(busyDecisionKeys[decision.key])}
                  onResolve={(d, mode) => void handleResolveDecision(d, mode)}
                />
              ))}
            </div>
          </OverviewSection>
        )}

        {/* 2. In Progress (Strictly actively executing workers — aligned with Overview Subagents) */}
        {partitioned.inProgressCount > 0 && (
          <OverviewSection
            id="briefing_in_progress"
            label={t("briefing.inProgressTitle")}
            count={partitioned.inProgressCount}
            collapsed={Boolean(collapsedSections.briefing_in_progress)}
            isRunning
            runningTone="spinner"
            footer={
              allInProgressWorkers.length > DEFAULT_VISIBLE_WORKERS ? (
                <StageRailExpandFooter
                  expanded={showAllInProgress}
                  hiddenCount={hiddenInProgressCount}
                  onToggle={() => setShowAllInProgress((v) => !v)}
                />
              ) : undefined
            }
          >
            <div className="flex flex-col gap-0.5 pb-1">
              {visibleRunningMission.map((item) => (
                <BriefingWorkerRow
                  key={item.key}
                  item={item}
                  agentContext={resolveBriefingAgentContext(
                    hostDetails,
                    characterByAgentKey,
                    item.worker.host_id,
                    item.worker.agent_id,
                  )}
                  paContext={paContext}
                  showHostBadge={showHostBadges}
                />
              ))}
              {visibleRunningOther.length > 0 && (
                <div
                  className={cn(
                    "flex flex-col gap-0.5",
                    visibleRunningMission.length > 0 &&
                      "border-border-subtle mt-1 border-t pt-1.5",
                  )}
                >
                  {visibleRunningMission.length > 0 && (
                    <span className="text-ink-faint px-2 pb-0.5 text-[11px] font-medium">
                      {t("briefing.otherActiveLabel")}
                    </span>
                  )}
                  {visibleRunningOther.map((item) => (
                    <BriefingWorkerRow
                      key={item.key}
                      item={item}
                      agentContext={resolveBriefingAgentContext(
                        hostDetails,
                        characterByAgentKey,
                        item.worker.host_id,
                        item.worker.agent_id,
                      )}
                      paContext={paContext}
                      showHostBadge={showHostBadges}
                    />
                  ))}
                </div>
              )}
            </div>
          </OverviewSection>
        )}

        {/* 3. Waiting (Active Goals, Armed Timers, Background Tasks — reuses OverviewGoalRow & ArmedTimerRow) */}
        {waiting.length > 0 && (
          <OverviewSection
            id="briefing_waiting"
            label={t("briefing.waitingTitle")}
            count={waiting.length}
            collapsed={Boolean(collapsedSections.briefing_waiting)}
            isRunning
            hasAlert={waitingMeta.hasPausedGoal}
            runningTone={waitingMeta.hasActiveGoal ? "emerald" : "timer"}
            footer={
              waiting.length > DEFAULT_VISIBLE_WAITING ? (
                <StageRailExpandFooter
                  expanded={showAllWaiting}
                  hiddenCount={hiddenWaitingCount}
                  onToggle={() => setShowAllWaiting((v) => !v)}
                />
              ) : undefined
            }
          >
            <div className="flex flex-col gap-0.5 pb-1">
              {visibleWaiting.map((item) => (
                <BriefingWaitRow
                  key={item.key}
                  item={item}
                  hostDetails={hostDetails}
                  paContext={paContext}
                  showHostBadge={showHostBadges}
                />
              ))}
            </div>
          </OverviewSection>
        )}

        {/* 4. Completed (Settled / stopped / idle PA workers with hover Archive done matching Overview) */}
        {partitioned.completedWorkers.length > 0 && (
          <OverviewSection
            id="briefing_completed"
            label={t("briefing.completedTitle")}
            count={partitioned.completedWorkers.length}
            collapsed={Boolean(collapsedSections.briefing_completed)}
            headerActionVisibility="hover"
            headerAction={
              partitioned.archivableCompletedWorkers.length > 0 ? (
                <button
                  type="button"
                  onClick={handleArchiveCompletedWorkers}
                  className="text-ink-tertiary hover:text-ink-primary cursor-pointer rounded-[6px] px-1.5 py-0.5 text-[11px] font-medium transition-colors"
                >
                  {t("overview.archiveDone")}
                </button>
              ) : undefined
            }
            footer={
              partitioned.completedWorkers.length > DEFAULT_VISIBLE_WORKERS ? (
                <StageRailExpandFooter
                  expanded={showAllCompleted}
                  hiddenCount={hiddenCompletedCount}
                  onToggle={() => setShowAllCompleted((v) => !v)}
                />
              ) : undefined
            }
          >
            <div className="flex flex-col gap-0.5 pb-1">
              {visibleCompleted.map((item) => (
                <BriefingWorkerRow
                  key={item.key}
                  item={item}
                  agentContext={resolveBriefingAgentContext(
                    hostDetails,
                    characterByAgentKey,
                    item.worker.host_id,
                    item.worker.agent_id,
                  )}
                  paContext={paContext}
                  showHostBadge={showHostBadges}
                />
              ))}
            </div>
          </OverviewSection>
        )}

        {/* 5. Handled For You (PA / Guardian / Permission Rule automatic decisions) */}
        {activity.length > 0 && (
          <OverviewSection
            id="briefing_handled"
            label={t("briefing.handledForYouTitle")}
            count={activity.length}
            collapsed={Boolean(collapsedSections.briefing_handled)}
            footer={
              activity.length > DEFAULT_VISIBLE_ACTIVITY ? (
                <StageRailExpandFooter
                  expanded={showAllActivity}
                  hiddenCount={hiddenActivityCount}
                  onToggle={() => setShowAllActivity((v) => !v)}
                />
              ) : undefined
            }
          >
            <div className="flex flex-col gap-0.5 pb-1">
              {visibleActivity.map((entry) => (
                <ActivityLogRow
                  key={`${entry.seq}:${entry.timestamp_ms}`}
                  entry={entry}
                  paContext={paContext}
                />
              ))}
            </div>
          </OverviewSection>
        )}

        {!hasAnyWorkSection && !showHostsStrip && (
          <StageRailEmptyState>{t("briefing.allQuiet")}</StageRailEmptyState>
        )}

        {/* 6. Hosts (only meaningful when coordinating 2+ hosts) */}
        {showHostsStrip && (
          <OverviewSection
            id="briefing_hosts"
            label={t("briefing.hostsTitle")}
            collapsed={Boolean(collapsedSections.briefing_hosts)}
            trailingMeta={
              <span className="text-ink-secondary font-mono text-[11px] tabular-nums">
                {t("briefing.hostsOnlineSummary", {
                  online: hostsOnlineCount,
                  total: hostsTotalCount,
                })}
              </span>
            }
          >
            <HostsChipList hosts={hosts} />
          </OverviewSection>
        )}
      </StageRailScrollBody>
    </>
  );
});

export interface PersonalAgentBriefingRailProps {
  canDock: boolean;
  hostId: string;
  agentId: string;
  stageContainerRef?: RefObject<HTMLDivElement | null>;
}

/**
 * 304px Personal Agent Briefing Rail:
 * Composes the exact same `StageRailShell`, `StageRailHeader`, `StageRailScrollBody`,
 * `OverviewSection` (`StageRailSection`), `StageRailExpandFooter`, `AgentListItem`,
 * `OverviewGoalRow`, and `ArmedTimerRow` primitives as `StageOverviewPanel`.
 */
export const PersonalAgentBriefingRail = memo(function PersonalAgentBriefingRail({
  canDock,
  hostId,
  agentId,
  stageContainerRef,
}: PersonalAgentBriefingRailProps) {
  const { t } = useI18n();
  const ariaLabel = t("briefing.ariaLabel");

  const bodyNode = useMemo(
    () => <BriefingRailBody hostId={hostId} agentId={agentId} />,
    [hostId, agentId],
  );

  return (
    <StageRailShell
      canDock={canDock}
      ariaLabel={ariaLabel}
      stageContainerRef={stageContainerRef}
    >
      {bodyNode}
    </StageRailShell>
  );
});

/**
 * Dismissible 1-line "While you were away" summary shown at the top of the PA conversation
 * whenever `since_last_visit` has completed tasks or auto-handled decisions.
 */
export const PersonalAgentAwayBanner = memo(function PersonalAgentAwayBanner({
  summary,
  onDismiss,
}: {
  summary: SinceLastVisitSummary;
  onDismiss: () => void;
}) {
  const { t } = useI18n();
  if (!summary.hasSummary) return null;

  const message =
    summary.completed > 0 && summary.autoHandled > 0
      ? t("briefing.whileAwayBoth", {
          completed: summary.completed,
          handled: summary.autoHandled,
        })
      : summary.completed > 0
        ? t("briefing.whileAwayCompletedOnly", { completed: summary.completed })
        : t("briefing.whileAwayHandledOnly", { handled: summary.autoHandled });

  return (
    <div className="mx-auto mt-2.5 w-full max-w-[var(--orbit-stage-max-w,740px)]">
      <ContextBanner
        variant="inline"
        tone="neutral"
        icon={<Bot className="size-[13px] stroke-[1.8]" />}
        onDismiss={onDismiss}
        dismissLabel={t("common.close")}
      >
        <span className="text-[13px]">{message}</span>
      </ContextBanner>
    </div>
  );
});

/**
 * Self-contained stage header slot for the Personal Agent conversation so briefing updates
 * never force `StreamStage` to re-render.
 */
export const PersonalAgentStageAwayBanner = memo(function PersonalAgentStageAwayBanner({
  hostId,
  agentId,
  overviewReservePx,
  overviewReserveTransition,
  stagePushStyle,
}: {
  hostId: string;
  agentId: string;
  overviewReservePx: number;
  overviewReserveTransition: string;
  stagePushStyle: CSSProperties;
}) {
  const scopeKey = `${hostId}::${agentId}`;
  const { briefing, activity } = usePersonalAgentBriefing({
    enabled: true,
    includeActivity: true,
  });
  const { summary, dismissed, dismiss } = usePersonalAgentAwayBanner(
    scopeKey,
    briefing,
    activity,
  );

  if (dismissed || !summary.hasSummary) return null;

  return (
    <div
      style={{
        paddingRight: `${24 + overviewReservePx}px`,
        transition: overviewReserveTransition,
      }}
      className="shrink-0 pl-[24px]"
    >
      <div style={stagePushStyle}>
        <PersonalAgentAwayBanner summary={summary} onDismiss={dismiss} />
      </div>
    </div>
  );
});
