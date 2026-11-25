import { memo, useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import {
  ConversationViewport,
  useViewportAnchorHold,
} from "@/components/conversation/conversation-viewport";
import { DecisionDock } from "@/components/stage/stage-approval-dock";
import { PlanReviewCard } from "@/components/stage/plan-review-card";
import {
  StageActivityWave,
  summarizeActiveSubagents,
} from "@/components/stage/stage-activity-wave";
import {
  PromptImageLightbox,
  type PromptImagePreviewTarget,
  TurnStageSection,
} from "@/components/stage/stream-stage";
import { useDelegationDispatches } from "@/components/stage/use-delegation-dispatches";
import {
  createStageSessionSelector,
  getTurnFooter,
  getTurnTopGapPx,
} from "@/components/stage/stage-helpers";
import { Button } from "@/components/ui/button";
import { ContextBanner } from "@/components/ui/context-banner";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
import {
  type PendingPermissionSummary,
  type StageTurnView,
  type SubagentLineageStepView,
  type TimelineItemView,
  isAgentForegroundRunning,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { computeSubagentTurnsFingerprint, MAX_CACHED_STAGE_VIEWS } from "@/lib/stage-cache";
import {
  type ResolvedSubagentItem,
  buildSubagentNameMatcher,
  resolveAgentAncestorLineage,
} from "@/lib/subagents";
import { useI18n } from "@/lib/i18n";
import type {
  PendingPermissionTimelineItem,
  PendingQuestionTimelineItem,
  UnifiedPendingSubagentPermission,
} from "@/state/selectors/session-selectors";
import { upsertLruRecord } from "@/state/store-helpers";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";

const NOOP_REWIND = () => {};
const EMPTY_TURNS: StageTurnView[] = [];
const EMPTY_HOST_PERMS: readonly PendingPermissionSummary[] = [];
const EMPTY_QUESTIONS: readonly PendingQuestionTimelineItem[] = [];
const EMPTY_SUBAGENT_PERMS: readonly UnifiedPendingSubagentPermission[] = [];
let providerTurnCache: Record<string, StageTurnView[]> = {};
const HYDRATION_TIMEOUT_MS = 1500;

export function mergeAgentPendingPermissions(
  agentId: string | null | undefined,
  stagePendingPermissions: readonly PendingPermissionTimelineItem[],
  stageRawPermissions: readonly TimelineItemView[] | undefined,
  hostPendingPermissions: readonly PendingPermissionSummary[] | undefined,
  excludeRequestId?: string | null,
): readonly PendingPermissionTimelineItem[] {
  if (!agentId || !hostPendingPermissions || hostPendingPermissions.length === 0) {
    return stagePendingPermissions;
  }
  const knownIds = new Set<string>();
  if (excludeRequestId) {
    knownIds.add(excludeRequestId);
  }
  for (const item of stagePendingPermissions) {
    knownIds.add(item.request_id);
  }
  if (stageRawPermissions) {
    for (const item of stageRawPermissions) {
      if (
        item.item_type === "permission_request" &&
        (item.resolved_decision !== null || item.plan_approval != null)
      ) {
        knownIds.add(item.request_id);
      }
    }
  }
  let extras: PendingPermissionTimelineItem[] | null = null;
  for (const perm of hostPendingPermissions) {
    if (perm.agent_id !== agentId || perm.plan_approval != null || knownIds.has(perm.request_id)) {
      continue;
    }
    knownIds.add(perm.request_id);
    const ts = Date.parse(perm.created_at);
    extras ??= [...stagePendingPermissions];
    extras.push({
      item_type: "permission_request",
      seq: Number.isFinite(ts) ? ts : 0,
      request_id: perm.request_id,
      title: perm.title,
      detail: perm.detail,
      allowed_decisions: perm.allowed_decisions,
      resolved_decision: null,
      guardian_assessment: perm.guardian_assessment ?? null,
      plan_approval: perm.plan_approval ?? null,
      timestamp_ms: Number.isFinite(ts) ? ts : null,
    });
  }
  return extras ?? stagePendingPermissions;
}

interface ThinkingTimingRecord {
  startedAtMs: number;
  completedAtMs: number | null;
  durationMs: number | null;
}

function buildSubagentTurnThinkingSignature(
  turn: StageTurnView,
  agentId: string,
  thinkingTimings: Record<string, ThinkingTimingRecord>,
): string {
  let sig = "";
  for (const seg of turn.live_segments) {
    if (seg.kind !== "step_group") continue;
    for (const it of seg.items) {
      if (it.item_type !== "assistant_thinking") continue;
      const key = `${agentId}::think::${it.item_id || it.seq}`;
      const dur = thinkingTimings[key]?.durationMs;
      if (dur !== null && dur !== undefined) {
        sig += `${it.seq}:${dur};`;
      }
    }
  }
  return sig;
}

function hasProviderContent(turns: StageTurnView[]): boolean {
  return turns.some((turn) =>
    turn.live_segments.length > 0 || turn.folded_steps.length > 0 ||
    turn.terminal_reply !== null || turn.agent_message !== null || turn.notices.length > 0 ||
    turn.compactions.length > 0 || turn.turn_diffs.length > 0,
  );
}

export const SubagentTab = memo(function SubagentTab({ tab }: { tab: SidePanelTab }) {
  const { t } = useI18n();
  const hostId = useOrbitSelector((s) => tab.hostId ?? s.selectedHostId);
  const isManaged = Boolean(hostId && tab.childAgentId);
  const parentHostId = useOrbitSelector((s) => isManaged ? (s.selectedHostId ?? hostId) : hostId);
  const providerId = isManaged ? null : tab.providerSubagentId;
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const remoteHostName = useOrbitSelector((s) => {
    if (!tab.hostId || !s.selectedHostId || tab.hostId === s.selectedHostId) return null;
    return (
      s.fleet.hosts.find((h) => h.host_id === tab.hostId)?.display_name ??
      s.hostDetails[tab.hostId]?.host.display_name ??
      tab.hostId
    );
  });
  const cwd = useOrbitSelector((s) => tab.cwd ?? s.selectedWorkspaceCwd);
  const parentAgentId = useOrbitSelector((s) => tab.parentAgentId ?? s.selectedAgentId);
  const parentHostRevision = useOrbitSelector((s) =>
    providerId && parentHostId ? (s.hostDetails[parentHostId]?.revision ?? 0) : 0,
  );
  const connectionState = useOrbitSelector((s) => hostId
    ? s.fleet.hosts.find((host) => host.host_id === hostId)?.connection_state ??
      s.hostDetails[hostId]?.host.connection_state
    : null,
  );
  const isNetworkOffline = connectionState === "disconnected";
  const hostConnected = connectionState === "online_direct" || connectionState === "online_relay";
  const childAgent = useOrbitSelector((s) =>
    hostId && tab.childAgentId
      ? (s.hostDetails[hostId]?.agents.find((a) => a.agent_id === tab.childAgentId) ?? null)
      : null,
  );
  const childStage = useOrbitSelector((s) =>
    hostId && tab.childAgentId
      ? (s.stages[`${hostId}::${tab.childAgentId}`] ?? null)
      : null,
  );
  const hostPendingPermissions = useOrbitSelector((s) =>
    hostId ? (s.hostDetails[hostId]?.pending_permissions ?? EMPTY_HOST_PERMS) : EMPTY_HOST_PERMS,
  );
  const providerSubagent = useOrbitSelector((s) =>
    parentHostId && parentAgentId && providerId
      ? (s.hostDetails[parentHostId]?.provider_subagents?.[parentAgentId]?.find(
          (ps) => ps.subagent_id === providerId,
        ) ?? null)
      : null,
  );
  const parentSubagent = useOrbitSelector((s) => {
    if (!parentHostId || !parentAgentId || !tab.subagentCallId) return null;
    const parentStage = s.stages[`${parentHostId}::${parentAgentId}`];
    return parentStage?.insights.subagents.find((sa) => sa.id === tab.subagentCallId) ?? null;
  });

  const childStatusLabel = childAgent?.status_label.toLowerCase() ?? "";
  const status: "awaiting_permission" | "running" | "completed" | "canceled" | "failed" = childAgent
    ? childAgent.is_awaiting_permission
      ? "awaiting_permission"
      : isAgentForegroundRunning(childAgent)
        ? "running"
        : childStatusLabel.includes("cancel") ||
            childStatusLabel.includes("interrupt") ||
            childStatusLabel.includes("stopped")
          ? "canceled"
          : childStatusLabel.includes("error") || childStatusLabel.includes("fail")
            ? "failed"
            : "completed"
    : providerSubagent
      ? providerSubagent.status === "running"
        ? "running"
        : providerSubagent.status === "completed"
          ? "completed"
          : providerSubagent.status === "canceled" || providerSubagent.status === "stopped"
            ? "canceled"
            : "failed"
      : parentSubagent?.status === "running"
        ? "running"
        : parentSubagent?.status === "failed" || parentSubagent?.status === "denied"
          ? "failed"
          : "completed";

  const resolvedPrompt = tab.subagentPrompt || providerSubagent?.subtitle || "";
  const providerKey = providerId && parentHostId && parentAgentId
    ? JSON.stringify([parentHostId, parentAgentId, providerId])
    : null;
  const [providerSnapshot, setProviderSnapshot] = useState<{ key: string; turns: StageTurnView[] } | null>(null);
  const cachedProviderTurns = providerKey ? providerTurnCache[providerKey] : undefined;
  const providerTurns = providerSnapshot?.key === providerKey
    ? providerSnapshot.turns
    : cachedProviderTurns ?? EMPTY_TURNS;
  const refreshProvider = useRef<(() => void) | null>(null);
  const readProviderTurns = useEffectEvent((host: string, parent: string, child: string) =>
    orbitIpc.getProviderSubagentTurns(host, parent, child, resolvedPrompt),
  );
  const wasHostConnected = useRef(hostConnected);
  const isActiveTab = useOrbitSelector((s) => s.sidePanelOpen && s.activeSidePanelTabId === tab.id);
  const thinkingTimingsRef = useRef<Record<string, ThinkingTimingRecord>>({});
  const [, setThinkingTick] = useState(0);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef<boolean>(true);
  const holdViewport = useViewportAnchorHold(viewportRef, stickToBottomRef);
  const [imagePreviewTarget, setImagePreviewTarget] =
    useState<PromptImagePreviewTarget | null>(null);
  const handleCloseImagePreview = useCallback(() => setImagePreviewTarget(null), []);

  const getThinkingDurationMs = useCallback((thinkKey: string): number | null => {
    return thinkingTimingsRef.current[thinkKey]?.durationMs ?? null;
  }, []);

  // Coalesce revisions during IPC, but always project once after the subscription opens.
  useEffect(() => {
    if (!isActiveTab || !parentHostId || !parentAgentId || !providerId || !providerKey) return;
    const subscribe = () => orbitActions.openProviderSubagentTimeline(parentHostId, parentAgentId, providerId);
    const opened = isNetworkOffline ? Promise.resolve() : subscribe().catch(() => undefined);
    let cancelled = false;
    let reading = false;
    let queued = false;
    let retried = false;

    const sync = async () => {
      if (reading) {
        queued = true;
        return;
      }
      reading = true;
      try {
        await opened;
        while (!cancelled) {
          queued = false;
          const turns = await readProviderTurns(parentHostId, parentAgentId, providerId);
          if (cancelled) return;
          const cached = providerTurnCache[providerKey];
          if (cached && hasProviderContent(cached) && !hasProviderContent(turns)) {
            // Preserve the last transcript during a transient Core cache miss.
            if (retried || isNetworkOffline) return;
            retried = true;
            await subscribe();
            continue;
          }
          retried = false;
          const unchanged = cached &&
            computeSubagentTurnsFingerprint(cached) === computeSubagentTurnsFingerprint(turns);
          const next = unchanged ? cached : turns;
          if (!unchanged) {
            providerTurnCache = upsertLruRecord(providerTurnCache, providerKey, next, MAX_CACHED_STAGE_VIEWS);
          }
          setProviderSnapshot((prev) =>
            prev?.key === providerKey && prev.turns === next ? prev : { key: providerKey, turns: next },
          );
          if (!queued) return;
        }
      } finally {
        reading = false;
      }
    };
    refreshProvider.current = () => { void sync().catch(() => undefined); };
    refreshProvider.current();
    return () => {
      cancelled = true;
      refreshProvider.current = null;
    };
  }, [isActiveTab, parentHostId, parentAgentId, providerId, providerKey, hostConnected, isNetworkOffline, tab]);

  useEffect(() => {
    refreshProvider.current?.();
  }, [parentHostRevision, providerSubagent?.updated_at, status, resolvedPrompt]);

  useEffect(() => {
    const reconnected = hostConnected && !wasHostConnected.current;
    wasHostConnected.current = hostConnected;
    if (reconnected && hostId && tab.childAgentId) {
      void orbitActions.hydrateSubagentStage(hostId, tab.childAgentId).catch(() => undefined);
    }
  }, [hostConnected, hostId, tab.childAgentId]);

  // Core can project just the descriptor's synthetic prompt before any events arrive.
  const solePrompt = providerTurns.length === 1 ? providerTurns[0].user_prompt : null;
  const projectedPromptOnly = solePrompt?.item_type === "user_prompt" && solePrompt.seq === 0 &&
    solePrompt.message_id === `sub_prompt_${providerId}` && !hasProviderContent(providerTurns);
  const emptyPrompt = projectedPromptOnly ? solePrompt.text : resolvedPrompt;
  const displayTurns = isManaged
    ? childStage?.turns ?? EMPTY_TURNS
    : projectedPromptOnly ? EMPTY_TURNS : providerTurns;
  const timelineKey = isManaged ? JSON.stringify([hostId, tab.childAgentId]) : providerKey;
  const [expiredKey, setExpiredKey] = useState<string | null>(null);
  const hydrating = Boolean(
    timelineKey && !displayTurns.length && expiredKey !== timelineKey &&
    providerSnapshot?.key !== timelineKey,
  );
  useEffect(() => {
    if (!hydrating) return;
    const timer = window.setTimeout(() => setExpiredKey(timelineKey), HYDRATION_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [hydrating, timelineKey]);

  const displayTurnsFingerprint = useMemo(
    () => computeSubagentTurnsFingerprint(displayTurns),
    [displayTurns],
  );
  const contentVersion = `${childStage?.revision ?? 0}:${displayTurnsFingerprint}`;
  const effectiveAgentId = tab.childAgentId ?? tab.providerSubagentId ?? parentAgentId ?? "";
  const effectiveHostId = isManaged ? (hostId ?? "") : (parentHostId ?? "");
  const selectChildren = useMemo(
    () => createStageSessionSelector(effectiveHostId, effectiveAgentId),
    [effectiveHostId, effectiveAgentId],
  );
  const selectSiblings = useMemo(
    () => createStageSessionSelector(parentHostId, parentAgentId),
    [parentHostId, parentAgentId],
  );
  const { subagents: children, decisions: childDecisions } = useOrbitSelector(selectChildren);
  const { subagents: siblings, decisions: parentDecisions } = useOrbitSelector(selectSiblings);
  const effectiveCwd = childAgent?.cwd || cwd || "";
  const pendingPlanApproval = isManaged ? childDecisions.pendingPlanApproval : null;
  const pendingPermissions = useMemo(
    () =>
      isManaged
        ? mergeAgentPendingPermissions(
            tab.childAgentId,
            childDecisions.parentPermissions,
            childStage?.pending_permissions,
            hostPendingPermissions,
            pendingPlanApproval?.request_id ?? null,
          )
        : childDecisions.parentPermissions,
    [
      isManaged,
      tab.childAgentId,
      childDecisions.parentPermissions,
      childStage?.pending_permissions,
      hostPendingPermissions,
      pendingPlanApproval?.request_id,
    ],
  );
  const pendingQuestions = isManaged ? childDecisions.parentQuestions : EMPTY_QUESTIONS;
  const nestedSubagentPermissions = isManaged ? childDecisions.subagentPermissions : EMPTY_SUBAGENT_PERMS;
  const hasManagedDecisions = Boolean(
    isManaged &&
      hostId &&
      tab.childAgentId &&
      (pendingPlanApproval !== null ||
        pendingPermissions.length > 0 ||
        pendingQuestions.length > 0 ||
        nestedSubagentPermissions.length > 0),
  );
  const isActiveProviderSubagent =
    !isManaged && (status === "running" || status === "awaiting_permission");
  const showParentFallbackDock = Boolean(
    isActiveProviderSubagent &&
      parentHostId &&
      parentAgentId &&
      (parentDecisions.pendingPlanApproval !== null ||
        parentDecisions.parentPermissions.length > 0 ||
        parentDecisions.parentQuestions.length > 0),
  );
  const currentPeer = siblings.find((sa) =>
    (tab.childAgentId && sa.childAgentId === tab.childAgentId) ||
    (tab.providerSubagentId && sa.providerSubagentId === tab.providerSubagentId) ||
    sa.callId === tab.subagentCallId,
  );
  const subagentSessionSubagents = useMemo(() => {
    const peers = new Map(siblings.map((sa) => [sa.key, sa]));
    for (const child of children) peers.set(child.key, child);
    return Array.from(peers.values());
  }, [children, siblings]);
  const dispatches = useDelegationDispatches(displayTurns, children, effectiveHostId, subagentSessionSubagents);

  const subagentNameMatcher = useMemo(
    () => buildSubagentNameMatcher(subagentSessionSubagents),
    [subagentSessionSubagents],
  );

  const handleOpenNestedSubagent = useCallback(
    (sa: ResolvedSubagentItem) => {
      orbitActions.openSubagentTab({
        callId: sa.callId,
        role: sa.title,
        promptPreview: sa.promptPreview,
        childAgentId: sa.childAgentId,
        childHostId: sa.childHostId || undefined,
        providerSubagentId: sa.providerSubagentId,
        parentAgentId: parentAgentId || effectiveAgentId || undefined,
      });
    },
    [effectiveAgentId, parentAgentId],
  );

  const latestTurn = displayTurns[displayTurns.length - 1] ?? null;
  const isLatestTurnActive = Boolean(latestTurn && status === "running");

  const nestedSubagentActivity = useMemo(
    () => summarizeActiveSubagents(children),
    [children],
  );

  const runningBackgroundTasks = useMemo(
    () =>
      isManaged
        ? (childStage?.insights.background_tasks ?? []).filter((task) => task.status === "running")
        : [],
    [isManaged, childStage?.insights.background_tasks],
  );

  const handleNestedWaveClick = useMemo(() => {
    const target = nestedSubagentActivity.firstActive;
    if (!target) return undefined;
    return () => handleOpenNestedSubagent(target);
  }, [nestedSubagentActivity.firstActive, handleOpenNestedSubagent]);

  const effectiveContentVersion = `${contentVersion}:${nestedSubagentActivity.runningCount}:${nestedSubagentActivity.awaitingCount}:${runningBackgroundTasks.length}`;

  // Track live per-thinking-block start/completion durations matching StreamStage
  useEffect(() => {
    if (!effectiveAgentId) return;
    let changed = false;
    const now = Date.now();
    const activeThinkKeys = new Set<string>();

    if (latestTurn) {
      const lastSeg = latestTurn.live_segments[latestTurn.live_segments.length - 1] ?? null;
      const lastSegItem =
        lastSeg?.kind === "step_group" ? (lastSeg.items[lastSeg.items.length - 1] ?? null) : null;

      for (const seg of latestTurn.live_segments) {
        if (seg.kind !== "step_group") continue;
        for (const item of seg.items) {
          if (item.item_type !== "assistant_thinking") continue;

          const thinkKey = `${effectiveAgentId}::think::${item.item_id || item.seq}`;
          const isCurrentlyThinking =
            isLatestTurnActive &&
            lastSegItem?.item_type === "assistant_thinking" &&
            lastSegItem.seq === item.seq;
          const prevThink = thinkingTimingsRef.current[thinkKey];

          if (isCurrentlyThinking) {
            activeThinkKeys.add(thinkKey);
            if (!prevThink) {
              const seededStartMs =
                item.timestamp_ms != null && item.timestamp_ms <= now ? item.timestamp_ms : now;
              thinkingTimingsRef.current[thinkKey] = {
                startedAtMs: seededStartMs,
                completedAtMs: null,
                durationMs: null,
              };
              changed = true;
            }
          } else if (prevThink && prevThink.completedAtMs === null) {
            thinkingTimingsRef.current[thinkKey] = {
              ...prevThink,
              completedAtMs: now,
              durationMs: Math.max(0, now - prevThink.startedAtMs),
            };
            changed = true;
          }
        }
      }
    }

    const prefix = `${effectiveAgentId}::think::`;
    for (const [k, rec] of Object.entries(thinkingTimingsRef.current)) {
      if (k.startsWith(prefix) && rec.completedAtMs === null && !activeThinkKeys.has(k)) {
        thinkingTimingsRef.current[k] = {
          ...rec,
          completedAtMs: now,
          durationMs: Math.max(0, now - rec.startedAtMs),
        };
        changed = true;
      }
    }

    if (changed) {
      setThinkingTick((n) => n + 1);
    }
  }, [effectiveAgentId, latestTurn, isLatestTurnActive]);

  const viewportResetKey = `${tab.id}:${tab.childAgentId ?? tab.providerSubagentId ?? tab.subagentCallId ?? ""}`;

  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const fleetHosts = useOrbitSelector((s) => s.fleet.hosts);
  const ancestorLineage = useMemo((): SubagentLineageStepView[] => {
    if (!isManaged || !hostId || !tab.childAgentId) return [];
    return resolveAgentAncestorLineage({
      hostId,
      agentId: tab.childAgentId,
      stageAncestorLineage: childStage?.insights?.ancestor_lineage ?? null,
      hostDetails,
      fleetHosts,
    });
  }, [isManaged, hostId, tab.childAgentId, childStage?.insights?.ancestor_lineage, hostDetails, fleetHosts]);

  const handleOpenLineageStep = useCallback(
    (step: SubagentLineageStepView, index: number) => {
      if (index === 0) {
        orbitActions.selectAgent(step.host_id, step.agent_id);
        return;
      }
      const prevStep = ancestorLineage[index - 1];
      orbitActions.openSubagentTab({
        callId: step.agent_id,
        role: step.title,
        promptPreview: "",
        childAgentId: step.agent_id,
        childHostId: step.host_id,
        parentAgentId: prevStep?.agent_id,
      });
    },
    [ancestorLineage],
  );

  // Show an actionable fallback banner ONLY when awaiting approval without a live DecisionDock payload
  // (e.g. cold hydration or provider-only subagent whose gate lives on the parent), or to identify a cross-host subagent.
  const showApprovalFallbackBanner =
    status === "awaiting_permission" && !hasManagedDecisions && !showParentFallbackDock;
  const showInlineContextBar = showApprovalFallbackBanner || Boolean(remoteHostName);

  const handleFallbackApprovalAction = useCallback(() => {
    if (isManaged && hostId && tab.childAgentId) {
      void orbitActions.hydrateSubagentStage(hostId, tab.childAgentId).catch(() => undefined);
      void orbitIpc.refreshPendingPermissions(hostId).catch(() => undefined);
      return;
    }
    if (parentHostId && parentAgentId) {
      orbitActions.selectAgent(parentHostId, parentAgentId);
    }
  }, [isManaged, hostId, tab.childAgentId, parentHostId, parentAgentId]);

  return (
    <div className="orbit-stage-enter bg-surface-base flex min-h-0 flex-1 flex-col overflow-hidden">
      {ancestorLineage.length > 1 && (
        <nav
          aria-label={t("overview.subagentLineageAria")}
          className="border-border-subtle bg-surface-base flex h-[30px] shrink-0 items-center gap-1 overflow-x-auto border-b px-4 text-[11px] text-ink-tertiary"
        >
          {ancestorLineage.map((step, idx) => {
            const isCurrent = idx === ancestorLineage.length - 1;
            return (
              <span key={`${step.host_id}::${step.agent_id}`} className="inline-flex shrink-0 items-center gap-1">
                {idx > 0 && <ChevronRight aria-hidden="true" className="size-3 text-ink-muted" />}
                {isCurrent ? (
                  <span className="max-w-[180px] truncate font-medium text-ink-primary">
                    {step.title}
                  </span>
                ) : (
                  <button
                    type="button"
                    onClick={() => handleOpenLineageStep(step, idx)}
                    title={step.host_name ? `${step.title} · ${step.host_name}` : step.title}
                    className="max-w-[160px] cursor-pointer truncate rounded-[4px] px-1 py-0.5 text-ink-tertiary transition-colors hover:bg-accent/70 hover:text-ink-primary"
                  >
                    {step.title}
                  </button>
                )}
              </span>
            );
          })}
        </nav>
      )}
      {showInlineContextBar && (
        <ContextBanner
          variant="strip"
          tone={showApprovalFallbackBanner ? "warning" : "neutral"}
          icon={
            <SubagentAvatar
              character={currentPeer?.character}
              seedKey={tab.childAgentId || tab.providerSubagentId || tab.subagentCallId || tab.title}
              status={status}
              size="xs"
            />
          }
          actions={
            showApprovalFallbackBanner ? (
              <Button
                type="button"
                variant="outline"
                size="xs"
                onClick={handleFallbackApprovalAction}
              >
                {isManaged ? t("common.refresh") : t("approval.inspectSubagent")}
              </Button>
            ) : undefined
          }
        >
          <span className="inline-flex items-center gap-2">
            {showApprovalFallbackBanner && (
              <span className="font-medium">{t("overview.subagentApprovalRequired")}</span>
            )}
            {remoteHostName && (
              <span className="bg-surface-recessed text-ink-tertiary border-border-subtle inline-flex max-w-[180px] items-center truncate rounded-[6px] border px-1.5 py-0.5 text-[11px] leading-none">
                {remoteHostName}
              </span>
            )}
          </span>
        </ContextBanner>
      )}

      {/* Main Conversation Stream Parity (identical TurnStageSection, spacing, and typography; no Composer) */}
      <ConversationViewport
        hostId={effectiveHostId}
        cwd={cwd}
        resetKey={viewportResetKey}
        contentVersion={effectiveContentVersion}
        viewportRef={viewportRef}
        stickToBottomRef={stickToBottomRef}
        viewportClassName="px-6 pt-6 pb-[24px]"
      >
        <div className="orbit-stage-enter mx-auto w-full min-w-0 max-w-[740px]">
          {displayTurns.length > 0 ? (
            <div>
              {(() => {
                let previousHasFooter: boolean | undefined;
                return displayTurns.map((turn, idx) => {
                  const isLastTurn = idx === displayTurns.length - 1;
                  const isTurnActive = isLastTurn && status === "running";
                  const topGapPx = getTurnTopGapPx(turn, previousHasFooter);
                  previousHasFooter =
                    getTurnFooter(turn, {
                      isActive: isTurnActive,
                      isLastTurn,
                      canFork: false,
                    }) !== null;
                  const liveLabel = isLastTurn
                    ? isManaged
                      ? (childAgent?.status_label ?? null)
                      : status === "running"
                        ? "Working..."
                        : null
                    : null;
                  const thinkingSignature = buildSubagentTurnThinkingSignature(
                    turn,
                    effectiveAgentId,
                    thinkingTimingsRef.current,
                  );
                  return (
                    <div key={turn.key} style={topGapPx > 0 ? { paddingTop: topGapPx } : undefined}>
                      <TurnStageSection
                        turn={turn}
                        dispatches={dispatches}
                        isTurnActive={isTurnActive}
                        isLastTurn={isLastTurn}
                        agentStatusLabel={liveLabel}
                        hasLiveBackgroundTasks={isTurnActive && runningBackgroundTasks.length > 0}
                        isNetworkOffline={isNetworkOffline}
                        autoFoldCompleted={coalesceExploration}
                        hostId={effectiveHostId}
                        cwd={cwd}
                        agentId={effectiveAgentId}
                        thinkingSignature={thinkingSignature}
                        getThinkingDurationMs={getThinkingDurationMs}
                        holdViewport={holdViewport}
                        onRequestRewind={NOOP_REWIND}
                        onPreviewImage={setImagePreviewTarget}
                        sessionSubagents={subagentSessionSubagents}
                        sessionSubagentMatcher={subagentNameMatcher}
                        onOpenSubagent={handleOpenNestedSubagent}
                        parentMessageLabel="Sent to parent"
                      />
                    </div>
                  );
                });
              })()}
              <StageActivityWave
                runningSubagentsCount={nestedSubagentActivity.runningCount}
                awaitingSubagentsCount={nestedSubagentActivity.awaitingCount}
                runningBackgroundTasks={runningBackgroundTasks}
                armedTimersCount={0}
                hasActiveGoal={false}
                isForegroundRunning={isLatestTurnActive}
                onClick={handleNestedWaveClick}
                className="mt-3 mb-1"
              />
            </div>
          ) : hydrating ? (
            <div role="status" className="text-ink-muted flex items-center justify-center gap-2 py-6 text-[13px]">
              <OrbitSpinner size={14} />
              {t("common.syncing")}
            </div>
          ) : emptyPrompt.trim() ? (
            <p className="selectable-text text-ink-secondary text-[13px] leading-[1.55] whitespace-pre-wrap break-words">{emptyPrompt}</p>
          ) : (
            <div className="text-muted-foreground flex flex-col items-center justify-center gap-2.5 py-20 text-center text-[13px]">
              {status === "running" && <OrbitSpinner size={16} className="text-foreground/65" />}
              <span>
                {status === "running"
                  ? "Running in an isolated context…"
                  : status === "canceled"
                    ? "Subagent run was canceled."
                    : "Subagent outputs were folded into the main conversation."}
              </span>
            </div>
          )}
        </div>
      </ConversationViewport>

      {hasManagedDecisions && hostId && tab.childAgentId ? (
        <div className="bg-surface-base relative z-20 shrink-0 pt-2">
          {pendingPlanApproval && (
            <PlanReviewCard
              hostId={hostId}
              agentId={tab.childAgentId}
              cwd={effectiveCwd}
              plan={pendingPlanApproval}
              compact
            />
          )}
          <DecisionDock
            hostId={hostId}
            agentId={tab.childAgentId}
            cwd={effectiveCwd}
            permissions={pendingPermissions}
            questions={pendingQuestions}
            subagentPermissions={nestedSubagentPermissions}
            registerGlobalShortcuts={false}
            compact
          />
        </div>
      ) : showParentFallbackDock && parentHostId && parentAgentId ? (
        <div className="bg-surface-base relative z-20 shrink-0 pt-2">
          {parentDecisions.pendingPlanApproval && (
            <PlanReviewCard
              hostId={parentHostId}
              agentId={parentAgentId}
              cwd={effectiveCwd}
              plan={parentDecisions.pendingPlanApproval}
              compact
            />
          )}
          <DecisionDock
            hostId={parentHostId}
            agentId={parentAgentId}
            cwd={effectiveCwd}
            permissions={parentDecisions.parentPermissions}
            questions={parentDecisions.parentQuestions}
            registerGlobalShortcuts={false}
            compact
          />
        </div>
      ) : null}

      <PromptImageLightbox
        target={imagePreviewTarget}
        onClose={handleCloseImagePreview}
      />
    </div>
  );
});
