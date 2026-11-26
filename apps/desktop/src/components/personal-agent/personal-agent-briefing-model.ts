import {
  type AgentListItemStatus,
  mapBriefingWorkerStateToStatus,
} from "../../lib/agent-list-item.ts";
import type {
  AgentCardView,
  BriefingDecision,
  BriefingMission,
  BriefingWaitItem,
  BriefingWorker,
  ConditionalTimerRecord,
  GoalRunRecord,
  HostDetailView,
  PersonalAgentActivityEntry,
  PersonalAgentBriefingView,
  PersonalRouterConfig,
  ProviderKind,
  RouterHostCapabilities,
} from "../../lib/orbit-ipc.ts";
import type { ArchivedThreadEntry, WindowedAgentStageView } from "../../state/store-types.ts";

export interface PersonalAgentIdentity {
  readonly hostId: string;
  readonly agentId: string;
}

export interface SinceLastVisitSummary {
  readonly completed: number;
  readonly autoHandled: number;
  readonly hasSummary: boolean;
}

export type LiveBriefingAgentCard = AgentCardView & {
  status?: { state: string; source?: string };
};

export interface PartitionedBriefingWorker {
  readonly key: string;
  readonly worker: BriefingWorker;
  readonly status: AgentListItemStatus;
  readonly indentLevel: number;
  readonly parentHostId: string | null;
  readonly lineage: string | null;
  readonly archivable: boolean;
  readonly fromOtherActive: boolean;
}

export interface PartitionedBriefingWorkers {
  readonly runningMissionWorkers: readonly PartitionedBriefingWorker[];
  readonly runningOtherWorkers: readonly PartitionedBriefingWorker[];
  readonly completedWorkers: readonly PartitionedBriefingWorker[];
  readonly archivableCompletedWorkers: readonly PartitionedBriefingWorker[];
  readonly inProgressCount: number;
}

export function briefingWorkerKey(hostId: string, agentId: string): string {
  return `${hostId}::${agentId}`;
}

function isBriefingAgentArchived(
  liveAgent: LiveBriefingAgentCard | null,
  archivedThreads: Readonly<Record<string, ArchivedThreadEntry>> | undefined,
  hostId: string,
  agentId: string,
): boolean {
  if (liveAgent?.archived_at_rfc3339) return true;
  return Boolean(archivedThreads?.[briefingWorkerKey(hostId, agentId)]);
}

function isLiveAgentStatusFailed(liveAgent: LiveBriefingAgentCard): boolean {
  const state = liveAgent.status?.state;
  const lower = liveAgent.status_label?.toLowerCase() ?? "";
  return state === "error" || /error|fail/.test(lower);
}

function isLiveAgentStatusCanceled(liveAgent: LiveBriefingAgentCard): boolean {
  const state = liveAgent.status?.state;
  const lower = liveAgent.status_label?.toLowerCase() ?? "";
  return state === "closed" || /cancel|interrupt|stopped|closed/.test(lower);
}

/**
 * Resolves the authoritative live `AgentListItemStatus` for a briefing worker by reconciling
 * the Rust briefing projection with the latest in-memory `AgentCardView` from `hostDetails`.
 *
 * Guarantees that completed, idle, stopped, or timer/background-waiting sessions are NEVER
 * classified as `"running"` even during brief briefing refresh windows.
 */
export function resolveEffectiveBriefingWorkerStatus(
  worker: BriefingWorker,
  liveAgent: LiveBriefingAgentCard | null,
  hasActiveManagedChild = false,
): AgentListItemStatus {
  if (!liveAgent) {
    return mapBriefingWorkerStateToStatus(worker.state, null);
  }

  const liveState = liveAgent.status?.state;
  if (liveAgent.is_awaiting_permission || liveState === "awaiting_permission") {
    return "awaiting_permission";
  }
  if (isLiveAgentStatusFailed(liveAgent)) {
    return "failed";
  }

  const isWakeupWaiting =
    liveState === "autonomous_wakeup" &&
    typeof liveAgent.status?.source === "string" &&
    liveAgent.status.source.trim().startsWith("waiting:");

  const isLiveForegroundRunning =
    !liveAgent.has_background_tasks &&
    !isWakeupWaiting &&
    (liveState === "initializing" ||
      liveState === "running" ||
      liveState === "autonomous_wakeup" ||
      (liveState === undefined &&
        liveAgent.is_running &&
        !/^(?:waiting\b|running\s*\(.*background task|background task$)/i.test(
          liveAgent.status_label?.trim() ?? "",
        )));

  if (isLiveForegroundRunning) {
    return "running";
  }

  // Preserve running state strictly when this worker owns an active provider-native subagent
  // (indicated by `has_running_subagents` with no active managed descendant in this mission).
  if (
    liveAgent.has_running_subagents &&
    !hasActiveManagedChild &&
    worker.state === "running"
  ) {
    return "running";
  }

  if (isLiveAgentStatusCanceled(liveAgent)) {
    return "canceled";
  }

  if (liveAgent.has_background_tasks || liveAgent.has_armed_timers || isWakeupWaiting) {
    return "idle";
  }

  if (liveAgent.latest_seq > 0 || worker.state === "done") {
    return "completed";
  }

  return "idle";
}

function isWorkerArchivable(
  liveAgent: LiveBriefingAgentCard | null,
  status: AgentListItemStatus,
): boolean {
  if (status === "running" || status === "awaiting_permission") return false;
  if (liveAgent?.has_background_tasks || liveAgent?.has_armed_timers) return false;
  return true;
}

/**
 * Splits Personal Agent briefing missions and `other_active` workers strictly by true live
 * execution state:
 * - `runningMissionWorkers` / `runningOtherWorkers`: ONLY actively executing (`"running"`)
 *   workers (plus any un-surfaced `"awaiting_permission"` fallback not already in `needs_you`).
 * - `completedWorkers`: settled PA workers (`"completed"`, `"canceled"`, `"idle"`, and any
 *   un-surfaced `"failed"` fallback), preserving mission indentation when siblings share the
 *   same group and preserving parent lineage in hover tooltips when split across groups.
 * - Deduplicates agents already surfaced in `needs_you` and omits archived settled workers.
 */
export function partitionBriefingWorkers(params: {
  missions: readonly BriefingMission[];
  otherActive: readonly BriefingWorker[];
  needsYou: readonly BriefingDecision[];
  hostDetails: Readonly<Record<string, HostDetailView>>;
  archivedThreads?: Readonly<Record<string, ArchivedThreadEntry>>;
}): PartitionedBriefingWorkers {
  const { missions, otherActive, needsYou, hostDetails, archivedThreads } = params;

  const findLiveAgent = (hostId: string, agentId: string): LiveBriefingAgentCard | null =>
    (hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId) as
      | LiveBriefingAgentCard
      | undefined) ?? null;

  const needsYouAgentKeys = new Set<string>();
  for (const decision of needsYou) {
    needsYouAgentKeys.add(briefingWorkerKey(decision.host_id, decision.agent_id));
  }

  const seenKeys = new Set<string>();
  const runningMissionWorkers: PartitionedBriefingWorker[] = [];
  const completedWorkers: PartitionedBriefingWorker[] = [];

  for (const mission of missions) {
    const childEntries = mission.workers.map((child) => {
      const key = briefingWorkerKey(child.host_id, child.agent_id);
      const liveAgent = findLiveAgent(child.host_id, child.agent_id);
      const status = resolveEffectiveBriefingWorkerStatus(child, liveAgent, false);
      return { worker: child, key, liveAgent, status };
    });

    const hasActiveManagedChild = childEntries.some(
      (c) => c.status === "running" || c.status === "awaiting_permission",
    );

    const rootKey = briefingWorkerKey(mission.root.host_id, mission.root.agent_id);
    const rootLive = findLiveAgent(mission.root.host_id, mission.root.agent_id);
    const rootStatus = resolveEffectiveBriefingWorkerStatus(
      mission.root,
      rootLive,
      hasActiveManagedChild,
    );
    const rootArchived = isBriefingAgentArchived(
      rootLive,
      archivedThreads,
      mission.root.host_id,
      mission.root.agent_id,
    );
    const rootInNeedsYou = needsYouAgentKeys.has(rootKey);

    let rootPlacedInRunning = false;
    let rootPlacedInCompleted = false;

    if (!seenKeys.has(rootKey)) {
      seenKeys.add(rootKey);
      if (rootStatus === "running" || (rootStatus === "awaiting_permission" && !rootInNeedsYou)) {
        runningMissionWorkers.push({
          key: rootKey,
          worker: mission.root,
          status: rootStatus,
          indentLevel: 0,
          parentHostId: null,
          lineage: null,
          archivable: false,
          fromOtherActive: false,
        });
        rootPlacedInRunning = true;
      } else if (
        !rootArchived &&
        rootStatus !== "awaiting_permission" &&
        !(rootStatus === "failed" && rootInNeedsYou)
      ) {
        completedWorkers.push({
          key: rootKey,
          worker: mission.root,
          status: rootStatus,
          indentLevel: 0,
          parentHostId: null,
          lineage: null,
          archivable: isWorkerArchivable(rootLive, rootStatus),
          fromOtherActive: false,
        });
        rootPlacedInCompleted = true;
      }
    }

    for (const child of childEntries) {
      if (seenKeys.has(child.key)) continue;
      seenKeys.add(child.key);

      const childArchived = isBriefingAgentArchived(
        child.liveAgent,
        archivedThreads,
        child.worker.host_id,
        child.worker.agent_id,
      );
      const childInNeedsYou = needsYouAgentKeys.has(child.key);
      const lineage =
        mission.root.title.trim() && mission.root.title.trim() !== child.worker.title.trim()
          ? `${mission.root.title.trim()} → ${child.worker.title.trim()}`
          : null;

      if (
        child.status === "running" ||
        (child.status === "awaiting_permission" && !childInNeedsYou)
      ) {
        runningMissionWorkers.push({
          key: child.key,
          worker: child.worker,
          status: child.status,
          indentLevel: rootPlacedInRunning ? 1 : 0,
          parentHostId: rootPlacedInRunning ? mission.root.host_id : null,
          lineage,
          archivable: false,
          fromOtherActive: false,
        });
      } else if (
        !childArchived &&
        child.status !== "awaiting_permission" &&
        !(child.status === "failed" && childInNeedsYou)
      ) {
        completedWorkers.push({
          key: child.key,
          worker: child.worker,
          status: child.status,
          indentLevel: rootPlacedInCompleted ? 1 : 0,
          parentHostId: rootPlacedInCompleted ? mission.root.host_id : null,
          lineage,
          archivable: isWorkerArchivable(child.liveAgent, child.status),
          fromOtherActive: false,
        });
      }
    }
  }

  const runningOtherWorkers: PartitionedBriefingWorker[] = [];
  for (const worker of otherActive) {
    const key = briefingWorkerKey(worker.host_id, worker.agent_id);
    if (seenKeys.has(key)) continue;
    seenKeys.add(key);

    const liveAgent = findLiveAgent(worker.host_id, worker.agent_id);
    const status = resolveEffectiveBriefingWorkerStatus(worker, liveAgent, false);
    const inNeedsYou = needsYouAgentKeys.has(key);
    const archived = isBriefingAgentArchived(
      liveAgent,
      archivedThreads,
      worker.host_id,
      worker.agent_id,
    );

    if (archived) continue;
    if (status === "running" || (status === "awaiting_permission" && !inNeedsYou)) {
      runningOtherWorkers.push({
        key,
        worker,
        status,
        indentLevel: 0,
        parentHostId: null,
        lineage: null,
        archivable: false,
        fromOtherActive: true,
      });
    }
  }

  const archivableCompletedWorkers = completedWorkers.filter((item) => item.archivable);
  const inProgressCount = runningMissionWorkers.length + runningOtherWorkers.length;

  return {
    runningMissionWorkers,
    runningOtherWorkers,
    completedWorkers,
    archivableCompletedWorkers,
    inProgressCount,
  };
}

/**
 * Resolves the live `GoalRunRecord` backing a `BriefingWaitItem` so the Briefing Rail can
 * directly render the shared interactive `OverviewGoalRow`.
 */
export function findLiveBriefingGoal(
  item: BriefingWaitItem,
  hostDetails: Readonly<Record<string, HostDetailView>>,
): GoalRunRecord | null {
  if (item.kind !== "goal") return null;
  const goals = hostDetails[item.host_id]?.goal_runs;
  if (!goals || goals.length === 0) return null;
  const exact = goals.find((g) => `goal:${item.host_id}:${g.goal_id}` === item.key);
  if (exact && exact.status !== "succeeded" && exact.status !== "failed" && exact.status !== "canceled") {
    return exact;
  }
  return (
    goals.find(
      (g) =>
        g.agent_id === item.agent_id &&
        (g.status === "running" || g.status === "verifying" || g.status === "paused"),
    ) ?? null
  );
}

/**
 * Resolves the live armed `ConditionalTimerRecord` backing a `BriefingWaitItem` so the
 * Briefing Rail can directly render the shared interactive `ArmedTimerRow`.
 */
export function findLiveBriefingTimer(
  item: BriefingWaitItem,
  hostDetails: Readonly<Record<string, HostDetailView>>,
): ConditionalTimerRecord | null {
  if (item.kind !== "timer") return null;
  const timers = hostDetails[item.host_id]?.conditional_timers;
  if (!timers || timers.length === 0) return null;
  return (
    timers.find(
      (tm) => `timer:${item.host_id}:${tm.timer_id}` === item.key && tm.state === "armed",
    ) ?? null
  );
}

/**
 * Filters raw daemon activity logs (which include all Guardian Tier-1 / permission-rule
 * auto-decisions on the host) to strictly the Personal Agent and its dispatched worker subtree.
 */
export function filterActivityForPersonalAgent(
  entries: readonly PersonalAgentActivityEntry[],
  briefing: PersonalAgentBriefingView | null,
  pa: PersonalAgentIdentity | null,
): PersonalAgentActivityEntry[] {
  if (entries.length === 0) return [];
  if (!pa) return [];

  const paScopedKeys = new Set<string>([`${pa.hostId}::${pa.agentId}`]);
  const paAgentIds = new Set<string>([pa.agentId]);

  const addWorker = (w: BriefingWorker) => {
    paScopedKeys.add(`${w.host_id}::${w.agent_id}`);
    paAgentIds.add(w.agent_id);
  };

  if (briefing) {
    for (const mission of briefing.missions) {
      addWorker(mission.root);
      for (const worker of mission.workers) {
        addWorker(worker);
      }
    }
    for (const decision of briefing.needs_you) {
      if (decision.dispatched_by_personal_agent) {
        paScopedKeys.add(`${decision.host_id}::${decision.agent_id}`);
        paAgentIds.add(decision.agent_id);
      }
    }
  }

  return entries.filter((entry) => {
    if (entry.actor === "personal_agent") return true;
    if (!entry.agent_id) return false;
    if (entry.host_id) {
      return paScopedKeys.has(`${entry.host_id}::${entry.agent_id}`);
    }
    return paAgentIds.has(entry.agent_id);
  });
}

/**
 * Combines Rust core's `since_last_visit.completed` with frontend-observed auto-handled
 * activity entries created after the user's previous Personal Agent visit timestamp.
 */
export function computeSinceLastVisitSummary(
  briefing: PersonalAgentBriefingView | null,
  filteredActivity: readonly PersonalAgentActivityEntry[],
  previousVisitMs: number | null,
): SinceLastVisitSummary {
  const completed = briefing?.since_last_visit.completed ?? 0;
  const coreAutoHandled = briefing?.since_last_visit.auto_handled ?? 0;
  const activityAutoHandled =
    previousVisitMs !== null && previousVisitMs > 0
      ? filteredActivity.filter((e) => e.timestamp_ms > previousVisitMs).length
      : 0;
  const autoHandled = Math.max(coreAutoHandled, activityAutoHandled);
  return {
    completed,
    autoHandled,
    hasSummary: completed > 0 || autoHandled > 0,
  };
}

/**
 * Selects the recommended host for one-click Personal Agent activation from
 * `discoverFleetRouterHosts()`, preferring online hosts with highest score.
 */
export function selectRecommendedRouterHost(
  hosts: readonly RouterHostCapabilities[],
): RouterHostCapabilities | null {
  if (hosts.length === 0) return null;
  const sorted = [...hosts].sort((a, b) => {
    if (a.online !== b.online) return a.online ? -1 : 1;
    if (b.recommended_score !== a.recommended_score) {
      return b.recommended_score - a.recommended_score;
    }
    if (a.hermes_installed !== b.hermes_installed) return a.hermes_installed ? -1 : 1;
    if (a.is_local !== b.is_local) return a.is_local ? 1 : -1;
    return a.host_name.localeCompare(b.host_name);
  });
  return sorted[0] ?? null;
}

/**
 * Builds the default `PersonalRouterConfig` payload for one-click Personal Agent enablement.
 */
export function buildDefaultRouterConfig(
  target: RouterHostCapabilities,
  existing?: PersonalRouterConfig | null,
): PersonalRouterConfig {
  const fallbackProvider: ProviderKind = target.hermes_installed
    ? "acp"
    : (target.available_providers[0] ?? "claude_code");
  const fallbackAcpAgentId = target.hermes_installed ? "hermes" : null;

  const useExistingEngine = Boolean(
    existing &&
      (existing.enabled ||
        existing.router_agent_id ||
        existing.provider !== "acp" ||
        target.hermes_installed),
  );

  return {
    enabled: true,
    router_agent_id: existing?.router_agent_id ?? target.active_router_agent_id ?? null,
    provider: useExistingEngine && existing ? existing.provider : fallbackProvider,
    acp_agent_id:
      useExistingEngine && existing ? (existing.acp_agent_id ?? null) : fallbackAcpAgentId,
    model: existing?.model ?? null,
    effort: existing?.effort ?? null,
    cwd: existing?.cwd ?? null,
    scope: existing?.scope ?? "global_fleet",
    pre_review_mode: existing?.pre_review_mode ?? "medium_risk_only",
    share_host_hermes_memory: existing?.share_host_hermes_memory ?? true,
    soul_prompt_override: existing?.soul_prompt_override ?? null,
    external_webhook_url: existing?.external_webhook_url ?? null,
    has_external_webhook_secret: existing?.has_external_webhook_secret ?? false,
  };
}

export function formatBriefingElapsed(tsMs: number, nowMs: number): string {
  if (!tsMs || !Number.isFinite(tsMs)) return "";
  const sec = Math.max(0, Math.floor((nowMs - tsMs) / 1000));
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h`;
  return `${Math.floor(hr / 24)}d`;
}

export function computeMissionDiffTotals(
  mission: BriefingMission,
  stages: Readonly<Record<string, WindowedAgentStageView>>,
): { additions: number; deletions: number } {
  let additions = 0;
  let deletions = 0;
  const seen = new Set<string>();
  const workers = [mission.root, ...mission.workers];
  for (const w of workers) {
    const key = `${w.host_id}::${w.agent_id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    const diffs = stages[key]?.session_turn_diffs;
    if (!diffs) continue;
    for (const d of diffs) {
      additions += d.additions;
      deletions += d.deletions;
    }
  }
  return { additions, deletions };
}
