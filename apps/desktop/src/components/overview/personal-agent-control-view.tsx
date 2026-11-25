import { memo, useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowUp,
  ArrowUpRight,
  Check,
  CheckCheck,
  Columns2,
  Eye,
  FileText,
  GitBranch,
  ShieldAlert,
  Sparkles,
  X,
} from "lucide-react";
import { resolveComposerEnterAction } from "@/components/composer/omni-composer";
import { AgentStagePane } from "@/components/stage/agent-stage-pane";
import { Button } from "@/components/ui/button";
import { DiffStat } from "@/components/ui/diff-stat";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { ScrollArea } from "@/components/ui/scroll-area";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { useLiveSecondClock } from "@/lib/live-clock";
import {
  type AgentCardView,
  type HostCardView,
  type PendingPermissionSummary,
  type ToolCallDetail,
  isAgentForegroundRunning,
  isAgentTurnBusy,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isAlwaysDecision, isDenyDecision } from "@/lib/timeline-coalesce";
import { cn } from "@/lib/utils";
import {
  orbitActions,
  threadScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";

interface TriageDecisionItem {
  key: string;
  kind: "permission" | "plan" | "error";
  hostId: string;
  hostName: string;
  agentId: string;
  agentTitle: string;
  agent: AgentCardView;
  permission?: PendingPermissionSummary;
  summaryText: string;
  riskLevel: string | null;
  humanOnly: boolean;
  lowRiskBatchEligible: boolean;
  updatedAtMs: number;
}

interface FleetSessionRowItem {
  key: string;
  hostId: string;
  hostName: string;
  agent: AgentCardView;
  effectiveTitle: string;
  isRunning: boolean;
  isAwaiting: boolean;
  isError: boolean;
  isPinnedLane: boolean;
  updatedAtMs: number;
  additions: number;
  deletions: number;
  stepPreview: string;
}

function describeToolDetailPlainly(detail: ToolCallDetail | undefined, fallbackTitle: string): string {
  if (!detail) return fallbackTitle;
  switch (detail.kind) {
    case "shell":
      return detail.command ? `Run command: ${detail.command}` : fallbackTitle;
    case "file_edit":
      return detail.path ? `Edit file: ${detail.path}` : fallbackTitle;
    case "file_read":
      return detail.path ? `Read file: ${detail.path}` : fallbackTitle;
    case "search":
      return detail.query ? `Search: ${detail.query}` : fallbackTitle;
    case "subagent":
      return detail.role ? `Launch specialist: ${detail.role}` : fallbackTitle;
    case "generic":
      return detail.tool_name ? `${detail.tool_name}: ${fallbackTitle}` : fallbackTitle;
    default:
      return fallbackTitle;
  }
}

function formatCompactElapsed(tsMs: number, nowMs: number): string {
  if (!tsMs || !Number.isFinite(tsMs)) return "";
  const sec = Math.max(0, Math.floor((nowMs - tsMs) / 1000));
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h`;
  return `${Math.floor(hr / 24)}d`;
}

function isAgentErrored(agent: AgentCardView): boolean {
  const lbl = (agent.status_label || "").toLowerCase();
  return lbl.includes("error") || lbl.includes("fail");
}

function parseTsMs(rfc3339?: string | null): number {
  if (!rfc3339) return 0;
  const parsed = Date.parse(rfc3339);
  return Number.isFinite(parsed) ? parsed : 0;
}

/**
 * Humanized Personal Agent Global Overview ("全局总控视角").
 *
 * Replaces separate confusing Inbox + Mission Grid modes with one calm, human-centered deck:
 * 1. Calm Top Status Line (1 quiet sentence, zero noisy KPI cards)
 * 2. Needs Your Decision (0px when clear; inline 1-click Approve/Deny/Review + safe low-risk batch)
 * 3. On-Demand Live Watch Lanes (2–4 pinned `<AgentStagePane />` side-by-side when requested)
 * 4. Running & Recent Fleet (scannable cross-host ledger with instant Watch Live & Open)
 * 5. Docked Bottom Personal Agent Prompt (exact `max-w-[740px]` reading rhythm)
 */
export const PersonalAgentControlView = memo(function PersonalAgentControlView() {
  const { t } = useI18n();
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const stages = useOrbitSelector((s) => s.stages);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const threadCustomTitles = useOrbitSelector((s) => s.threadCustomTitles);
  const overviewPinnedLanes = useOrbitSelector((s) => s.overviewPinnedLanes);
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const activeTurnSendBehavior = useOrbitSelector((s) => s.activeTurnSendBehavior);

  const [busyDecisionKeys, setBusyDecisionKeys] = useState<Record<string, true>>({});
  const [batchApproving, setBatchApproving] = useState(false);
  const [routerPrompt, setRouterPrompt] = useState("");
  const [sendingRouter, setSendingRouter] = useState(false);
  const [primaryRouterHostId, setPrimaryRouterHostId] = useState<string | null>(null);
  const routerIme = useImeCompositionGuard();

  const hasAnyActive = useMemo(() => {
    for (const detail of Object.values(hostDetails)) {
      if (detail.agents.some((a) => !a.archived_at_rfc3339 && isAgentForegroundRunning(a))) {
        return true;
      }
    }
    return false;
  }, [hostDetails]);

  const nowMs = useLiveSecondClock(hasAnyActive);

  useEffect(() => {
    let active = true;
    void orbitIpc
      .getPrimaryRouterSelection()
      .then((sel) => {
        if (active && sel?.host_id) setPrimaryRouterHostId(sel.host_id);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, [fleet.hosts.length]);

  const hostById = useMemo(() => {
    const map = new Map<string, HostCardView>();
    for (const h of fleet.hosts) map.set(h.host_id, h);
    return map;
  }, [fleet.hosts]);

  // Resolve Personal Agent (Router) across fleet
  const activePersonalAgent = useMemo(() => {
    const orderedHosts = primaryRouterHostId
      ? [
          ...fleet.hosts.filter((h) => h.host_id === primaryRouterHostId),
          ...fleet.hosts.filter((h) => h.host_id !== primaryRouterHostId),
        ]
      : fleet.hosts;

    for (const h of orderedHosts) {
      const detail = hostDetails[h.host_id];
      if (!detail) continue;
      const found = detail.agents.find(
        (a) =>
          !a.archived_at_rfc3339 &&
          !archivedThreads[threadScopeKey(h.host_id, a.agent_id)] &&
          (a.handle === "router" || a.title.includes("Personal Router")),
      );
      if (found) {
        return {
          hostId: h.host_id,
          hostName: h.display_name,
          agent: found,
        };
      }
    }
    return null;
  }, [fleet.hosts, hostDetails, archivedThreads, primaryRouterHostId]);

  const pinnedLaneKeySet = useMemo(() => {
    const set = new Set<string>();
    for (const lane of overviewPinnedLanes) {
      set.add(threadScopeKey(lane.hostId, lane.agentId));
    }
    return set;
  }, [overviewPinnedLanes]);

  // 1. Unified Needs-Your-Decision triage list + 2. Running & Recent Fleet
  const { triageItems, fleetSessions, activeCount, onlineHostCount } = useMemo(() => {
    const decisions: TriageDecisionItem[] = [];
    const sessions: FleetSessionRowItem[] = [];
    const seenPermAgentKeys = new Set<string>();
    let activeSessions = 0;

    let onlineHosts = 0;
    for (const h of fleet.hosts) {
      if (h.connection_state === "online_direct" || h.connection_state === "online_relay") {
        onlineHosts += 1;
      }
    }

    for (const [hostId, detail] of Object.entries(hostDetails)) {
      const host = hostById.get(hostId) ?? detail.host;
      const hostName = host?.display_name || hostId;
      const agentById = new Map<string, AgentCardView>();
      for (const ag of detail.agents) {
        agentById.set(ag.agent_id, ag);
      }

      // 1a. Explicit pending permission & plan approval items across all hosts
      for (const perm of detail.pending_permissions ?? []) {
        const ag = agentById.get(perm.agent_id);
        if (!ag || ag.archived_at_rfc3339) continue;
        const tKey = threadScopeKey(hostId, ag.agent_id);
        if (archivedThreads[tKey]) continue;
        seenPermAgentKeys.add(tKey);

        const isPlan = Boolean(perm.plan_approval);
        const humanOnly = Boolean(perm.human_only);
        const riskLevel = perm.guardian_assessment?.risk_level ?? null;
        const isLowRisk =
          !isPlan &&
          !humanOnly &&
          (!riskLevel || riskLevel.toLowerCase() === "low");

        const agentTitle = threadCustomTitles[tKey] || ag.title || perm.agent_title || ag.agent_id;
        const summaryText = isPlan
          ? perm.plan_approval?.plan_title ||
            perm.plan_approval?.summary ||
            perm.title ||
            t("overview.planWaitingReview")
          : describeToolDetailPlainly(perm.detail, perm.title);

        decisions.push({
          key: `perm:${hostId}:${ag.agent_id}:${perm.request_id}`,
          kind: isPlan ? "plan" : "permission",
          hostId,
          hostName,
          agentId: ag.agent_id,
          agentTitle,
          agent: ag,
          permission: perm,
          summaryText,
          riskLevel,
          humanOnly,
          lowRiskBatchEligible: isLowRisk,
          updatedAtMs: parseTsMs(perm.created_at) || parseTsMs(ag.updated_at_rfc3339),
        });
      }

      // 1b. Agents in error / awaiting-without-summary + all visible sessions for Fleet ledger
      for (const ag of detail.agents) {
        if (ag.background || ag.archived_at_rfc3339) continue;
        const tKey = threadScopeKey(hostId, ag.agent_id);
        if (archivedThreads[tKey]) continue;

        const effectiveTitle = threadCustomTitles[tKey] || ag.title || ag.provider_label;
        const isRunning = isAgentForegroundRunning(ag) || Boolean(ag.has_running_subagents);
        const isAwaiting = Boolean(ag.is_awaiting_permission);
        const isError = isAgentErrored(ag);
        const updatedAtMs = parseTsMs(ag.updated_at_rfc3339);

        if (isRunning) activeSessions += 1;

        if (isAwaiting && !seenPermAgentKeys.has(tKey)) {
          decisions.push({
            key: `await:${tKey}`,
            kind: "permission",
            hostId,
            hostName,
            agentId: ag.agent_id,
            agentTitle: effectiveTitle,
            agent: ag,
            summaryText: ag.status_label || t("overview.waitingForDecision"),
            riskLevel: null,
            humanOnly: true,
            lowRiskBatchEligible: false,
            updatedAtMs,
          });
        } else if (isError) {
          decisions.push({
            key: `err:${tKey}`,
            kind: "error",
            hostId,
            hostName,
            agentId: ag.agent_id,
            agentTitle: effectiveTitle,
            agent: ag,
            summaryText: ag.task_summary || ag.status_label || t("overview.agentErrorSummary"),
            riskLevel: "high",
            humanOnly: true,
            lowRiskBatchEligible: false,
            updatedAtMs,
          });
        }

        // Skip same-host subagents from top-level fleet ledger (keep primary threads crisp)
        if (ag.parent_agent_id && !ag.parent_host_id) continue;

        const stage = stages[tKey];
        let additions = 0;
        let deletions = 0;
        for (const df of stage?.session_turn_diffs ?? []) {
          additions += df.additions;
          deletions += df.deletions;
        }
        const lastTurn = stage?.turns?.[stage.turns.length - 1];
        const stepPreview =
          (isRunning ? lastTurn?.work_summary_live_label : lastTurn?.work_summary_label) ||
          ag.task_summary ||
          ag.status_label ||
          "";

        sessions.push({
          key: tKey,
          hostId,
          hostName,
          agent: ag,
          effectiveTitle,
          isRunning,
          isAwaiting,
          isError,
          isPinnedLane: pinnedLaneKeySet.has(tKey),
          updatedAtMs,
          additions,
          deletions,
          stepPreview,
        });
      }
    }

    decisions.sort((a, b) => {
      const rank = (k: TriageDecisionItem["kind"]) => (k === "plan" ? 0 : k === "permission" ? 1 : 2);
      return rank(a.kind) - rank(b.kind) || b.updatedAtMs - a.updatedAtMs;
    });

    sessions.sort((a, b) => {
      const score = (s: FleetSessionRowItem) =>
        s.isAwaiting ? 0 : s.isRunning ? 1 : s.isError ? 2 : 3;
      return score(a) - score(b) || b.updatedAtMs - a.updatedAtMs;
    });

    return {
      triageItems: decisions,
      fleetSessions: sessions.slice(0, 24),
      activeCount: activeSessions,
      onlineHostCount: onlineHosts,
    };
  }, [
    fleet.hosts,
    hostDetails,
    hostById,
    stages,
    archivedThreads,
    threadCustomTitles,
    pinnedLaneKeySet,
    t,
  ]);

  const lowRiskBatchItems = useMemo(
    () => triageItems.filter((item) => item.lowRiskBatchEligible && item.permission),
    [triageItems],
  );

  // Resolve valid pinned live lanes
  const resolvedPinnedLanes = useMemo(() => {
    return overviewPinnedLanes.map((lane) => {
      const host = hostById.get(lane.hostId);
      const detail = hostDetails[lane.hostId];
      const ag = detail?.agents.find((a) => a.agent_id === lane.agentId);
      return {
        ...lane,
        key: threadScopeKey(lane.hostId, lane.agentId),
        hostName: host?.display_name ?? detail?.host.display_name ?? lane.hostId,
        title:
          threadCustomTitles[threadScopeKey(lane.hostId, lane.agentId)] ||
          ag?.title ||
          lane.agentId,
        cwd: ag?.cwd ?? "",
      };
    });
  }, [overviewPinnedLanes, hostById, hostDetails, threadCustomTitles]);

  const handleSinglePermission = useCallback(
    async (item: TriageDecisionItem, mode: "allow" | "deny") => {
      if (!item.permission) return;
      const decisions =
        item.permission.allowed_decisions.length > 0
          ? item.permission.allowed_decisions
          : ["allow", "deny"];
      const choice =
        mode === "allow"
          ? decisions.find((d) => !isDenyDecision(d) && !isAlwaysDecision(d)) ?? "allow"
          : decisions.find((d) => isDenyDecision(d)) ?? "deny";

      setBusyDecisionKeys((prev) => ({ ...prev, [item.key]: true }));
      try {
        await orbitIpc.resolvePermission(
          item.hostId,
          item.agentId,
          item.permission.request_id,
          choice,
        );
        void orbitActions.reloadHostDetail(item.hostId, true);
      } catch (err) {
        orbitActions.showToast(
          t("approval.permissionFailed", {
            error: err instanceof Error ? err.message : String(err),
          }),
        );
      } finally {
        setBusyDecisionKeys((prev) => {
          const next = { ...prev };
          delete next[item.key];
          return next;
        });
      }
    },
    [t],
  );

  const handleBatchApproveLowRisk = useCallback(async () => {
    if (batchApproving || lowRiskBatchItems.length === 0) return;
    setBatchApproving(true);
    try {
      const byHost = new Map<string, Array<[string, string]>>();
      for (const item of lowRiskBatchItems) {
        if (!item.permission) continue;
        const list = byHost.get(item.hostId) ?? [];
        list.push([item.agentId, item.permission.request_id]);
        byHost.set(item.hostId, list);
      }
      let resolvedTotal = 0;
      let skippedTotal = 0;
      for (const [hid, entries] of byHost.entries()) {
        const res = await orbitIpc.resolvePermissionsBatch(hid, entries, "allow");
        resolvedTotal += res.resolved_count;
        skippedTotal += res.skipped_count;
        void orbitActions.reloadHostDetail(hid, true);
      }
      if (resolvedTotal > 0) {
        orbitActions.showToast(
          skippedTotal > 0
            ? t("overview.batchApprovedWithSkipped", {
                approved: resolvedTotal,
                skipped: skippedTotal,
              })
            : t("overview.batchApprovedToast", { count: resolvedTotal }),
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
  }, [batchApproving, lowRiskBatchItems, t]);

  const handleSendToPersonalAgent = useCallback(async (steerOverride?: boolean) => {
    const trimmed = routerPrompt.trim();
    if (!trimmed || !activePersonalAgent || sendingRouter) return;
    setSendingRouter(true);
    setRouterPrompt("");
    try {
      const outcome = await orbitActions.dispatchDirectAgentPrompt(
        activePersonalAgent.hostId,
        activePersonalAgent.agent.agent_id,
        trimmed,
        {
          displayPrompt: trimmed,
          steerImmediately: steerOverride,
        },
      );
      orbitActions.showToast(
        outcome === "queued"
          ? t("overview.queuedForPersonalAgent")
          : t("overview.dispatchedToPersonalAgent"),
      );
    } catch (err) {
      setRouterPrompt((cur) => (cur.trim() ? cur : trimmed));
      orbitActions.showToast(
        err instanceof Error ? err.message : t("common.actionFailed"),
      );
    } finally {
      setSendingRouter(false);
    }
  }, [routerPrompt, activePersonalAgent, sendingRouter, t]);

  // Calm single-sentence fleet status line
  const calmStatusLine = useMemo(() => {
    const hostCount = Math.max(onlineHostCount, fleet.hosts.length);
    if (triageItems.length > 0) {
      return activeCount > 0
        ? t("overview.statusWaitingAndActive", {
            waiting: triageItems.length,
            active: activeCount,
          })
        : t("overview.statusWaitingOnly", { waiting: triageItems.length });
    }
    if (activeCount > 0) {
      return hostCount > 1
        ? t("overview.statusClearActiveMultiHost", {
            active: activeCount,
            hosts: hostCount,
          })
        : t("overview.statusClearActiveSingle", { active: activeCount });
    }
    return hostCount > 1
      ? t("overview.statusAllQuietMultiHost", { hosts: hostCount })
      : t("overview.statusAllQuiet");
  }, [triageItems.length, activeCount, onlineHostCount, fleet.hosts.length, t]);

  const showHostBadges = fleet.hosts.length > 1;
  const laneGridClass =
    resolvedPinnedLanes.length === 1
      ? "grid-cols-1"
      : resolvedPinnedLanes.length === 2
        ? "grid-cols-1 lg:grid-cols-2"
        : resolvedPinnedLanes.length === 3
          ? "grid-cols-1 lg:grid-cols-3"
          : "grid-cols-1 md:grid-cols-2 2xl:grid-cols-4";

  return (
    <div className="bg-surface-base flex min-h-0 flex-1 flex-col overflow-hidden">
      <ScrollArea className="min-h-0 flex-1" viewportClassName="px-6 py-6">
        <div className="flex flex-col gap-6">
          {/* Calm Top Status Line — exact 740px reading rhythm, zero KPI box clutter */}
          <div className="mx-auto flex w-full max-w-[740px] items-center justify-between gap-4">
            <div className="flex min-w-0 items-center gap-2.5">
              <span
                className={cn(
                  "size-[8px] shrink-0 rounded-full",
                  triageItems.length > 0
                    ? "bg-signal-warning"
                    : activeCount > 0
                      ? "bg-signal-success"
                      : "bg-ink-faint",
                )}
              />
              <h1 className="text-ink-primary truncate text-[15px] font-medium tracking-[-0.01em]">
                {calmStatusLine}
              </h1>
            </div>

            {resolvedPinnedLanes.length > 0 && (
              <button
                type="button"
                onClick={() => orbitActions.clearOverviewLanes()}
                className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex h-[26px] shrink-0 cursor-pointer items-center gap-1 rounded-[7px] px-2 text-[12px] transition-colors"
              >
                <X className="size-3" />
                <span>{t("overview.closeAllLanes")}</span>
              </button>
            )}
          </div>

          {/* Section 1 — Needs Your Decision (ONLY renders when > 0 items exist!) */}
          {triageItems.length > 0 && (
            <section
              aria-label={t("overview.needsDecisionTitle")}
              className="mx-auto flex w-full max-w-[740px] flex-col gap-2.5"
            >
              <div className="flex items-center justify-between gap-2 px-0.5">
                <div className="flex items-center gap-2">
                  <span className="text-signal-warning text-[11px] font-medium tracking-[0.04em] uppercase">
                    {t("overview.needsDecisionTitle")}
                  </span>
                  <span className="text-signal-warning/80 font-mono text-[11px] font-medium tabular-nums">
                    {triageItems.length}
                  </span>
                </div>

                {lowRiskBatchItems.length > 1 && (
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    disabled={batchApproving}
                    onClick={() => void handleBatchApproveLowRisk()}
                    className="h-[26px] gap-1.5 rounded-[7px] px-2.5 text-[12px]"
                  >
                    {batchApproving ? (
                      <OrbitSpinner size={12} />
                    ) : (
                      <CheckCheck className="size-3.5" />
                    )}
                    <span>
                      {t("overview.approveLowRiskBatch", {
                        count: lowRiskBatchItems.length,
                      })}
                    </span>
                  </Button>
                )}
              </div>

              <div className="flex flex-col gap-2">
                {triageItems.map((item) => {
                  const busy = Boolean(busyDecisionKeys[item.key]);
                  const isPlan = item.kind === "plan";
                  const isError = item.kind === "error";
                  const canQuickApprove =
                    item.kind === "permission" && Boolean(item.permission) && !item.humanOnly;

                  return (
                    <div
                      key={item.key}
                      className="border-border-default bg-surface-raised flex flex-col gap-2.5 rounded-xl border p-3.5 transition-colors hover:border-border-strong"
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="flex min-w-0 items-start gap-2.5">
                          <div className="mt-0.5 shrink-0">
                            {item.agent.parent_agent_id ? (
                              <SubagentAvatar
                                seedKey={item.agentId}
                                status="awaiting_permission"
                                size="sm"
                              />
                            ) : (
                              <ProviderIcon
                                provider={item.agent.provider}
                                className="size-[18px] opacity-85"
                              />
                            )}
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="flex flex-wrap items-center gap-1.5">
                              <span className="text-ink-primary truncate text-[13px] font-medium">
                                {item.agentTitle}
                              </span>
                              {showHostBadges && (
                                <span className="bg-surface-recessed text-ink-tertiary rounded px-1.5 py-0.5 text-[11px]">
                                  {item.hostName}
                                </span>
                              )}
                              {isPlan && (
                                <span className="bg-signal-warning-bg text-signal-warning inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium">
                                  <FileText className="size-3" />
                                  {t("overview.planReviewBadge")}
                                </span>
                              )}
                              {isError && (
                                <span className="bg-signal-danger-bg text-signal-danger inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium">
                                  <AlertTriangle className="size-3" />
                                  {t("overview.errorBadge")}
                                </span>
                              )}
                              {!isPlan &&
                                !isError &&
                                item.riskLevel &&
                                (item.riskLevel.toLowerCase() === "high" ||
                                  item.riskLevel.toLowerCase() === "critical") && (
                                  <span className="bg-signal-danger-bg text-signal-danger inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium">
                                    <ShieldAlert className="size-3" />
                                    {item.riskLevel}
                                  </span>
                                )}
                            </div>
                            <p className="text-ink-secondary mt-1 line-clamp-2 font-mono text-[12px] leading-[1.45] break-words">
                              {item.summaryText}
                            </p>
                          </div>
                        </div>

                        <span className="text-ink-faint shrink-0 font-mono text-[11px] tabular-nums">
                          {formatCompactElapsed(item.updatedAtMs, nowMs)}
                        </span>
                      </div>

                      <div className="flex flex-wrap items-center justify-end gap-1.5">
                        {canQuickApprove && (
                          <>
                            <Button
                              type="button"
                              size="sm"
                              variant="ghost"
                              disabled={busy}
                              onClick={() => void handleSinglePermission(item, "deny")}
                              className="text-ink-secondary hover:text-signal-danger h-[26px] rounded-[7px] px-2.5 text-[12px]"
                            >
                              {t("approval.deny")}
                            </Button>
                            <Button
                              type="button"
                              size="sm"
                              disabled={busy}
                              onClick={() => void handleSinglePermission(item, "allow")}
                              className="h-[26px] gap-1 rounded-[7px] px-2.5 text-[12px]"
                            >
                              {busy ? <OrbitSpinner size={11} /> : <Check className="size-3" />}
                              <span>{t("approval.allow")}</span>
                            </Button>
                          </>
                        )}

                        <Button
                          type="button"
                          size="sm"
                          variant={canQuickApprove ? "outline" : "default"}
                          onClick={() => orbitActions.selectAgent(item.hostId, item.agentId)}
                          className="h-[26px] gap-1 rounded-[7px] px-2.5 text-[12px]"
                        >
                          <span>
                            {isPlan ? t("overview.reviewPlan") : t("overview.openSession")}
                          </span>
                          <ArrowUpRight className="size-3" />
                        </Button>
                      </div>
                    </div>
                  );
                })}
              </div>
            </section>
          )}

          {/* On-Demand Side-by-Side Live Watch Lanes (2–4 shared `<AgentStagePane />`) */}
          {resolvedPinnedLanes.length > 0 && (
            <section
              aria-label={t("overview.liveLanesTitle")}
              className="mx-auto flex w-full max-w-[1600px] flex-col gap-2.5"
            >
              <div className="flex items-center justify-between px-0.5">
                <div className="flex items-center gap-2">
                  <Columns2 className="text-ink-tertiary size-3.5" />
                  <span className="text-ink-tertiary text-[11px] font-medium tracking-[0.04em] uppercase">
                    {t("overview.liveLanesTitle")}
                  </span>
                  <span className="text-ink-faint font-mono text-[11px] tabular-nums">
                    {resolvedPinnedLanes.length} / 4
                  </span>
                </div>
              </div>

              <div className={cn("grid gap-3", laneGridClass)}>
                {resolvedPinnedLanes.map((lane) => (
                  <div
                    key={lane.key}
                    className="border-border-default bg-surface-base flex h-[480px] min-w-0 flex-col overflow-hidden rounded-xl border"
                  >
                    <AgentStagePane
                      hostId={lane.hostId}
                      agentId={lane.agentId}
                      fallbackTitle={lane.title}
                      fallbackCwd={lane.cwd}
                      hostBadge={showHostBadges ? lane.hostName : null}
                      onClose={() => orbitActions.unpinOverviewLane(lane.hostId, lane.agentId)}
                    />
                  </div>
                ))}
              </div>
            </section>
          )}

          {/* Section 2 — Running & Recent Fleet */}
          <section
            aria-label={t("overview.fleetSectionTitle")}
            className="mx-auto flex w-full max-w-[740px] flex-col gap-2"
          >
            <div className="flex items-center justify-between px-0.5">
              <span className="text-ink-tertiary text-[11px] font-medium tracking-[0.04em] uppercase">
                {t("overview.fleetSectionTitle")}
              </span>
              <span className="text-ink-faint text-[11px]">
                {t("overview.watchLiveHint")}
              </span>
            </div>

            {fleetSessions.length === 0 ? (
              <div className="border-border-subtle text-ink-tertiary rounded-xl border border-dashed px-4 py-8 text-center text-[13px]">
                {t("overview.noActiveSessions")}
              </div>
            ) : (
              <div className="border-border-subtle bg-surface-raised divide-border-subtle divide-y overflow-hidden rounded-xl border">
                {fleetSessions.map((row) => (
                  <div
                    key={row.key}
                    onClick={(e) => {
                      if (e.metaKey || e.ctrlKey) {
                        if (row.isPinnedLane) {
                          orbitActions.unpinOverviewLane(row.hostId, row.agent.agent_id);
                        } else {
                          orbitActions.pinOverviewLane(row.hostId, row.agent.agent_id);
                        }
                      } else {
                        orbitActions.selectAgent(row.hostId, row.agent.agent_id);
                      }
                    }}
                    className="group/row hover:bg-accent/55 flex cursor-pointer items-center justify-between gap-3 px-3.5 py-2.5 transition-colors"
                  >
                    <div className="flex min-w-0 flex-1 items-center gap-2.5">
                      <span className="inline-flex size-[18px] shrink-0 items-center justify-center">
                        {row.isRunning ? (
                          <OrbitSpinner size={14} className="text-ink-secondary" />
                        ) : row.isAwaiting ? (
                          <span className="bg-signal-warning size-[8px] rounded-full" />
                        ) : row.isError ? (
                          <span className="bg-signal-danger size-[8px] rounded-full" />
                        ) : (
                          <ProviderIcon
                            provider={row.agent.provider}
                            className="size-[15px] opacity-75"
                          />
                        )}
                      </span>

                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5">
                          <span className="text-ink-primary truncate text-[13px] font-medium">
                            {row.effectiveTitle}
                          </span>
                          {showHostBadges && (
                            <span className="bg-surface-recessed text-ink-tertiary shrink-0 rounded px-1.5 py-0.5 text-[11px]">
                              {row.hostName}
                            </span>
                          )}
                          {row.agent.worktree_branch && (
                            <span className="text-ink-faint hidden max-w-[140px] items-center gap-0.5 truncate font-mono text-[11px] sm:inline-flex">
                              <GitBranch className="size-[10px] shrink-0" />
                              <span className="truncate">{row.agent.worktree_branch}</span>
                            </span>
                          )}
                        </div>
                        {row.stepPreview && (
                          <div className="text-ink-tertiary mt-0.5 truncate text-[11.5px]">
                            {row.stepPreview}
                          </div>
                        )}
                      </div>
                    </div>

                    <div className="flex shrink-0 items-center gap-2">
                      {(row.additions > 0 || row.deletions > 0) && (
                        <DiffStat additions={row.additions} deletions={row.deletions} />
                      )}

                      <span className="text-ink-faint font-mono text-[11px] tabular-nums">
                        {formatCompactElapsed(row.updatedAtMs, nowMs)}
                      </span>

                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          if (row.isPinnedLane) {
                            orbitActions.unpinOverviewLane(row.hostId, row.agent.agent_id);
                          } else {
                            orbitActions.pinOverviewLane(row.hostId, row.agent.agent_id);
                          }
                        }}
                        className={cn(
                          "inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-[6px] border px-2 text-[11.5px] transition-colors",
                          row.isPinnedLane
                            ? "border-border-strong bg-accent text-ink-primary font-medium"
                            : "border-border-subtle text-ink-tertiary hover:border-border-default hover:text-ink-primary opacity-80 group-hover/row:opacity-100",
                        )}
                      >
                        <Eye className="size-3" />
                        <span>
                          {row.isPinnedLane ? t("overview.watchingLive") : t("overview.watchLive")}
                        </span>
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>
      </ScrollArea>

      {/* Docked Bottom — Talk to Your Personal Agent (exact `max-w-[740px]` OmniComposer rhythm) */}
      <div className="border-border-subtle/70 bg-surface-base shrink-0 border-t px-6 py-3.5">
        <div className="mx-auto w-full max-w-[740px]">
          {activePersonalAgent ? (
            <div className="border-border-default bg-surface-raised focus-within:border-border-strong flex items-center gap-2.5 rounded-[20px] border px-3.5 py-2 transition-colors">
              <Sparkles className="text-ink-muted size-[15px] shrink-0 stroke-[1.75]" />
              <input
                type="text"
                value={routerPrompt}
                onChange={(e) => setRouterPrompt(e.target.value)}
                {...routerIme.compositionProps}
                onKeyDown={(e) => {
                  if (routerIme.shouldIgnoreKeyDown(e)) return;
                  const enterAction = resolveComposerEnterAction(
                    e,
                    enterToSend,
                    isAgentTurnBusy(activePersonalAgent.agent),
                    activeTurnSendBehavior,
                    Boolean(activePersonalAgent.agent.is_awaiting_permission),
                  );
                  if (enterAction.submit) {
                    e.preventDefault();
                    void handleSendToPersonalAgent(enterAction.steerImmediately);
                  }
                }}
                placeholder={t("overview.talkToPersonalAgentPlaceholder")}
                className="text-ink-primary placeholder:text-ink-faint min-w-0 flex-1 bg-transparent text-[15px] leading-[1.45] outline-none"
              />
              {showHostBadges && (
                <span className="text-ink-faint hidden shrink-0 text-[11px] sm:inline">
                  {activePersonalAgent.hostName}
                </span>
              )}
              <button
                type="button"
                aria-label={t("overview.sendToPersonalAgent")}
                disabled={!routerPrompt.trim() || sendingRouter}
                onClick={() => void handleSendToPersonalAgent()}
                className="bg-action-primary-bg text-action-primary-fg hover:bg-action-primary-hover inline-flex size-[28px] shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors disabled:opacity-35"
              >
                {sendingRouter ? (
                  <OrbitSpinner size={13} />
                ) : (
                  <ArrowUp className="size-[14px] stroke-[2.15]" />
                )}
              </button>
            </div>
          ) : (
            <div className="border-border-subtle bg-surface-recessed flex items-center justify-between gap-3 rounded-[16px] border px-4 py-2.5">
              <div className="flex min-w-0 items-center gap-2.5">
                <Sparkles className="text-ink-muted size-[15px] shrink-0 stroke-[1.75]" />
                <span className="text-ink-secondary truncate text-[13px]">
                  {t("overview.setupPersonalAgentPrompt")}
                </span>
              </div>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => orbitActions.openSettings("personal_router")}
                className="h-[26px] shrink-0 rounded-[7px] px-2.5 text-[12px]"
              >
                {t("overview.setupPersonalAgentAction")}
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
});
