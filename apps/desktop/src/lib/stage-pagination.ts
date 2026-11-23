import type {
  AgentStageDeltaView,
  AgentStageView,
  StageTurnView,
} from "./orbit-ipc.ts";

export const INITIAL_STAGE_TURN_WINDOW = 24;
export const OLDER_STAGE_TURN_PAGE_SIZE = 24;

export const LOG_INITIAL_LINE_CAP = 800;
export const LOG_PAGE_INCREMENT = 800;
export const LOG_TOP_PAGINATE_THRESHOLD_PX = 260;

export type WindowedAgentStageView = AgentStageView & {
  window_start_turn_index?: number;
  total_turn_count?: number;
  has_older_turns?: boolean;
  has_older_history?: boolean;
  oldest_loaded_seq?: number;
};

export type StageDeltaWithWindow = AgentStageDeltaView & {
  window_start_turn_index?: number;
  total_turn_count?: number;
  has_older_turns?: boolean;
  has_older_history?: boolean;
  oldest_loaded_seq?: number;
};

export interface StageTurnPaginationIpc {
  getAgentStageDelta: (
    hostId: string,
    agentId: string,
    sinceRevision: number,
    sinceAuxRevision?: number | null,
    tailTurnLimit?: number | null,
  ) => Promise<StageDeltaWithWindow>;
  getAgentStageTurnsSlice?: (
    hostId: string,
    agentId: string,
    startTurnIndex: number,
    count: number,
  ) => Promise<StageTurnView[]>;
}

export interface MergedStageTurnsWindow {
  turns: StageTurnView[];
  window_start_turn_index: number;
  total_turn_count: number;
}

/**
 * Merges an incoming stage delta into an existing tail-windowed stage turns buffer.
 *
 * `delta.replace_from_turn_index` is always an ABSOLUTE turn index in Rust's full `turns` vector.
 * When `prev` holds a tail window starting at `prevWindowStart > 0`:
 * - If `replace_from_turn_index >= prevWindowStart`, splice at `localReplaceFrom = replace_from_turn_index - prevWindowStart`
 *   and preserve `prevWindowStart`.
 * - If `replace_from_turn_index < prevWindowStart`, Rust emitted turns starting earlier than our window,
 *   so replace `turns` with `delta.changed_turns` and move `window_start_turn_index` to `replace_from_turn_index`.
 */
export function mergeStageDeltaWithWindow(
  prev: Pick<WindowedAgentStageView, "turns" | "window_start_turn_index" | "total_turn_count"> | undefined,
  delta: Pick<
    StageDeltaWithWindow,
    "reset" | "replace_from_turn_index" | "changed_turns" | "window_start_turn_index" | "total_turn_count"
  >,
  sinceRevision: number,
): MergedStageTurnsWindow {
  const resetTurns = delta.reset || sinceRevision === 0 || !prev;
  if (resetTurns) {
    const windowStart = Math.max(0, Math.floor(delta.window_start_turn_index ?? 0));
    const turns = delta.changed_turns;
    const totalCount =
      typeof delta.total_turn_count === "number"
        ? Math.max(delta.total_turn_count, windowStart + turns.length)
        : windowStart + turns.length;
    return {
      turns,
      window_start_turn_index: windowStart,
      total_turn_count: totalCount,
    };
  }

  const prevWindowStart = Math.max(0, Math.floor(prev.window_start_turn_index ?? 0));
  const absReplaceFrom = Math.max(0, Math.floor(delta.replace_from_turn_index));

  if (absReplaceFrom >= prevWindowStart) {
    const localReplaceFrom = absReplaceFrom - prevWindowStart;
    const turns =
      delta.changed_turns.length === 0 && localReplaceFrom >= prev.turns.length
        ? prev.turns
        : [...prev.turns.slice(0, localReplaceFrom), ...delta.changed_turns];
    const totalCount =
      typeof delta.total_turn_count === "number"
        ? delta.total_turn_count
        : prevWindowStart + turns.length;
    return {
      turns,
      window_start_turn_index: prevWindowStart,
      total_turn_count: totalCount,
    };
  }

  const turns = delta.changed_turns;
  const totalCount =
    typeof delta.total_turn_count === "number"
      ? delta.total_turn_count
      : absReplaceFrom + turns.length;
  return {
    turns,
    window_start_turn_index: absReplaceFrom,
    total_turn_count: totalCount,
  };
}

export interface EarlierTurnSliceRequest {
  startTurnIndex: number;
  count: number;
}

/**
 * Computes the next `(startTurnIndex, count)` slice to fetch immediately preceding `currentWindowStart`.
 * When `targetTurnIndex` is supplied (e.g. jump-to-seq targeting an older turn), widens the slice
 * down to `targetTurnIndex` so a single IPC round-trip reaches the jump destination.
 */
export function computeEarlierTurnSliceRequest(
  currentWindowStart: number,
  pageSize: number = OLDER_STAGE_TURN_PAGE_SIZE,
  targetTurnIndex?: number,
): EarlierTurnSliceRequest | null {
  const curStart = Math.max(0, Math.floor(currentWindowStart));
  if (curStart <= 0) return null;

  const safePageSize = Math.max(1, Math.floor(pageSize));
  const defaultStart = Math.max(0, curStart - safePageSize);
  const nextStart =
    targetTurnIndex !== undefined && Number.isFinite(targetTurnIndex)
      ? Math.max(0, Math.min(defaultStart, Math.floor(targetTurnIndex)))
      : defaultStart;
  const count = curStart - nextStart;
  if (count <= 0) return null;

  return { startTurnIndex: nextStart, count };
}

export interface PrependedStageTurnsResult {
  applied: boolean;
  prependedCount: number;
  turns: StageTurnView[];
  window_start_turn_index: number;
  total_turn_count: number;
}

/**
 * Safely prepends a fetched slice of historical turns onto `prev` while guarding against
 * concurrent stage resets and duplicate turn keys at the splice boundary.
 */
export function mergePrependedStageTurns(
  prev: Pick<WindowedAgentStageView, "turns" | "window_start_turn_index" | "total_turn_count"> | undefined,
  olderTurns: readonly StageTurnView[],
  requestedStartTurnIndex: number,
  expectedWindowStart: number,
  expectedFirstTurnKey: string | null,
): PrependedStageTurnsResult {
  if (!prev) {
    return {
      applied: false,
      prependedCount: 0,
      turns: [],
      window_start_turn_index: 0,
      total_turn_count: 0,
    };
  }

  const prevStart = Math.max(0, Math.floor(prev.window_start_turn_index ?? 0));
  const curFirstKey = prev.turns[0]?.key ?? null;
  if (
    prevStart !== expectedWindowStart ||
    (expectedFirstTurnKey !== null && curFirstKey !== expectedFirstTurnKey)
  ) {
    return {
      applied: false,
      prependedCount: 0,
      turns: prev.turns,
      window_start_turn_index: prevStart,
      total_turn_count: prev.total_turn_count ?? prevStart + prev.turns.length,
    };
  }

  if (olderTurns.length === 0) {
    return {
      applied: false,
      prependedCount: 0,
      turns: prev.turns,
      window_start_turn_index: prevStart,
      total_turn_count: prev.total_turn_count ?? prevStart + prev.turns.length,
    };
  }

  const existingKeys = new Set(prev.turns.map((t) => t.key));
  const dedupedOlder = olderTurns.filter((t) => !existingKeys.has(t.key));
  const requestedCount = Math.max(0, prevStart - Math.max(0, Math.floor(requestedStartTurnIndex)));
  const safeReqStart = Math.max(0, Math.floor(requestedStartTurnIndex));
  const nextWindowStart =
    olderTurns.length >= requestedCount || safeReqStart === 0
      ? safeReqStart
      : Math.max(0, prevStart - dedupedOlder.length);

  if (dedupedOlder.length === 0) {
    return {
      applied: nextWindowStart !== prevStart,
      prependedCount: 0,
      turns: prev.turns,
      window_start_turn_index: nextWindowStart,
      total_turn_count: Math.max(prev.total_turn_count ?? 0, nextWindowStart + prev.turns.length),
    };
  }

  const mergedTurns = [...dedupedOlder, ...prev.turns];
  const nextTotal = Math.max(prev.total_turn_count ?? 0, nextWindowStart + mergedTurns.length);

  return {
    applied: true,
    prependedCount: dedupedOlder.length,
    turns: mergedTurns,
    window_start_turn_index: nextWindowStart,
    total_turn_count: nextTotal,
  };
}

export interface LogWindowSlice {
  effectiveCap: number;
  startIdx: number;
}

/**
 * Computes the active tail window for long log rendering, automatically expanding
 * when a find-in-file match or focus line lies earlier than `renderedLineCap`.
 */
export function computeLogWindowSlice(
  totalLines: number,
  renderedLineCap: number,
  activeTargetLine: number | null | undefined,
): LogWindowSlice {
  const safeTotal = Math.max(0, Math.floor(totalLines));
  let effectiveCap = Math.max(0, Math.floor(renderedLineCap));
  if (
    activeTargetLine != null &&
    Number.isFinite(activeTargetLine) &&
    safeTotal - activeTargetLine + 20 > effectiveCap
  ) {
    effectiveCap = safeTotal - Math.floor(activeTargetLine) + 40;
  }
  const startIdx = Math.max(0, safeTotal - effectiveCap);
  return { effectiveCap, startIdx };
}

export function computeNextLogLineCap(
  totalLines: number,
  currentCap: number,
  increment: number = LOG_PAGE_INCREMENT,
): number {
  const safeTotal = Math.max(0, Math.floor(totalLines));
  const safeCap = Math.max(0, Math.floor(currentCap));
  const safeInc = Math.max(1, Math.floor(increment));
  return Math.min(safeTotal, safeCap + safeInc);
}

export function shouldTriggerTopLogPagination(
  logStartIdx: number,
  scrollTop: number,
  isAdjusting: boolean,
  thresholdPx: number = LOG_TOP_PAGINATE_THRESHOLD_PX,
): boolean {
  return logStartIdx > 0 && !isAdjusting && scrollTop < thresholdPx;
}

/**
 * Computes the restored `scrollTop` after prepending content above the viewport so
 * the reader's visible anchor line stays visually stationary (zero jump).
 */
export function computePrependRestoredScrollTop(
  prevScrollTop: number,
  prevScrollHeight: number,
  nextScrollHeight: number,
): number {
  const deltaHeight = Math.max(0, nextScrollHeight - prevScrollHeight);
  return Math.max(0, prevScrollTop + deltaHeight);
}
