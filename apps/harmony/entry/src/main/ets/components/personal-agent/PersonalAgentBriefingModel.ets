import {
  AgentCardDto,
  BriefingDecisionDto,
  BriefingMissionDto,
  BriefingWaitItemDto,
  BriefingWorkerDto,
  HostDetailDto,
  PermissionBatchItemDto,
  PersonalAgentActivityEntryDto,
  PersonalAgentBriefingDto,
  PersonalRouterConfigDto,
  ResolvePermissionsBatchResultDto,
  RouterHostCapabilitiesDto
} from '../../model/OrbitTypes';
import { mapBriefingStateToListStatus } from '../../model/OrbitAgentDisplayModel';

export interface PersonalAgentIdentityRef {
  readonly hostId: string;
  readonly agentId: string;
}

export interface SinceLastVisitSummary {
  readonly completed: number;
  readonly autoHandled: number;
  readonly hasSummary: boolean;
}

export interface FrozenBatchApprovalTarget {
  readonly hostId: string;
  readonly agentId: string;
  readonly requestId: string;
}

export interface BatchApprovalHostOutcome {
  readonly hostId: string;
  readonly resolvedCount: number;
  readonly skippedCount: number;
  readonly firstSkipReason: string;
  readonly failed: boolean;
}

export interface BatchApprovalAggregateSummary {
  readonly resolvedCount: number;
  readonly skippedCount: number;
  readonly failedHostCount: number;
  readonly firstSkipReason: string;
}

export type BatchApprovalFeedbackKind =
  | 'none'
  | 'approved_all'
  | 'approved_with_skipped'
  | 'partial_failure'
  | 'skipped_and_failed'
  | 'all_skipped'
  | 'failed';

export interface BatchApprovalFeedbackPlan {
  readonly kind: BatchApprovalFeedbackKind;
  readonly isError: boolean;
  readonly resolvedCount: number;
  readonly skippedCount: number;
  readonly failedHostCount: number;
  readonly firstSkipReason: string;
}

export interface BriefingPresentationState {
  readonly briefingViewingOpen: boolean;
  readonly showBriefingSheet: boolean;
  readonly showBriefingPanel: boolean;
}

export type CompactTimeKind =
  | 'empty'
  | 'just_now'
  | 'sec'
  | 'min'
  | 'hr'
  | 'day'
  | 'week'
  | 'min_sec'
  | 'hr_min';

export interface CompactTimeSpec {
  readonly kind: CompactTimeKind;
  readonly primary: number;
  readonly secondary: number;
}

export interface CompactTimeTemplates {
  readonly justNow: string;
  readonly secondsShort: string;
  readonly minutesShort: string;
  readonly hoursShort: string;
  readonly daysShort: string;
  readonly weeksShort: string;
  readonly minutesSecondsShort: string;
  readonly hoursMinutesShort: string;
}

export const EN_COMPACT_TIME_TEMPLATES: CompactTimeTemplates = {
  justNow: 'now',
  secondsShort: '%ds',
  minutesShort: '%dm',
  hoursShort: '%dh',
  daysShort: '%dd',
  weeksShort: '%dw',
  minutesSecondsShort: '%1$dm %2$ds',
  hoursMinutesShort: '%1$dh %2$dm'
};

export interface PartitionedBriefingWorker {
  readonly key: string;
  readonly worker: BriefingWorkerDto;
  readonly status: string;
  readonly indentLevel: number;
  readonly depth: number;
  readonly parentHostId: string | null;
  readonly archivable: boolean;
  readonly fromOtherActive: boolean;
}

export interface PartitionedBriefingWorkers {
  readonly runningMissionWorkers: PartitionedBriefingWorker[];
  readonly runningOtherWorkers: PartitionedBriefingWorker[];
  readonly completedWorkers: PartitionedBriefingWorker[];
  readonly archivableCompletedWorkers: PartitionedBriefingWorker[];
  readonly inProgressCount: number;
}

export interface InProgressPaginationResult {
  readonly visibleMissionWorkers: PartitionedBriefingWorker[];
  readonly visibleOtherWorkers: PartitionedBriefingWorker[];
  readonly hiddenCount: number;
  readonly showToggle: boolean;
}

export function briefingWorkerKey(hostId: string, agentId: string): string {
  return `${hostId}::${agentId}`;
}

export function batchApprovalTargetKey(hostId: string, agentId: string, requestId: string): string {
  return `${hostId}::${agentId}::${requestId}`;
}

/**
 * Freeze the exact `(hostId, agentId, requestId)` targets displayed in the confirmation alert
 * so permissions arriving while the dialog is open are NEVER approved without explicit user review.
 */
export function snapshotBatchApprovalTargets(
  items: readonly BriefingDecisionDto[]
): FrozenBatchApprovalTarget[] {
  const seen = new Set<string>();
  const out: FrozenBatchApprovalTarget[] = [];
  for (let i = 0; i < items.length; i += 1) {
    const item = items[i];
    if (
      item.kind !== 'permission' ||
      !item.batch_eligible ||
      item.human_only ||
      item.request_id === null ||
      item.request_id.trim().length === 0
    ) {
      continue;
    }
    const reqId = item.request_id.trim();
    const key = batchApprovalTargetKey(item.host_id, item.agent_id, reqId);
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    out.push({
      hostId: item.host_id,
      agentId: item.agent_id,
      requestId: reqId
    });
  }
  return out;
}

/**
 * Intersect the frozen targets confirmed by the user with the live eligible `needs_you` set.
 * - Newly arrived items not in `frozen` are excluded (left for a future confirmation).
 * - Canceled/replaced items no longer present or no longer batch-eligible are excluded.
 */
export function intersectFrozenBatchTargets(
  frozen: readonly FrozenBatchApprovalTarget[],
  currentNeedsYou: readonly BriefingDecisionDto[]
): Map<string, PermissionBatchItemDto[]> {
  const frozenKeys = new Set<string>();
  for (let i = 0; i < frozen.length; i += 1) {
    const t = frozen[i];
    if (t.hostId.length > 0 && t.agentId.length > 0 && t.requestId.length > 0) {
      frozenKeys.add(batchApprovalTargetKey(t.hostId, t.agentId, t.requestId));
    }
  }
  const byHost = new Map<string, PermissionBatchItemDto[]>();
  if (frozenKeys.size === 0) {
    return byHost;
  }
  const emittedKeys = new Set<string>();
  for (let i = 0; i < currentNeedsYou.length; i += 1) {
    const item = currentNeedsYou[i];
    if (
      item.kind !== 'permission' ||
      !item.batch_eligible ||
      item.human_only ||
      item.request_id === null ||
      item.request_id.trim().length === 0
    ) {
      continue;
    }
    const reqId = item.request_id.trim();
    const key = batchApprovalTargetKey(item.host_id, item.agent_id, reqId);
    if (!frozenKeys.has(key) || emittedKeys.has(key)) {
      continue;
    }
    emittedKeys.add(key);
    const list = byHost.get(item.host_id) ?? [];
    list.push({
      agent_id: item.agent_id,
      permission_request_id: reqId
    });
    byHost.set(item.host_id, list);
  }
  return byHost;
}

export function parseBatchApprovalHostResponse(
  hostId: string,
  rawJson: string
): BatchApprovalHostOutcome {
  try {
    const parsed = JSON.parse(rawJson) as ResolvePermissionsBatchResultDto;
    const resolved = typeof parsed.resolved_count === 'number' && Number.isFinite(parsed.resolved_count)
      ? Math.max(0, Math.floor(parsed.resolved_count))
      : 0;
    const skipped = typeof parsed.skipped_count === 'number' && Number.isFinite(parsed.skipped_count)
      ? Math.max(0, Math.floor(parsed.skipped_count))
      : 0;
    let firstReason = '';
    const reasons = parsed.skipped_reasons ?? [];
    if (reasons.length > 0 && typeof reasons[0].reason === 'string') {
      firstReason = reasons[0].reason.trim();
    }
    return {
      hostId: hostId,
      resolvedCount: resolved,
      skippedCount: skipped,
      firstSkipReason: firstReason,
      failed: false
    };
  } catch (_err) {
    return {
      hostId: hostId,
      resolvedCount: 0,
      skippedCount: 0,
      firstSkipReason: '',
      failed: true
    };
  }
}

export function aggregateBatchApprovalOutcomes(
  outcomes: readonly BatchApprovalHostOutcome[]
): BatchApprovalAggregateSummary {
  let resolvedCount = 0;
  let skippedCount = 0;
  let failedHostCount = 0;
  let firstSkipReason = '';
  for (let i = 0; i < outcomes.length; i += 1) {
    const o = outcomes[i];
    if (o.failed) {
      failedHostCount += 1;
      continue;
    }
    resolvedCount += o.resolvedCount;
    skippedCount += o.skippedCount;
    if (firstSkipReason.length === 0 && o.firstSkipReason.length > 0) {
      firstSkipReason = o.firstSkipReason;
    }
  }
  return {
    resolvedCount: resolvedCount,
    skippedCount: skippedCount,
    failedHostCount: failedHostCount,
    firstSkipReason: firstSkipReason
  };
}

export function classifyBatchApprovalFeedback(
  summary: BatchApprovalAggregateSummary
): BatchApprovalFeedbackPlan {
  if (summary.failedHostCount > 0) {
    if (summary.resolvedCount > 0) {
      return {
        kind: 'partial_failure',
        isError: true,
        resolvedCount: summary.resolvedCount,
        skippedCount: summary.skippedCount,
        failedHostCount: summary.failedHostCount,
        firstSkipReason: summary.firstSkipReason
      };
    }
    if (summary.skippedCount > 0) {
      return {
        kind: 'skipped_and_failed',
        isError: true,
        resolvedCount: 0,
        skippedCount: summary.skippedCount,
        failedHostCount: summary.failedHostCount,
        firstSkipReason: summary.firstSkipReason
      };
    }
    return {
      kind: 'failed',
      isError: true,
      resolvedCount: 0,
      skippedCount: 0,
      failedHostCount: summary.failedHostCount,
      firstSkipReason: summary.firstSkipReason
    };
  }
  if (summary.resolvedCount > 0 && summary.skippedCount === 0) {
    return {
      kind: 'approved_all',
      isError: false,
      resolvedCount: summary.resolvedCount,
      skippedCount: 0,
      failedHostCount: 0,
      firstSkipReason: ''
    };
  }
  if (summary.resolvedCount > 0) {
    return {
      kind: 'approved_with_skipped',
      isError: false,
      resolvedCount: summary.resolvedCount,
      skippedCount: summary.skippedCount,
      failedHostCount: 0,
      firstSkipReason: summary.firstSkipReason
    };
  }
  if (summary.skippedCount > 0) {
    return {
      kind: 'all_skipped',
      isError: true,
      resolvedCount: 0,
      skippedCount: summary.skippedCount,
      failedHostCount: 0,
      firstSkipReason: summary.firstSkipReason
    };
  }
  return {
    kind: 'none',
    isError: false,
    resolvedCount: 0,
    skippedCount: 0,
    failedHostCount: 0,
    firstSkipReason: ''
  };
}

/**
 * Reconcile exclusive Personal Agent Briefing presentation bidirectionally across the 1024 vp threshold.
 * - Wide (`>= 1024 vp`): docked 304 vp rail (`showBriefingPanel`), Sheet always closed.
 * - Compact (`< 1024 vp`): 100% full-height Sheet (`showBriefingSheet`), docked rail always closed.
 * - Preserves `briefingViewingOpen` across `1023 -> 1024 -> 1023 vp` transitions and stays closed when dismissed.
 */
export function reconcileBriefingPresentationOnResize(
  prevWidthVp: number,
  nextWidthVp: number,
  isPaSession: boolean,
  showBriefingSheet: boolean,
  showBriefingPanel: boolean,
  expandedMinVp: number = 1024,
  briefingViewingOpen?: boolean
): BriefingPresentationState {
  const prevWide = prevWidthVp >= expandedMinVp;
  const nextWide = nextWidthVp >= expandedMinVp;
  const isViewing = briefingViewingOpen !== undefined
    ? briefingViewingOpen
    : (prevWide ? showBriefingPanel : showBriefingSheet);

  if (!isPaSession || !isViewing) {
    return {
      briefingViewingOpen: false,
      showBriefingSheet: false,
      showBriefingPanel: false
    };
  }
  if (nextWide) {
    return {
      briefingViewingOpen: true,
      showBriefingSheet: false,
      showBriefingPanel: true
    };
  }
  return {
    briefingViewingOpen: true,
    showBriefingSheet: true,
    showBriefingPanel: false
  };
}

export interface PaIdentityArrivalPresentation extends BriefingPresentationState {
  readonly shouldCloseDefaultDiffInspector: boolean;
}

/**
 * Resolves Briefing rail presentation when PA identity arrives asynchronously (e.g., cold-start
 * deep link at 1200 vp where the PA session was selected before the first Briefing arrived).
 * Auto-expands the 304 vp docked rail on wide viewports and yields any layout-default Diff rail
 * unless the user explicitly dismissed Briefing for this PA or explicitly opened the Diff inspector.
 */
export function resolvePaIdentityArrivalPresentation(
  windowWidthVp: number,
  isPaSession: boolean,
  briefingViewingOpen: boolean,
  showBriefingPanel: boolean,
  showBriefingSheet: boolean,
  userOpenedDiffInspector: boolean,
  userDismissedCurrentPa: boolean,
  expandedMinVp: number = 1024
): PaIdentityArrivalPresentation {
  if (!isPaSession) {
    return {
      briefingViewingOpen: briefingViewingOpen,
      showBriefingSheet: showBriefingSheet,
      showBriefingPanel: showBriefingPanel,
      shouldCloseDefaultDiffInspector: false
    };
  }
  const isWide = windowWidthVp >= expandedMinVp;
  if (
    isWide &&
    !briefingViewingOpen &&
    !showBriefingPanel &&
    !userOpenedDiffInspector &&
    !userDismissedCurrentPa
  ) {
    return {
      briefingViewingOpen: true,
      showBriefingSheet: false,
      showBriefingPanel: true,
      shouldCloseDefaultDiffInspector: true
    };
  }
  return {
    briefingViewingOpen: briefingViewingOpen,
    showBriefingSheet: showBriefingSheet,
    showBriefingPanel: showBriefingPanel,
    shouldCloseDefaultDiffInspector: false
  };
}

/**
 * Resolves Briefing rail presentation when `selectAgent` selects a Personal Agent session:
 * - On fresh navigation (`sameSession === false`) at wide width (`>= expandedMinVp`), expands the
 *   304 vp docked Briefing rail and yields any layout-default inline Diff inspector.
 * - On same-session reselect (e.g., another deep link targeting the already-active PA), preserves
 *   an explicit user dismissal of the Briefing rail and any user-opened Diff inspector.
 */
export function resolvePaSessionSelectPresentation(
  windowWidthVp: number,
  sameSession: boolean,
  briefingViewingOpen: boolean,
  showBriefingPanel: boolean,
  showBriefingSheet: boolean,
  userDismissedCurrentPa: boolean,
  userOpenedDiffInspector: boolean,
  expandedMinVp: number = 1024
): PaIdentityArrivalPresentation {
  const isWide = windowWidthVp >= expandedMinVp;
  if (isWide) {
    if (sameSession && (userDismissedCurrentPa || userOpenedDiffInspector)) {
      return {
        briefingViewingOpen: briefingViewingOpen,
        showBriefingSheet: showBriefingSheet,
        showBriefingPanel: showBriefingPanel,
        shouldCloseDefaultDiffInspector: false
      };
    }
    return {
      briefingViewingOpen: true,
      showBriefingSheet: false,
      showBriefingPanel: true,
      shouldCloseDefaultDiffInspector: !userOpenedDiffInspector
    };
  }
  if (!briefingViewingOpen) {
    return {
      briefingViewingOpen: false,
      showBriefingSheet: false,
      showBriefingPanel: false,
      shouldCloseDefaultDiffInspector: false
    };
  }
  return {
    briefingViewingOpen: briefingViewingOpen,
    showBriefingSheet: showBriefingSheet,
    showBriefingPanel: showBriefingPanel,
    shouldCloseDefaultDiffInspector: false
  };
}

function findLiveAgentInMap(
  hostDetails: Map<string, HostDetailDto>,
  hostId: string,
  agentId: string
): AgentCardDto | null {
  const detail = hostDetails.get(hostId);
  if (detail === undefined) {
    return null;
  }
  for (let i = 0; i < detail.agents.length; i += 1) {
    if (detail.agents[i].agent_id === agentId) {
      return detail.agents[i];
    }
  }
  return null;
}

function isAgentArchived(liveAgent: AgentCardDto | null): boolean {
  if (liveAgent === null) {
    return false;
  }
  return (
    liveAgent.archived_at_rfc3339 !== undefined &&
    liveAgent.archived_at_rfc3339 !== null &&
    liveAgent.archived_at_rfc3339.length > 0
  );
}

function isLiveAgentFailed(liveAgent: AgentCardDto): boolean {
  const state = liveAgent.status?.state ?? '';
  const lower = (liveAgent.status_label ?? '').toLowerCase();
  return state === 'error' || lower.includes('error') || lower.includes('fail');
}

function isLiveAgentCanceled(liveAgent: AgentCardDto): boolean {
  const state = liveAgent.status?.state ?? '';
  const lower = (liveAgent.status_label ?? '').toLowerCase();
  return (
    state === 'closed' ||
    lower.includes('cancel') ||
    lower.includes('interrupt') ||
    lower.includes('stopped') ||
    lower.includes('closed')
  );
}

function parseLiveAgentUpdatedMs(liveAgent: AgentCardDto | null): number {
  if (liveAgent === null) {
    return 0;
  }
  const raw = liveAgent.updated_at_rfc3339;
  if (typeof raw !== 'string' || raw.trim().length === 0) {
    return 0;
  }
  const ms = Date.parse(raw);
  return Number.isFinite(ms) && ms > 0 ? ms : 0;
}

function isBriefingWorkerRawActive(worker: BriefingWorkerDto): boolean {
  const lower = (worker.state ?? '').trim().toLowerCase();
  return lower === 'running' ||
    lower === 'awaiting' ||
    lower === 'waiting' ||
    lower === 'awaiting_permission';
}

function isBriefingWorkerNewerThanLive(
  worker: BriefingWorkerDto,
  liveAgent: AgentCardDto | null
): boolean {
  if (liveAgent === null) {
    return true;
  }
  const workerMs = typeof worker.updated_at_ms === 'number' && Number.isFinite(worker.updated_at_ms)
    ? worker.updated_at_ms
    : 0;
  if (workerMs <= 0) {
    return false;
  }
  const liveMs = parseLiveAgentUpdatedMs(liveAgent);
  return liveMs <= 0 || workerMs > liveMs;
}

/**
 * Resolves the authoritative live status for a briefing worker by reconciling the Rust
 * briefing projection with the latest `AgentCardDto` from `hostDetails`.
 * Matches Desktop `personal-agent-briefing-model.ts` & Rust `personal_agent.rs:413`.
 *
 * When a newer Briefing snapshot marks a worker active (`running`/`awaiting`) while the cached
 * `HostDetail` is older (`idle`), preserves the active status so stale `HostDetail` NEVER
 * demotes a running worker back to archivable `completed`.
 */
export function resolveEffectiveBriefingWorkerStatus(
  worker: BriefingWorkerDto,
  liveAgent: AgentCardDto | null,
  hasActiveManagedChild: boolean = false
): string {
  if (liveAgent === null) {
    return mapBriefingStateToListStatus(worker.state, null);
  }
  if (
    !hasActiveManagedChild &&
    isBriefingWorkerRawActive(worker) &&
    isBriefingWorkerNewerThanLive(worker, liveAgent) &&
    !isAgentArchived(liveAgent)
  ) {
    const lower = (worker.state ?? '').trim().toLowerCase();
    return (lower === 'awaiting' || lower === 'awaiting_permission')
      ? 'awaiting_permission'
      : 'running';
  }
  const liveState = liveAgent.status?.state ?? '';
  if (liveAgent.is_awaiting_permission || liveState === 'awaiting_permission') {
    return 'awaiting_permission';
  }
  if (isLiveAgentFailed(liveAgent)) {
    return 'failed';
  }
  const isWakeupWaiting =
    liveState === 'autonomous_wakeup' &&
    typeof liveAgent.status?.source === 'string' &&
    liveAgent.status.source.trim().startsWith('waiting:');

  const isForegroundRunning =
    liveAgent.has_background_tasks !== true &&
    !isWakeupWaiting &&
    (liveState === 'initializing' ||
      liveState === 'running' ||
      liveState === 'autonomous_wakeup' ||
      (liveState.length === 0 && liveAgent.is_running));

  if (isForegroundRunning) {
    return 'running';
  }
  // Preserve running state when this worker owns an active provider-native subagent
  // (`has_running_subagents === true` with no active managed descendant projected separately).
  if (
    liveAgent.has_running_subagents === true &&
    !hasActiveManagedChild &&
    worker.state === 'running'
  ) {
    return 'running';
  }
  if (isLiveAgentCanceled(liveAgent)) {
    return 'canceled';
  }
  if (liveAgent.has_background_tasks === true || liveAgent.has_armed_timers === true || isWakeupWaiting) {
    return 'idle';
  }
  if (liveAgent.latest_seq > 0 || worker.state === 'done') {
    return 'completed';
  }
  return 'idle';
}

function isWorkerArchivable(
  worker: BriefingWorkerDto,
  liveAgent: AgentCardDto | null,
  status: string,
  hasActiveSubtreeDescendant: boolean = false,
  lineageUnverifiedOnActiveMission: boolean = false,
  isWaitingInBriefing: boolean = false
): boolean {
  if (
    status === 'running' ||
    status === 'awaiting_permission' ||
    hasActiveSubtreeDescendant ||
    lineageUnverifiedOnActiveMission ||
    isWaitingInBriefing
  ) {
    return false;
  }
  // Conservative safety: if Briefing itself marks this worker active (e.g. stale HostDetail),
  // NEVER offer it for batch archiving.
  if (isBriefingWorkerRawActive(worker)) {
    return false;
  }
  if (liveAgent !== null) {
    if (
      liveAgent.has_background_tasks === true ||
      liveAgent.has_armed_timers === true ||
      liveAgent.has_running_subagents === true
    ) {
      return false;
    }
  }
  return true;
}

function isLiveAgentTreeActive(liveAgent: AgentCardDto): boolean {
  if (isAgentArchived(liveAgent)) {
    return false;
  }
  const state = (liveAgent.status?.state ?? '').trim().toLowerCase();
  return liveAgent.is_running === true ||
    liveAgent.is_awaiting_permission === true ||
    liveAgent.has_running_subagents === true ||
    liveAgent.has_background_tasks === true ||
    liveAgent.has_armed_timers === true ||
    state === 'running' ||
    state === 'autonomous_wakeup' ||
    state === 'initializing' ||
    state === 'awaiting_permission';
}

/**
 * Computes every ancestor `(hostId::agentId)` key that owns at least one non-archived active
 * descendant (managed or provider-native, direct child or multi-hop grandchild, same-host or
 * cross-host) across ALL live `hostDetails` agents and briefing `missions`.
 * Prevents `Archive done` from archiving a completed parent whose cascade archive would kill
 * running managed descendants (matching daemon `manager.rs:6234`).
 */
export function collectAncestorsWithActiveDescendants(
  hostDetails: Map<string, HostDetailDto>,
  missions: readonly BriefingMissionDto[],
  needsYouKeys: ReadonlySet<string>,
  waitingKeys: ReadonlySet<string> = new Set<string>()
): Set<string> {
  const parentOf = new Map<string, string>();
  const activeNodeKeys = new Set<string>();

  hostDetails.forEach((detail: HostDetailDto, hostId: string): void => {
    const agents = detail.agents ?? [];
    for (let i = 0; i < agents.length; i += 1) {
      const ag = agents[i];
      const childKey = briefingWorkerKey(hostId, ag.agent_id);
      if (ag.parent_agent_id !== null && ag.parent_agent_id.length > 0) {
        const pHost = (
          ag.parent_host_id !== undefined &&
          ag.parent_host_id !== null &&
          ag.parent_host_id.length > 0
        ) ? ag.parent_host_id : hostId;
        parentOf.set(childKey, briefingWorkerKey(pHost, ag.parent_agent_id));
      }
      if (
        !isAgentArchived(ag) &&
        (isLiveAgentTreeActive(ag) || needsYouKeys.has(childKey) || waitingKeys.has(childKey))
      ) {
        activeNodeKeys.add(childKey);
      }
    }
  });

  for (let i = 0; i < missions.length; i += 1) {
    const mission = missions[i];
    const rootKey = briefingWorkerKey(mission.root.host_id, mission.root.agent_id);
    for (let j = 0; j < mission.workers.length; j += 1) {
      const w = mission.workers[j];
      const wKey = briefingWorkerKey(w.host_id, w.agent_id);
      if (!parentOf.has(wKey) && wKey !== rootKey) {
        parentOf.set(wKey, rootKey);
      }
      const liveW = findLiveAgentInMap(hostDetails, w.host_id, w.agent_id);
      const briefingSaysActive = isBriefingWorkerRawActive(w) || waitingKeys.has(wKey);
      if (liveW !== null) {
        if (
          !isAgentArchived(liveW) &&
          (isLiveAgentTreeActive(liveW) || briefingSaysActive || needsYouKeys.has(wKey))
        ) {
          activeNodeKeys.add(wKey);
        }
      } else if (briefingSaysActive || needsYouKeys.has(wKey)) {
        activeNodeKeys.add(wKey);
      }
    }
  }

  waitingKeys.forEach((wKey: string): void => {
    activeNodeKeys.add(wKey);
  });

  const ancestorsWithActive = new Set<string>();
  const markAncestors = (startKey: string): void => {
    const visited = new Set<string>([startKey]);
    let curr = parentOf.get(startKey);
    while (curr !== undefined && !visited.has(curr)) {
      visited.add(curr);
      ancestorsWithActive.add(curr);
      curr = parentOf.get(curr);
    }
  };
  activeNodeKeys.forEach((nodeKey: string): void => {
    markAncestors(nodeKey);
  });
  return ancestorsWithActive;
}

function computeMissionWorkerDepth(
  worker: BriefingWorkerDto,
  rootKey: string,
  agentByKey: Map<string, AgentCardDto>
): number {
  const startKey = briefingWorkerKey(worker.host_id, worker.agent_id);
  if (startKey === rootKey) {
    return 0;
  }
  let depth = 1;
  let curr = agentByKey.get(startKey);
  const visited = new Set<string>([startKey]);
  while (curr !== undefined && depth < 6) {
    const parentId = curr.parent_agent_id;
    if (parentId === null || parentId.length === 0) {
      break;
    }
    const parentHost = (curr.parent_host_id !== undefined && curr.parent_host_id !== null && curr.parent_host_id.length > 0)
      ? curr.parent_host_id
      : worker.host_id;
    const parentKey = briefingWorkerKey(parentHost, parentId);
    if (parentKey === rootKey || visited.has(parentKey)) {
      break;
    }
    visited.add(parentKey);
    const parentCard = agentByKey.get(parentKey);
    if (parentCard === undefined) {
      break;
    }
    depth += 1;
    curr = parentCard;
  }
  return depth;
}

interface ChildWorkerEntry {
  readonly worker: BriefingWorkerDto;
  readonly key: string;
  readonly liveAgent: AgentCardDto | null;
  readonly status: string;
  readonly depth: number;
  readonly hasActiveManagedChild: boolean;
}

/**
 * Returns `true` when any active non-root worker in `mission` lacks a complete verified
 * `parent_agent_id` chain in `hostDetails` (e.g., a newly spawned `R/leaf(running)` whose card
 * has not yet arrived in `R`'s cached `HostDetail`, even if `R` already caches `middle`, OR an
 * intermediate non-root parent card missing from `hostDetails`). In that case, ANY other worker
 * in the mission may be an ancestor whose archive would cascade-stop the active descendant
 * (daemon `manager.rs:6234`), so all potential ancestors MUST be conservatively excluded.
 */
function hasUnverifiedActiveLineageInMission(
  mission: BriefingMissionDto,
  rootKey: string,
  liveByKey: Map<string, AgentCardDto>,
  hostDetails: Map<string, HostDetailDto>,
  needsYouKeys: ReadonlySet<string>,
  waitingKeys: ReadonlySet<string>
): boolean {
  for (let j = 0; j < mission.workers.length; j += 1) {
    const child = mission.workers[j];
    const childKey = briefingWorkerKey(child.host_id, child.agent_id);
    if (childKey === rootKey) {
      continue;
    }
    const liveChild = liveByKey.get(childKey) ?? null;
    const effectiveStatus = resolveEffectiveBriefingWorkerStatus(child, liveChild, false);
    const isActive =
      effectiveStatus === 'running' ||
      effectiveStatus === 'awaiting_permission' ||
      isBriefingWorkerRawActive(child) ||
      needsYouKeys.has(childKey) ||
      waitingKeys.has(childKey) ||
      (liveChild !== null && !isAgentArchived(liveChild) && isLiveAgentTreeActive(liveChild));
    if (!isActive) {
      continue;
    }
    // Active/waiting worker's own host or card is missing from hostDetails -> parent_agent_id is unknown!
    if (!hostDetails.has(child.host_id) || liveChild === null) {
      return true;
    }
    // Walk parent_agent_id chain up to rootKey; if any non-root intermediate card is missing,
    // we cannot verify the complete set of ancestors above this active/waiting worker.
    let curr: AgentCardDto = liveChild;
    let currHost = child.host_id;
    const visited = new Set<string>([childKey]);
    while (true) {
      const parentId = (curr.parent_agent_id ?? '').trim();
      if (parentId.length === 0) {
        break;
      }
      const parentHost = (
        curr.parent_host_id !== undefined &&
        curr.parent_host_id !== null &&
        curr.parent_host_id.trim().length > 0
      ) ? curr.parent_host_id.trim() : currHost;
      const parentKey = briefingWorkerKey(parentHost, parentId);
      if (parentKey === rootKey || visited.has(parentKey)) {
        break;
      }
      visited.add(parentKey);
      const parentCard = findLiveAgentInMap(hostDetails, parentHost, parentId);
      if (parentCard === null) {
        return true;
      }
      curr = parentCard;
      currHost = parentHost;
    }
  }
  return false;
}

export function partitionBriefingWorkers(
  missions: BriefingMissionDto[],
  otherActive: BriefingWorkerDto[],
  needsYou: BriefingDecisionDto[],
  hostDetails: Map<string, HostDetailDto>,
  waiting: readonly BriefingWaitItemDto[] = []
): PartitionedBriefingWorkers {
  const needsYouKeys = new Set<string>();
  for (let i = 0; i < needsYou.length; i += 1) {
    needsYouKeys.add(briefingWorkerKey(needsYou[i].host_id, needsYou[i].agent_id));
  }
  const waitingKeys = new Set<string>();
  const hostsWithUnverifiedWaiting = new Set<string>();
  for (let i = 0; i < waiting.length; i += 1) {
    const w = waiting[i];
    const cleanHost = w.host_id.trim();
    const cleanAgent = w.agent_id.trim();
    if (cleanHost.length > 0 && cleanAgent.length > 0) {
      waitingKeys.add(briefingWorkerKey(cleanHost, cleanAgent));
      if (!hostDetails.has(cleanHost) || findLiveAgentInMap(hostDetails, cleanHost, cleanAgent) === null) {
        hostsWithUnverifiedWaiting.add(cleanHost);
      }
    }
  }
  const ancestorsWithActiveDesc = collectAncestorsWithActiveDescendants(
    hostDetails,
    missions,
    needsYouKeys,
    waitingKeys
  );

  const seenKeys = new Set<string>();
  const runningMissionWorkers: PartitionedBriefingWorker[] = [];
  const completedWorkers: PartitionedBriefingWorker[] = [];

  for (let i = 0; i < missions.length; i += 1) {
    const mission = missions[i];
    const rootKey = briefingWorkerKey(mission.root.host_id, mission.root.agent_id);
    const liveByKey = new Map<string, AgentCardDto>();
    const rootLive = findLiveAgentInMap(hostDetails, mission.root.host_id, mission.root.agent_id);
    if (rootLive !== null) {
      liveByKey.set(rootKey, rootLive);
    }
    for (let j = 0; j < mission.workers.length; j += 1) {
      const child = mission.workers[j];
      const liveChild = findLiveAgentInMap(hostDetails, child.host_id, child.agent_id);
      if (liveChild !== null) {
        liveByKey.set(briefingWorkerKey(child.host_id, child.agent_id), liveChild);
      }
    }

    // Track whether any child has an active managed descendant in this mission.
    const activeChildrenByParent = new Set<string>();
    const preliminaryChildStatuses: string[] = [];
    let hasActiveChild = false;
    for (let j = 0; j < mission.workers.length; j += 1) {
      const child = mission.workers[j];
      const childKey = briefingWorkerKey(child.host_id, child.agent_id);
      const liveChild = liveByKey.get(childKey) ?? null;
      const st = resolveEffectiveBriefingWorkerStatus(child, liveChild, false);
      preliminaryChildStatuses.push(st);
      if (st === 'running' || st === 'awaiting_permission') {
        hasActiveChild = true;
        if (liveChild !== null && liveChild.parent_agent_id !== null && liveChild.parent_agent_id.length > 0) {
          const pHost = (liveChild.parent_host_id !== undefined && liveChild.parent_host_id !== null && liveChild.parent_host_id.length > 0)
            ? liveChild.parent_host_id
            : child.host_id;
          activeChildrenByParent.add(briefingWorkerKey(pHost, liveChild.parent_agent_id));
        }
      }
    }

    const childEntries: ChildWorkerEntry[] = [];
    for (let j = 0; j < mission.workers.length; j += 1) {
      const child = mission.workers[j];
      const childKey = briefingWorkerKey(child.host_id, child.agent_id);
      const liveChild = liveByKey.get(childKey) ?? null;
      const childHasActiveDesc = activeChildrenByParent.has(childKey);
      const childStatus = childHasActiveDesc
        ? resolveEffectiveBriefingWorkerStatus(child, liveChild, true)
        : preliminaryChildStatuses[j];
      const depth = computeMissionWorkerDepth(child, rootKey, liveByKey);
      childEntries.push({
        worker: child,
        key: childKey,
        liveAgent: liveChild,
        status: childStatus,
        depth: depth,
        hasActiveManagedChild: childHasActiveDesc
      });
    }

    const rootStatus = resolveEffectiveBriefingWorkerStatus(mission.root, rootLive, hasActiveChild);
    const rootArchived = isAgentArchived(rootLive);
    const rootInNeedsYou = needsYouKeys.has(rootKey);
    let hasWaitingOrNeedsYouChild = false;
    for (let j = 0; j < childEntries.length; j += 1) {
      const ck = childEntries[j].key;
      if (waitingKeys.has(ck) || needsYouKeys.has(ck)) {
        hasWaitingOrNeedsYouChild = true;
        break;
      }
    }
    const missionHasAnyActive =
      hasActiveChild ||
      hasWaitingOrNeedsYouChild ||
      rootStatus === 'running' ||
      rootStatus === 'awaiting_permission' ||
      ancestorsWithActiveDesc.has(rootKey) ||
      rootInNeedsYou ||
      waitingKeys.has(rootKey);
    const missionHasUnverifiedActiveLineage =
      missionHasAnyActive &&
      hasUnverifiedActiveLineageInMission(
        mission,
        rootKey,
        liveByKey,
        hostDetails,
        needsYouKeys,
        waitingKeys
      );

    let rootInRunning = false;
    let rootInCompleted = false;

    if (!seenKeys.has(rootKey)) {
      seenKeys.add(rootKey);
      if (rootStatus === 'running' || (rootStatus === 'awaiting_permission' && !rootInNeedsYou)) {
        runningMissionWorkers.push({
          key: rootKey,
          worker: mission.root,
          status: rootStatus,
          indentLevel: 0,
          depth: 0,
          parentHostId: null,
          archivable: false,
          fromOtherActive: false
        });
        rootInRunning = true;
      } else if (
        !rootArchived &&
        rootStatus !== 'awaiting_permission' &&
        !(rootStatus === 'failed' && rootInNeedsYou)
      ) {
        const rootLineageUnverified =
          missionHasUnverifiedActiveLineage ||
          hostsWithUnverifiedWaiting.has(mission.root.host_id) ||
          (missionHasAnyActive && (!hostDetails.has(mission.root.host_id) || rootLive === null));
        completedWorkers.push({
          key: rootKey,
          worker: mission.root,
          status: rootStatus,
          indentLevel: 0,
          depth: 0,
          parentHostId: null,
          archivable: isWorkerArchivable(
            mission.root,
            rootLive,
            rootStatus,
            hasActiveChild || ancestorsWithActiveDesc.has(rootKey),
            rootLineageUnverified,
            waitingKeys.has(rootKey)
          ),
          fromOtherActive: false
        });
        rootInCompleted = true;
      }
    }

    for (let j = 0; j < childEntries.length; j += 1) {
      const child = childEntries[j];
      if (seenKeys.has(child.key)) {
        continue;
      }
      seenKeys.add(child.key);
      const childArchived = isAgentArchived(child.liveAgent);
      const childInNeedsYou = needsYouKeys.has(child.key);

      if (child.status === 'running' || (child.status === 'awaiting_permission' && !childInNeedsYou)) {
        runningMissionWorkers.push({
          key: child.key,
          worker: child.worker,
          status: child.status,
          indentLevel: rootInRunning ? Math.min(child.depth, 5) : 0,
          depth: child.depth,
          parentHostId: rootInRunning ? mission.root.host_id : null,
          archivable: false,
          fromOtherActive: false
        });
      } else if (
        !childArchived &&
        child.status !== 'awaiting_permission' &&
        !(child.status === 'failed' && childInNeedsYou)
      ) {
        // Conservatively exclude completed candidates when any active/waiting worker in the mission
        // has an unverified parent chain (e.g., `R` caches `middle` but not newly spawned `leaf`,
        // OR `R` is unloaded while `waiting=[timer:R:leaf]`), so daemon cascade-archive can never
        // stop a live descendant or cancel an armed timer.
        const childLineageUnverified =
          missionHasUnverifiedActiveLineage ||
          hostsWithUnverifiedWaiting.has(child.worker.host_id) ||
          (missionHasAnyActive && (!hostDetails.has(child.worker.host_id) || child.liveAgent === null));
        completedWorkers.push({
          key: child.key,
          worker: child.worker,
          status: child.status,
          indentLevel: rootInCompleted ? Math.min(child.depth, 5) : 0,
          depth: child.depth,
          parentHostId: rootInCompleted ? mission.root.host_id : null,
          archivable: isWorkerArchivable(
            child.worker,
            child.liveAgent,
            child.status,
            child.hasActiveManagedChild || ancestorsWithActiveDesc.has(child.key),
            childLineageUnverified,
            waitingKeys.has(child.key)
          ),
          fromOtherActive: false
        });
      }
    }
  }

  const runningOtherWorkers: PartitionedBriefingWorker[] = [];
  for (let i = 0; i < otherActive.length; i += 1) {
    const worker = otherActive[i];
    const key = briefingWorkerKey(worker.host_id, worker.agent_id);
    if (seenKeys.has(key)) {
      continue;
    }
    seenKeys.add(key);
    const liveAgent = findLiveAgentInMap(hostDetails, worker.host_id, worker.agent_id);
    if (isAgentArchived(liveAgent)) {
      continue;
    }
    const status = resolveEffectiveBriefingWorkerStatus(worker, liveAgent, false);
    const inNeedsYou = needsYouKeys.has(key);
    if (status === 'running' || (status === 'awaiting_permission' && !inNeedsYou)) {
      runningOtherWorkers.push({
        key: key,
        worker: worker,
        status: status,
        indentLevel: 0,
        depth: 0,
        parentHostId: null,
        archivable: false,
        fromOtherActive: true
      });
    }
  }

  const archivableCompletedWorkers: PartitionedBriefingWorker[] = [];
  for (let i = 0; i < completedWorkers.length; i += 1) {
    if (completedWorkers[i].archivable) {
      archivableCompletedWorkers.push(completedWorkers[i]);
    }
  }

  return {
    runningMissionWorkers: runningMissionWorkers,
    runningOtherWorkers: runningOtherWorkers,
    completedWorkers: completedWorkers,
    archivableCompletedWorkers: archivableCompletedWorkers,
    inProgressCount: runningMissionWorkers.length + runningOtherWorkers.length
  };
}

/**
 * Re-validates candidate archivable workers right before issuing `archiveAgent` RPCs so that
 * no session with a running turn, pending permission, background task, armed timer, or active
 * provider-native subagent is ever archived.
 */
export function selectConfirmedArchivableWorkers(
  candidates: readonly PartitionedBriefingWorker[],
  latestBriefing: PersonalAgentBriefingDto | null,
  latestHostDetails: Map<string, HostDetailDto>
): PartitionedBriefingWorker[] {
  if (candidates.length === 0 || latestBriefing === null) {
    return [];
  }
  const latestPartition = partitionBriefingWorkers(
    latestBriefing.missions,
    latestBriefing.other_active,
    latestBriefing.needs_you,
    latestHostDetails,
    latestBriefing.waiting ?? []
  );
  const stillArchivableByKey = new Map<string, PartitionedBriefingWorker>();
  for (let i = 0; i < latestPartition.archivableCompletedWorkers.length; i += 1) {
    const item = latestPartition.archivableCompletedWorkers[i];
    stillArchivableByKey.set(item.key, item);
  }
  const confirmed: PartitionedBriefingWorker[] = [];
  const seen = new Set<string>();
  for (let i = 0; i < candidates.length; i += 1) {
    const key = candidates[i].key;
    if (seen.has(key)) {
      continue;
    }
    const verified = stillArchivableByKey.get(key);
    if (verified !== undefined) {
      seen.add(key);
      confirmed.push(verified);
    }
  }
  return confirmed;
}

/**
 * Paginates combined Mission + Other-active in-progress workers under a single shared budget
 * so the "Show N more" button count always matches the exact number of hidden rows.
 */
export function paginateInProgressWorkers(
  missionWorkers: readonly PartitionedBriefingWorker[],
  otherWorkers: readonly PartitionedBriefingWorker[],
  showAll: boolean,
  limit: number = 5
): InProgressPaginationResult {
  const total = missionWorkers.length + otherWorkers.length;
  if (showAll || total <= limit) {
    return {
      visibleMissionWorkers: missionWorkers.slice(),
      visibleOtherWorkers: otherWorkers.slice(),
      hiddenCount: 0,
      showToggle: total > limit
    };
  }
  const visibleMission = missionWorkers.slice(0, limit);
  const remaining = Math.max(0, limit - visibleMission.length);
  const visibleOther = otherWorkers.slice(0, remaining);
  const hiddenCount = Math.max(0, total - (visibleMission.length + visibleOther.length));
  return {
    visibleMissionWorkers: visibleMission,
    visibleOtherWorkers: visibleOther,
    hiddenCount: hiddenCount,
    showToggle: hiddenCount > 0
  };
}

export function filterActivityForPersonalAgent(
  entries: PersonalAgentActivityEntryDto[],
  briefing: PersonalAgentBriefingDto | null,
  pa: PersonalAgentIdentityRef | null
): PersonalAgentActivityEntryDto[] {
  if (entries.length === 0 || pa === null) {
    return [];
  }
  const paScopedKeys = new Set<string>([briefingWorkerKey(pa.hostId, pa.agentId)]);
  const paAgentIds = new Set<string>([pa.agentId]);

  if (briefing !== null) {
    for (let i = 0; i < briefing.missions.length; i += 1) {
      const m = briefing.missions[i];
      paScopedKeys.add(briefingWorkerKey(m.root.host_id, m.root.agent_id));
      paAgentIds.add(m.root.agent_id);
      for (let j = 0; j < m.workers.length; j += 1) {
        const w = m.workers[j];
        paScopedKeys.add(briefingWorkerKey(w.host_id, w.agent_id));
        paAgentIds.add(w.agent_id);
      }
    }
    for (let i = 0; i < briefing.needs_you.length; i += 1) {
      const d = briefing.needs_you[i];
      if (d.dispatched_by_personal_agent) {
        paScopedKeys.add(briefingWorkerKey(d.host_id, d.agent_id));
        paAgentIds.add(d.agent_id);
      }
    }
  }

  const out: PersonalAgentActivityEntryDto[] = [];
  for (let i = 0; i < entries.length; i += 1) {
    const entry = entries[i];
    if (entry.actor === 'personal_agent') {
      out.push(entry);
      continue;
    }
    const agId = entry.agent_id ?? '';
    if (agId.length === 0) {
      continue;
    }
    const hId = entry.host_id ?? '';
    if (hId.length > 0) {
      if (paScopedKeys.has(briefingWorkerKey(hId, agId))) {
        out.push(entry);
      }
    } else if (paAgentIds.has(agId)) {
      out.push(entry);
    }
  }
  return out;
}

export function computeSinceLastVisitSummary(
  briefing: PersonalAgentBriefingDto | null,
  filteredActivity: PersonalAgentActivityEntryDto[],
  previousVisitMs: number
): SinceLastVisitSummary {
  const completed = briefing !== null ? briefing.since_last_visit.completed : 0;
  const coreAutoHandled = briefing !== null ? briefing.since_last_visit.auto_handled : 0;
  let activityAutoHandled = 0;
  if (previousVisitMs > 0) {
    for (let i = 0; i < filteredActivity.length; i += 1) {
      if (filteredActivity[i].timestamp_ms > previousVisitMs) {
        activityAutoHandled += 1;
      }
    }
  }
  const autoHandled = Math.max(coreAutoHandled, activityAutoHandled);
  return {
    completed: completed,
    autoHandled: autoHandled,
    hasSummary: completed > 0 || autoHandled > 0
  };
}

export function selectRecommendedRouterHost(
  hosts: RouterHostCapabilitiesDto[]
): RouterHostCapabilitiesDto | null {
  if (hosts.length === 0) {
    return null;
  }
  const sorted = hosts.slice().sort((a: RouterHostCapabilitiesDto, b: RouterHostCapabilitiesDto): number => {
    if (a.online !== b.online) {
      return a.online ? -1 : 1;
    }
    if (b.recommended_score !== a.recommended_score) {
      return b.recommended_score - a.recommended_score;
    }
    if (a.is_local !== b.is_local) {
      return a.is_local ? 1 : -1;
    }
    return a.host_name.localeCompare(b.host_name);
  });
  return sorted[0];
}

function normalizeOptionalString(val: string | null | undefined): string | null {
  if (typeof val !== 'string') {
    return null;
  }
  const trimmed = val.trim();
  return trimmed.length > 0 ? trimmed : null;
}

/**
 * Normalizes a `PersonalRouterConfigDto` deserialized from Rust JSON where `Option::None` fields
 * are omitted (`undefined` instead of `null`), preventing `undefined.trim()` runtime failures.
 */
export function normalizePersonalRouterConfig(
  raw: PersonalRouterConfigDto
): PersonalRouterConfigDto {
  const provider = typeof raw.provider === 'string' && raw.provider.trim().length > 0
    ? raw.provider.trim()
    : 'claude_code';
  const scope = typeof raw.scope === 'string' && raw.scope.trim().length > 0
    ? raw.scope.trim()
    : 'global_fleet';
  const preReviewMode = typeof raw.pre_review_mode === 'string' && raw.pre_review_mode.trim().length > 0
    ? raw.pre_review_mode.trim()
    : 'medium_risk_only';
  return {
    enabled: raw.enabled === true,
    router_agent_id: normalizeOptionalString(raw.router_agent_id),
    provider: provider,
    acp_agent_id: normalizeOptionalString(raw.acp_agent_id),
    model: normalizeOptionalString(raw.model),
    effort: normalizeOptionalString(raw.effort),
    cwd: normalizeOptionalString(raw.cwd),
    scope: scope,
    pre_review_mode: preReviewMode,
    share_host_hermes_memory: raw.share_host_hermes_memory !== false,
    soul_prompt_override: normalizeOptionalString(raw.soul_prompt_override),
    external_webhook_url: normalizeOptionalString(raw.external_webhook_url),
    has_external_webhook_secret: raw.has_external_webhook_secret === true
  };
}

/**
 * Merges user-selected provider/model and `enabled: true` onto a target host's verified existing
 * `PersonalRouterConfigDto`, preserving all security, scope, pre-review, memory, and webhook fields.
 */
export function mergePersonalAgentSetupConfig(
  provider: string,
  model: string,
  existing: PersonalRouterConfigDto
): PersonalRouterConfigDto {
  const base = normalizePersonalRouterConfig(existing);
  const cleanProvider = typeof provider === 'string' && provider.trim().length > 0
    ? provider.trim()
    : base.provider;
  const cleanModel = typeof model === 'string' && model.trim().length > 0
    ? model.trim()
    : null;
  return {
    enabled: true,
    router_agent_id: base.router_agent_id,
    provider: cleanProvider,
    acp_agent_id: cleanProvider === 'acp' ? base.acp_agent_id : null,
    model: cleanModel,
    effort: base.effort,
    cwd: base.cwd,
    scope: base.scope,
    pre_review_mode: base.pre_review_mode,
    share_host_hermes_memory: base.share_host_hermes_memory,
    soul_prompt_override: base.soul_prompt_override,
    external_webhook_url: base.external_webhook_url,
    has_external_webhook_secret: base.has_external_webhook_secret
  };
}

export function resolveRelativeTimeSpec(tsMs: number, nowMs: number): CompactTimeSpec {
  if (tsMs <= 0 || !Number.isFinite(tsMs)) {
    return { kind: 'empty', primary: 0, secondary: 0 };
  }
  const deltaSecs = Math.max(0, Math.floor((nowMs - tsMs) / 1000));
  if (deltaSecs < 60) {
    return { kind: 'just_now', primary: 0, secondary: 0 };
  }
  if (deltaSecs < 3600) {
    return { kind: 'min', primary: Math.floor(deltaSecs / 60), secondary: 0 };
  }
  if (deltaSecs < 86400) {
    return { kind: 'hr', primary: Math.floor(deltaSecs / 3600), secondary: 0 };
  }
  if (deltaSecs < 604800) {
    return { kind: 'day', primary: Math.floor(deltaSecs / 86400), secondary: 0 };
  }
  return { kind: 'week', primary: Math.floor(deltaSecs / 604800), secondary: 0 };
}

export function resolveElapsedTimeSpec(tsMs: number, nowMs: number): CompactTimeSpec {
  if (tsMs <= 0 || !Number.isFinite(tsMs)) {
    return { kind: 'empty', primary: 0, secondary: 0 };
  }
  const sec = Math.max(0, Math.floor((nowMs - tsMs) / 1000));
  if (sec < 60) {
    return { kind: 'sec', primary: sec, secondary: 0 };
  }
  const min = Math.floor(sec / 60);
  if (min < 60) {
    return { kind: 'min', primary: min, secondary: 0 };
  }
  const hr = Math.floor(min / 60);
  if (hr < 24) {
    return { kind: 'hr', primary: hr, secondary: 0 };
  }
  return { kind: 'day', primary: Math.floor(hr / 24), secondary: 0 };
}

export function resolveCountdownTimeSpec(dueAtMs: number, nowMs: number): CompactTimeSpec {
  if (!Number.isFinite(dueAtMs) || dueAtMs <= 0) {
    return { kind: 'empty', primary: 0, secondary: 0 };
  }
  const diffSec = Math.max(0, Math.floor((dueAtMs - nowMs) / 1000));
  if (diffSec < 60) {
    return { kind: 'sec', primary: diffSec, secondary: 0 };
  }
  const min = Math.floor(diffSec / 60);
  const remSec = diffSec % 60;
  if (min < 60) {
    return remSec > 0
      ? { kind: 'min_sec', primary: min, secondary: remSec }
      : { kind: 'min', primary: min, secondary: 0 };
  }
  const hr = Math.floor(min / 60);
  const remMin = min % 60;
  return remMin > 0
    ? { kind: 'hr_min', primary: hr, secondary: remMin }
    : { kind: 'hr', primary: hr, secondary: 0 };
}

function interpolateCompactTemplate(template: string, a: number, b: number = 0): string {
  let out = template.replace(/%1\$d/g, String(a)).replace(/%2\$d/g, String(b));
  let seqIdx = 0;
  const vals = [a, b];
  out = out.replace(/%d/g, (): string => {
    const v = seqIdx < vals.length ? String(vals[seqIdx]) : '';
    seqIdx += 1;
    return v;
  });
  return out;
}

export function formatCompactTimeWithTemplates(
  spec: CompactTimeSpec,
  templates: CompactTimeTemplates = EN_COMPACT_TIME_TEMPLATES
): string {
  if (spec.kind === 'empty') {
    return '';
  }
  if (spec.kind === 'just_now') {
    return templates.justNow;
  }
  if (spec.kind === 'sec') {
    return interpolateCompactTemplate(templates.secondsShort, spec.primary);
  }
  if (spec.kind === 'min') {
    return interpolateCompactTemplate(templates.minutesShort, spec.primary);
  }
  if (spec.kind === 'hr') {
    return interpolateCompactTemplate(templates.hoursShort, spec.primary);
  }
  if (spec.kind === 'day') {
    return interpolateCompactTemplate(templates.daysShort, spec.primary);
  }
  if (spec.kind === 'week') {
    return interpolateCompactTemplate(templates.weeksShort, spec.primary);
  }
  if (spec.kind === 'min_sec') {
    return interpolateCompactTemplate(templates.minutesSecondsShort, spec.primary, spec.secondary);
  }
  return interpolateCompactTemplate(templates.hoursMinutesShort, spec.primary, spec.secondary);
}

export function compactTimeSpecToResource(spec: CompactTimeSpec): ResourceStr {
  if (spec.kind === 'empty') {
    return '';
  }
  if (spec.kind === 'just_now') {
    return $r('app.string.time_justNow');
  }
  if (spec.kind === 'sec') {
    return $r('app.string.time_secondsShort', spec.primary);
  }
  if (spec.kind === 'min') {
    return $r('app.string.time_minutesShort', spec.primary);
  }
  if (spec.kind === 'hr') {
    return $r('app.string.time_hoursShort', spec.primary);
  }
  if (spec.kind === 'day') {
    return $r('app.string.time_daysShort', spec.primary);
  }
  if (spec.kind === 'week') {
    return $r('app.string.time_weeksShort', spec.primary);
  }
  if (spec.kind === 'min_sec') {
    return $r('app.string.time_minutesSecondsShort', spec.primary, spec.secondary);
  }
  return $r('app.string.time_hoursMinutesShort', spec.primary, spec.secondary);
}

export function formatBriefingElapsedResource(tsMs: number, nowMs: number): ResourceStr {
  return compactTimeSpecToResource(resolveElapsedTimeSpec(tsMs, nowMs));
}

export function formatWaitCountdownResource(dueAtMs: number, nowMs: number): ResourceStr {
  return compactTimeSpecToResource(resolveCountdownTimeSpec(dueAtMs, nowMs));
}

export function formatBriefingElapsed(tsMs: number, nowMs: number): string {
  return formatCompactTimeWithTemplates(resolveElapsedTimeSpec(tsMs, nowMs), EN_COMPACT_TIME_TEMPLATES);
}

export function formatWaitCountdown(dueAtMs: number, nowMs: number): string {
  return formatCompactTimeWithTemplates(resolveCountdownTimeSpec(dueAtMs, nowMs), EN_COMPACT_TIME_TEMPLATES);
}
