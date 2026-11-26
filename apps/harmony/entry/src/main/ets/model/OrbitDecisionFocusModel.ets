import { OrbitTimelineItemModel } from './viewmodels/OrbitStageModels';

export interface QueuedDecisionCard {
  key: string;
  kind: string; // 'permission' | 'question'
  seq: number;
  item: OrbitTimelineItemModel;
}

export interface DecisionFocusTarget {
  readonly hostId: string;
  readonly agentId: string;
  readonly eventKind: string;
  readonly requestId: string;
  readonly seq: number;
}

/**
 * Constructs a validated approval-card focus target strictly when ALL required identity pieces
 * are present (`eventKind === 'permission_required'`, non-empty `hostId`, non-empty `agentId`,
 * and either `requestId` or `seq > 0`). Host-only links or non-permission events return `null`
 * so they never write an unscoped focus target that could hijack an unrelated session.
 */
export function buildDecisionFocusTarget(
  eventKind: string,
  hostId: string,
  agentId: string,
  requestId: string,
  seq: number
): DecisionFocusTarget | null {
  const cleanEvent = eventKind.trim();
  const cleanHost = hostId.trim();
  const cleanAgent = agentId.trim();
  const cleanReq = requestId.trim();
  const cleanSeq = Number.isFinite(seq) && seq > 0 ? Math.floor(seq) : 0;
  if (
    cleanEvent !== 'permission_required' ||
    cleanHost.length === 0 ||
    cleanAgent.length === 0 ||
    (cleanReq.length === 0 && cleanSeq <= 0)
  ) {
    return null;
  }
  return {
    hostId: cleanHost,
    agentId: cleanAgent,
    eventKind: cleanEvent,
    requestId: cleanReq,
    seq: cleanReq.length > 0 ? 0 : cleanSeq
  };
}

/**
 * Resolves the index of the specific decision card targeted by a permission deep-link notification:
 * - Strictly requires BOTH non-empty `targetHostId` AND non-empty `targetAgentId` matching the
 *   active `(activeHostId, activeAgentId)` session whenever session context is present.
 * - When `targetRequestId` is non-empty, ONLY an exact `request_id` match is accepted (never
 *   falling back to `targetSeq`, so an expired request A can never hijack an unrelated request B
 *   that happens to share the same `seq`).
 * - Only when `targetRequestId` is empty and `targetSeq > 0` does it match an approval item by `seq`.
 */
export function findFocusedDecisionQueueIndex(
  queue: readonly QueuedDecisionCard[],
  targetRequestId: string,
  targetSeq: number,
  targetHostId: string = '',
  targetAgentId: string = '',
  activeHostId: string = '',
  activeAgentId: string = ''
): number {
  const cleanTargetHost = targetHostId.trim();
  const cleanTargetAgent = targetAgentId.trim();
  const cleanActiveHost = activeHostId.trim();
  const cleanActiveAgent = activeAgentId.trim();

  // Require complete (host, agent) identity whenever any session scope parameter is supplied.
  if (
    cleanTargetHost.length > 0 ||
    cleanTargetAgent.length > 0 ||
    cleanActiveHost.length > 0 ||
    cleanActiveAgent.length > 0
  ) {
    if (
      cleanTargetHost.length === 0 ||
      cleanTargetAgent.length === 0 ||
      cleanTargetHost !== cleanActiveHost ||
      cleanTargetAgent !== cleanActiveAgent
    ) {
      return -1;
    }
  }

  const cleanReq = targetRequestId.trim();
  if (cleanReq.length > 0) {
    for (let i = 0; i < queue.length; i += 1) {
      if (queue[i].item.requestId === cleanReq) {
        return i;
      }
    }
    // Explicit requestId did not match any active card; never hijack another card by seq.
    return -1;
  }
  if (targetSeq > 0) {
    for (let i = 0; i < queue.length; i += 1) {
      if (
        queue[i].kind === 'permission' &&
        (queue[i].seq === targetSeq || queue[i].item.seq === targetSeq)
      ) {
        return i;
      }
    }
  }
  return -1;
}
