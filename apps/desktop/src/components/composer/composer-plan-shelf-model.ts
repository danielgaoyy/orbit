import type { SessionPlanStepView, StageTurnView } from "../../lib/orbit-ipc.ts";

export type PlanRowState = "pending" | "running" | "paused" | "completed" | "skipped" | "failed";

/** Brief completion dwell before a finished todo list auto-dismisses cleanly. */
export const PLAN_AUTO_DISMISS_MS = 2800;

export const traceStatus = {
  pending: "pending",
  paused: "pending",
  running: "running",
  failed: "error",
  completed: undefined,
  skipped: undefined,
} as const;

export function presentPlanStep(
  step: SessionPlanStepView,
  isForegroundRunning: boolean,
  isFromPreviousTurn = false,
) {
  let state: PlanRowState;
  if (step.status === "in_progress") {
    if (isForegroundRunning && !isFromPreviousTurn) {
      state = "running";
    } else if (isForegroundRunning && isFromPreviousTurn) {
      // Carried-over step during an active continuation turn stays neutral (neither spinning nor "(paused)").
      state = "pending";
    } else {
      state = "paused";
    }
  } else {
    state = step.status;
  }
  return { id: step.id, state, title: (state === "running" && step.active_form) || step.title };
}

// A new prompt alone doesn't change the plan. Resolve the turn of the plan event,
// even when another turn has already started or earlier history is backfilled.
export function planSourceTurn(
  turns: readonly Pick<StageTurnView, "anchor_seq" | "live_segments">[],
  seq?: number,
) {
  if (seq === undefined) return null;
  // Optimistic uncommitted turns appended on send carry `anchor_seq === 0` and are
  // ordered strictly after committed `seq > 0` turns in `orbit-client-core`.
  // Strip trailing optimistic turns before binary-searching committed anchor_seq values.
  let high = turns.length;
  while (high > 0 && turns[high - 1].anchor_seq === 0) {
    high -= 1;
  }
  if (seq === 0) {
    return high < turns.length ? 0 : null;
  }
  let low = 0;
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if (turns[mid].anchor_seq <= seq) low = mid + 1;
    else high = mid;
  }
  const turn = turns[low - 1];
  if (!turn) return null;
  // Even a one-row TaskUpdate can finish last turn's plan. Only replacement
  // events establish a new source turn; row contents always come from insights.
  const incremental = turn.live_segments.some(
    (seg) =>
      seg.kind === "step_group" &&
      seg.first_seq <= seq &&
      seg.items.some(
        (item) =>
          item.seq === seq &&
          item.item_type === "tool_call" &&
          item.detail.kind === "generic" &&
          /(?:^|[/:\s]|__)task_?update$/i.test(item.detail.tool_name.trim()),
      ),
  );
  return incremental ? null : turn.anchor_seq;
}

export function describePlan(
  steps: readonly SessionPlanStepView[],
  isForegroundRunning: boolean,
  latestTurnAnchorSeq: number | null = null,
) {
  if (!steps.length) return null;
  let latestSeq = steps[0].updated_seq;
  for (let i = 1; i < steps.length; i += 1) {
    if (steps[i].updated_seq > latestSeq) latestSeq = steps[i].updated_seq;
  }
  // A plan whose last update occurred strictly before the latest turn started
  // belongs to a previous turn; never spin a live spinner for an abandoned step.
  // Optimistic uncommitted UserPrompts carry `anchor_seq === 0`, which represents
  // a brand-new turn strictly newer than any committed `latestSeq > 0` plan event.
  const isFromPreviousTurn =
    latestTurnAnchorSeq !== null &&
    (latestTurnAnchorSeq === 0 ? latestSeq > 0 : latestSeq < latestTurnAnchorSeq);
  const effectiveRunning = isForegroundRunning && !isFromPreviousTurn;
  const rows = steps.map((step) => presentPlanStep(step, isForegroundRunning, isFromPreviousTurn));
  const skipped = rows.filter((row) => row.state === "skipped").length;
  const done = skipped + rows.filter((row) => row.state === "completed").length;
  const failed = rows.find((row) => row.state === "failed");
  const inProgressCarryIdx = steps.findIndex((step) => step.status === "in_progress");
  const headline =
    rows.find((row) => row.state === "running") ??
    failed ??
    rows.find((row) => row.state === "paused") ??
    (inProgressCarryIdx >= 0 ? rows[inProgressCarryIdx] : undefined) ??
    rows.find((row) => row.state === "pending");
  const allSettled = done === rows.length;
  return {
    rows,
    skipped,
    done,
    allSettled,
    headline,
    hasFailure: !!failed,
    isFromPreviousTurn,
    canDismiss: !effectiveRunning,
    shouldAutoDismiss: allSettled && !effectiveRunning,
    signature: JSON.stringify(steps.map(({ id, title }) => [id, title])),
    version: JSON.stringify(
      steps.map(({ id, title, status, active_form }) => [id, title, status, active_form]),
    ),
    firstSeq: steps[0].updated_seq,
    latestSeq,
    wholeSnapshot: steps.every((step) => step.updated_seq === steps[0].updated_seq),
  };
}

export interface PlanShelfPreference {
  signature: string;
  version: string;
  firstSeq: number;
  latestSeq: number;
  sourceTurn: number | null;
  open: boolean;
  autoCollapsed: boolean;
  dismissed: boolean;
  /** True only when the user watched this plan settle live during this mount/session. */
  settledLive: boolean;
}

export function reconcilePlanShelf(
  previous: PlanShelfPreference | null,
  plan: ReturnType<typeof describePlan>,
  sourceTurn: number | null,
): PlanShelfPreference | null {
  if (!plan) return null;
  // Snapshot providers may reuse explicit IDs across plans. Treat either a different
  // title/id signature, a new turn's whole snapshot, or a reopened plan whose latestSeq
  // advanced after settling as a brand-new plan.
  const replaced =
    !previous ||
    previous.signature !== plan.signature ||
    (plan.wholeSnapshot &&
      plan.firstSeq !== previous.firstSeq &&
      ((sourceTurn !== null && sourceTurn !== previous.sourceTurn) ||
        (previous.dismissed && plan.latestSeq > previous.latestSeq)));
  if (replaced) {
    // Opening a historical/reloaded session whose plan is ALREADY 100% settled
    // (or left settled in an earlier turn) should not haunt the composer.
    const startHidden =
      plan.shouldAutoDismiss && (!previous || plan.isFromPreviousTurn);
    return {
      signature: plan.signature,
      version: plan.version,
      firstSeq: plan.firstSeq,
      latestSeq: plan.latestSeq,
      sourceTurn,
      open: !plan.allSettled,
      autoCollapsed: plan.shouldAutoDismiss,
      dismissed: startHidden,
      settledLive: plan.shouldAutoDismiss && !startHidden,
    };
  }
  let open = previous.open;
  if (plan.shouldAutoDismiss && !previous.autoCollapsed) open = false;
  if (!plan.allSettled && previous.autoCollapsed) open = true;
  const autoCollapsed =
    plan.shouldAutoDismiss || (plan.allSettled && previous.autoCollapsed);
  // If a newer turn started without updating a previously settled plan, retire it immediately.
  const retiredByNextTurn = plan.allSettled && plan.isFromPreviousTurn;
  // A settled plan that was already dismissed (auto-dismissed, manually closed, or hidden on
  // fresh mount) must never resurrect if `isForegroundRunning` flips true before the next
  // turn's timeline arrives (e.g. `HostChanged` preceding `TimelineChanged` on prompt submit).
  const keepDismissed =
    previous.dismissed &&
    previous.version === plan.version &&
    previous.latestSeq === plan.latestSeq &&
    (plan.canDismiss || plan.allSettled);
  const dismissed = retiredByNextTurn || keepDismissed;
  // Keep a collapsed, visible settled plan eligible for timed auto-dismiss even across
  // transient `isForegroundRunning` blips; manual expand (`open: true`) pauses auto-dismiss.
  const justSettledLive =
    plan.shouldAutoDismiss && (!previous.autoCollapsed || previous.settledLive || !open);
  const settledLive = !dismissed && justSettledLive;
  const knownTurn = sourceTurn ?? previous.sourceTurn;

  if (
    previous.version === plan.version &&
    previous.firstSeq === plan.firstSeq &&
    previous.latestSeq === plan.latestSeq &&
    previous.sourceTurn === knownTurn &&
    previous.open === open &&
    previous.autoCollapsed === autoCollapsed &&
    previous.dismissed === dismissed &&
    previous.settledLive === settledLive
  )
    return previous;
  return {
    signature: plan.signature,
    version: plan.version,
    firstSeq: plan.firstSeq,
    latestSeq: plan.latestSeq,
    sourceTurn: knownTurn,
    open,
    autoCollapsed,
    dismissed,
    settledLive,
  };
}

export function createPlanShelfCache(limit = 128) {
  const sessions = new Map<string, PlanShelfPreference>();
  return {
    get(scope: string) {
      return sessions.get(scope) ?? null;
    },
    save(scope: string, preference: PlanShelfPreference | null) {
      sessions.delete(scope);
      if (preference) sessions.set(scope, preference);
      if (sessions.size > limit) {
        const oldest = sessions.keys().next();
        if (!oldest.done) sessions.delete(oldest.value);
      }
    },
  };
}
