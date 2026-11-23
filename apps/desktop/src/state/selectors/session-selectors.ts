import type {
  AgentCardView,
  ConditionalTimerRecord,
  DiffFileView,
  GoalRunRecord,
  PendingPermissionSummary,
  SessionBackgroundTaskView,
  SessionSubagentEntryView,
  SessionTouchedFileView,
  TimelineItemView,
} from "../../lib/orbit-ipc.ts";
import { normalizeCwdForCompare } from "../../lib/stage-cache.ts";
import {
  type ResolvedSubagentItem,
  filterVisibleSubagents,
  isSubagentActiveStatus,
  resolveSessionSubagents,
} from "../../lib/subagents.ts";
import {
  type SubagentCharacterSpec,
  assignSubagentCharacters,
} from "../../lib/subagent-avatar.ts";
import type { OrbitUiState } from "../store-types.ts";

function isAgentBackgroundWaiting(
  agent: Pick<AgentCardView, "has_background_tasks" | "status_label"> | null | undefined,
): boolean {
  if (!agent) return false;
  if (typeof agent.has_background_tasks === "boolean") {
    return agent.has_background_tasks;
  }
  const lbl = agent.status_label.trim();
  return /^running\s*\(.*background task/i.test(lbl) || /^background task$/i.test(lbl);
}

export interface SessionSubagentWithAvatar extends ResolvedSubagentItem {
  readonly character: SubagentCharacterSpec;
}

export interface SessionSubagentsSummary {
  readonly all: readonly SessionSubagentWithAvatar[];
  readonly visible: readonly SessionSubagentWithAvatar[];
  readonly activeCount: number;
  readonly awaitingCount: number;
  readonly settledCount: number;
  readonly characterByKey: ReadonlyMap<string, SubagentCharacterSpec>;
}

const EMPTY_SUBAGENT_LIST: readonly SessionSubagentWithAvatar[] = Object.freeze([]);
const EMPTY_SUBAGENT_SUMMARY: SessionSubagentsSummary = Object.freeze({
  all: EMPTY_SUBAGENT_LIST,
  visible: EMPTY_SUBAGENT_LIST,
  activeCount: 0,
  awaitingCount: 0,
  settledCount: 0,
  characterByKey: new Map(),
});

const EMPTY_INSIGHT_SUBAGENTS: readonly SessionSubagentEntryView[] = Object.freeze([]);

/**
 * Single source of truth for a conversation's subagents (managed + provider-native + cross-host),
 * including collision-free OneWorks 3D character assignment and archive filtering.
 */
export function selectSessionSubagents(
  state: Pick<OrbitUiState, "stages" | "hostDetails" | "fleet" | "archivedSubagents">,
  hostId: string | null,
  agentId: string | null,
): SessionSubagentsSummary {
  if (!hostId || !agentId) return EMPTY_SUBAGENT_SUMMARY;

  const stage = state.stages[`${hostId}::${agentId}`];
  const insightsSubagents = stage?.insights?.subagents ?? EMPTY_INSIGHT_SUBAGENTS;

  const resolved = resolveSessionSubagents({
    hostId,
    agentId,
    insightsSubagents,
    hostDetails: state.hostDetails,
    fleetHosts: state.fleet.hosts,
  });

  if (resolved.length === 0) return EMPTY_SUBAGENT_SUMMARY;

  const characterByKey = assignSubagentCharacters(
    resolved,
    (sa) => sa.childAgentId || sa.providerSubagentId || sa.callId || sa.key,
  );

  const all: SessionSubagentWithAvatar[] = resolved.map((sa) => ({
    ...sa,
    character: characterByKey.get(sa.key)!,
  }));

  const visible = filterVisibleSubagents(all, state.archivedSubagents, hostId, agentId);

  let activeCount = 0;
  let awaitingCount = 0;
  let settledCount = 0;
  for (const sa of visible) {
    if (sa.status === "awaiting_permission") {
      awaitingCount += 1;
      activeCount += 1;
    } else if (isSubagentActiveStatus(sa.status)) {
      activeCount += 1;
    } else {
      settledCount += 1;
    }
  }

  return {
    all,
    visible,
    activeCount,
    awaitingCount,
    settledCount,
    characterByKey,
  };
}

export type PendingPermissionTimelineItem = Extract<
  TimelineItemView,
  { item_type: "permission_request" }
>;
export type PendingQuestionTimelineItem = Extract<
  TimelineItemView,
  { item_type: "question_request" }
>;

export interface UnifiedPendingSubagentPermission {
  readonly kind: "subagent_permission";
  readonly key: string;
  readonly hostId: string;
  readonly agentId: string;
  readonly subagent: SessionSubagentWithAvatar;
  readonly permission: PendingPermissionSummary;
}

export interface UnifiedPendingDecisions {
  readonly parentPermissions: readonly PendingPermissionTimelineItem[];
  readonly parentQuestions: readonly PendingQuestionTimelineItem[];
  readonly subagentPermissions: readonly UnifiedPendingSubagentPermission[];
  readonly totalCount: number;
}

const EMPTY_DECISIONS: UnifiedPendingDecisions = Object.freeze({
  parentPermissions: Object.freeze([]),
  parentQuestions: Object.freeze([]),
  subagentPermissions: Object.freeze([]),
  totalCount: 0,
});

/**
 * Single source of truth for all pending approvals and clarifications belonging to
 * the active conversation AND any of its managed child subagents across all hosts.
 */
export function selectPendingDecisions(
  state: Pick<OrbitUiState, "stages" | "hostDetails" | "fleet" | "archivedSubagents">,
  hostId: string | null,
  agentId: string | null,
): UnifiedPendingDecisions {
  if (!hostId || !agentId) return EMPTY_DECISIONS;

  const stage = state.stages[`${hostId}::${agentId}`];
  const parentPermissions = (stage?.pending_permissions ?? []).filter(
    (item): item is PendingPermissionTimelineItem =>
      item.item_type === "permission_request" && item.resolved_decision === null,
  );
  const parentQuestions = (stage?.pending_questions ?? []).filter(
    (item): item is PendingQuestionTimelineItem =>
      item.item_type === "question_request" && item.resolved_answers === null,
  );

  const subagents = selectSessionSubagents(state, hostId, agentId);
  const subagentPermissions: UnifiedPendingSubagentPermission[] = [];
  const seenReqIds = new Set<string>();

  for (const sa of subagents.all) {
    if (!sa.isManaged || !sa.childAgentId) continue;
    const childDetail = state.hostDetails[sa.childHostId];
    const pendingList = childDetail?.pending_permissions ?? [];
    for (const perm of pendingList) {
      if (perm.agent_id !== sa.childAgentId) continue;
      const dedupeKey = `${sa.childHostId}::${sa.childAgentId}::${perm.request_id}`;
      if (seenReqIds.has(dedupeKey)) continue;
      seenReqIds.add(dedupeKey);
      subagentPermissions.push({
        kind: "subagent_permission",
        key: dedupeKey,
        hostId: sa.childHostId,
        agentId: sa.childAgentId,
        subagent: sa,
        permission: perm,
      });
    }
  }

  const totalCount =
    parentPermissions.length + parentQuestions.length + subagentPermissions.length;

  if (totalCount === 0) return EMPTY_DECISIONS;

  return {
    parentPermissions,
    parentQuestions,
    subagentPermissions,
    totalCount,
  };
}

export interface WorkspaceToolSummary {
  readonly reviewScope: "working_tree" | "session_turns";
  readonly reviewCount: number;
  readonly reviewAdditions: number;
  readonly reviewDeletions: number;
  readonly artifactCount: number;
  readonly terminalCount: number;
  readonly showGroupNotes: boolean;
  readonly groupBoardRevision: number;
  readonly groupTitle: string | null;
  readonly workspaceId: string | null;
  readonly touchedFiles: readonly SessionTouchedFileView[];
  readonly touchedAdditions: number;
  readonly touchedDeletions: number;
}

const EMPTY_TOUCHED_FILES: readonly SessionTouchedFileView[] = [];

/**
 * Single source of truth for workspace tool counters (Review Diff, Artifacts, Terminal,
 * Group Notes, Touched Files) consumed by Overview HUD without duplicate calculations.
 */
export function selectWorkspaceToolSummary(
  state: Pick<OrbitUiState, "hostDetails" | "gitReviews" | "terminalTabs" | "stages">,
  hostId: string | null,
  agentId: string | null,
  cwd: string,
  sessionTurnDiffs: readonly DiffFileView[] = [],
  currentAgent?: AgentCardView | null,
): WorkspaceToolSummary {
  const hostDetail = hostId ? (state.hostDetails[hostId] ?? null) : null;
  const resolvedAgent =
    currentAgent ??
    (hostId && agentId
      ? (hostDetail?.agents.find((a) => a.agent_id === agentId) ??
        state.stages[`${hostId}::${agentId}`]?.agent ??
        null)
      : null);

  // Resolve git review by exact or normalized CWD
  let gitReview = hostId && cwd ? (state.gitReviews[`${hostId}::${cwd}`] ?? null) : null;
  if (!gitReview && hostId && cwd) {
    const norm = normalizeCwdForCompare(cwd);
    if (norm) {
      const prefix = `${hostId}::`;
      for (const [key, val] of Object.entries(state.gitReviews)) {
        if (key.startsWith(prefix) && normalizeCwdForCompare(key.slice(prefix.length)) === norm) {
          gitReview = val;
          break;
        }
      }
    }
  }

  let reviewScope: "working_tree" | "session_turns" = "working_tree";
  let reviewCount = 0;
  let reviewAdditions = 0;
  let reviewDeletions = 0;

  const gitFiles = gitReview?.diff_files ?? [];
  const statusFiles = gitReview?.status?.files ?? [];
  if (statusFiles.length > 0 || gitFiles.length > 0) {
    const diffByPath = new Map(gitFiles.map((f) => [f.path, f]));
    const mergedMap = new Map<string, { additions: number; deletions: number }>();
    for (const sf of statusFiles) {
      const df = diffByPath.get(sf.path);
      if (!mergedMap.has(sf.path)) {
        mergedMap.set(sf.path, {
          additions: df?.additions ?? 0,
          deletions: df?.deletions ?? 0,
        });
      }
    }
    for (const df of gitFiles) {
      if (!mergedMap.has(df.path)) {
        mergedMap.set(df.path, { additions: df.additions, deletions: df.deletions });
      }
    }
    reviewCount = mergedMap.size;
    for (const f of mergedMap.values()) {
      reviewAdditions += f.additions;
      reviewDeletions += f.deletions;
    }
  } else if (sessionTurnDiffs.length > 0) {
    reviewScope = "session_turns";
    reviewCount = sessionTurnDiffs.length;
    for (const f of sessionTurnDiffs) {
      reviewAdditions += f.additions;
      reviewDeletions += f.deletions;
    }
  }

  const allArtifacts = hostDetail?.artifacts ?? [];
  const artifactCount =
    allArtifacts.length === 0
      ? 0
      : resolvedAgent?.workspace_id
        ? allArtifacts.filter((a) => a.workspace_id === resolvedAgent.workspace_id).length
        : allArtifacts.length;

  const openTerminalsForHost = hostId
    ? state.terminalTabs.filter((t) => t.hostId === hostId)
    : [];
  const openIds = new Set(openTerminalsForHost.map((t) => t.terminalId).filter(Boolean));
  const detachedTerminals = (hostDetail?.terminals ?? []).filter(
    (t) => !openIds.has(t.terminal_id),
  ).length;
  const terminalCount = openTerminalsForHost.length + detachedTerminals;

  const currentWorkspace =
    hostDetail && resolvedAgent
      ? (hostDetail.workspaces.find((w) => w.workspace_id === resolvedAgent.workspace_id) ?? null)
      : null;
  const groupBoardRevision =
    (resolvedAgent
      ? hostDetail?.workspace_boards?.[resolvedAgent.workspace_id]?.revision
      : 0) ??
    currentWorkspace?.board_revision ??
    0;
  const showGroupNotes = Boolean(
    resolvedAgent &&
      !resolvedAgent.parent_agent_id &&
      (currentWorkspace?.is_curated || groupBoardRevision > 0),
  );

  const stage = hostId && agentId ? state.stages[`${hostId}::${agentId}`] : undefined;
  const touchedFiles = stage?.insights?.touched_files ?? EMPTY_TOUCHED_FILES;
  let touchedAdditions = 0;
  let touchedDeletions = 0;
  for (const tf of touchedFiles) {
    touchedAdditions += tf.additions;
    touchedDeletions += tf.deletions;
  }

  return {
    reviewScope,
    reviewCount,
    reviewAdditions,
    reviewDeletions,
    artifactCount,
    terminalCount,
    showGroupNotes,
    groupBoardRevision,
    groupTitle: currentWorkspace?.title ?? null,
    workspaceId: resolvedAgent?.workspace_id ?? null,
    touchedFiles,
    touchedAdditions,
    touchedDeletions,
  };
}

export type OverviewIndicatorTone = "none" | "approval" | "running" | "goal";

export interface SessionRuntimeActivity {
  readonly activeGoal: GoalRunRecord | null;
  readonly armedTimers: readonly ConditionalTimerRecord[];
  readonly backgroundTasks: readonly SessionBackgroundTaskView[];
  readonly hasRunningBgTask: boolean;
  readonly overviewIndicatorTone: OverviewIndicatorTone;
}

const EMPTY_TIMERS: readonly ConditionalTimerRecord[] = [];
const EMPTY_BG_TASKS: readonly SessionBackgroundTaskView[] = [];

const ACTIVE_GOAL_STATUSES = new Set<GoalRunRecord["status"]>([
  "running",
  "verifying",
  "paused",
]);

/**
 * Single source of truth for active Goal, armed Timers, Background Tasks, and
 * Overview toggle indicator tone (used by both TopBar & Overview HUD).
 */
export function selectSessionRuntimeActivity(
  state: Pick<OrbitUiState, "hostDetails" | "stages" | "fleet" | "archivedSubagents">,
  hostId: string | null,
  agentId: string | null,
): SessionRuntimeActivity {
  if (!hostId || !agentId) {
    return {
      activeGoal: null,
      armedTimers: EMPTY_TIMERS,
      backgroundTasks: EMPTY_BG_TASKS,
      hasRunningBgTask: false,
      overviewIndicatorTone: "none",
    };
  }

  const hostDetail = state.hostDetails[hostId] ?? null;
  const ag = hostDetail?.agents.find((a) => a.agent_id === agentId) ?? null;
  const stage = state.stages[`${hostId}::${agentId}`];

  const goalsForAgent = (hostDetail?.goal_runs ?? []).filter((g) => g.agent_id === agentId);
  const activeGoal =
    goalsForAgent.find((g) => ACTIVE_GOAL_STATUSES.has(g.status)) ?? null;

  const armedTimers = (hostDetail?.conditional_timers ?? []).filter(
    (t) => t.agent_id === agentId && t.state === "armed",
  );

  const backgroundTasks = stage?.insights?.background_tasks ?? EMPTY_BG_TASKS;
  const hasRunningBgTask =
    isAgentBackgroundWaiting(ag) || backgroundTasks.some((t) => t.status === "running");

  const subagents = selectSessionSubagents(state, hostId, agentId);
  let overviewIndicatorTone: OverviewIndicatorTone = "none";
  if (subagents.awaitingCount > 0) {
    overviewIndicatorTone = "approval";
  } else if (subagents.activeCount > 0 || hasRunningBgTask) {
    overviewIndicatorTone = "running";
  } else if (activeGoal || armedTimers.length > 0) {
    overviewIndicatorTone = "goal";
  }

  return {
    activeGoal,
    armedTimers,
    backgroundTasks,
    hasRunningBgTask,
    overviewIndicatorTone,
  };
}

export interface FleetHealthSummary {
  readonly totalHosts: number;
  readonly onlineDirectCount: number;
  readonly onlineRelayCount: number;
  readonly connectingCount: number;
  readonly offlineCount: number;
  readonly totalPendingApprovals: number;
}

export function selectFleetHealth(
  state: Pick<OrbitUiState, "fleet">,
): FleetHealthSummary {
  let onlineDirectCount = 0;
  let onlineRelayCount = 0;
  let connectingCount = 0;
  let offlineCount = 0;

  for (const host of state.fleet.hosts) {
    switch (host.connection_state) {
      case "online_direct":
        onlineDirectCount += 1;
        break;
      case "online_relay":
        onlineRelayCount += 1;
        break;
      case "connecting_direct":
      case "connecting_relay":
        connectingCount += 1;
        break;
      default:
        offlineCount += 1;
        break;
    }
  }

  return {
    totalHosts: state.fleet.hosts.length,
    onlineDirectCount,
    onlineRelayCount,
    connectingCount,
    offlineCount,
    totalPendingApprovals: state.fleet.total_pending_approvals,
  };
}
