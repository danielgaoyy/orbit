import type { GoalRunRecord } from "./orbit-ipc";

export const GOAL_SUCCEEDED_AUTO_DISMISS_MS = 15_000;
export const GOAL_SUCCEEDED_RECENT_WINDOW_MS = 120_000;

/** Formats elapsed seconds in concise human units (`45s`, `4m 12s`, `2h 19m`, `1d 3h`). */
export function formatGoalDuration(totalSeconds: number): string {
  const secs = Math.max(0, Math.floor(totalSeconds));
  if (secs < 60) {
    return `${secs}s`;
  }
  const mins = Math.floor(secs / 60);
  const remSecs = secs % 60;
  if (mins < 60) {
    return remSecs === 0 ? `${mins}m` : `${mins}m ${remSecs}s`;
  }
  const hours = Math.floor(mins / 60);
  const remMins = mins % 60;
  if (hours < 24) {
    return remMins === 0 ? `${hours}h` : `${hours}h ${remMins}m`;
  }
  const days = Math.floor(hours / 24);
  const remHours = hours % 24;
  return remHours === 0 ? `${days}d` : `${days}d ${remHours}h`;
}

/** Computes active elapsed seconds for a goal, pausing cleanly while paused or finished. */
export function computeGoalElapsedSecs(goal: GoalRunRecord, nowMs: number): number {
  const recorded = goal.active_duration_secs ?? 0;
  if (goal.status === "running" || goal.status === "verifying") {
    if (recorded > 0) {
      const delta = Math.max(0, Math.floor((nowMs - goal.updated_at_ms) / 1000));
      return recorded + delta;
    }
    const anchor = goal.updated_at_ms > goal.started_at_ms ? goal.started_at_ms : goal.updated_at_ms;
    return Math.max(0, Math.floor((nowMs - anchor) / 1000));
  }
  if (recorded > 0) {
    return recorded;
  }
  const endMs = goal.finished_at_ms ?? goal.updated_at_ms;
  return Math.max(0, Math.floor((endMs - goal.started_at_ms) / 1000));
}

export function countGoalCriteria(goal: GoalRunRecord): { passed: number; total: number } {
  const total = goal.criteria.length;
  let passed = 0;
  for (const c of goal.criteria) {
    if (c.passed) passed += 1;
  }
  return { passed, total };
}

export function isGoalActiveOrPaused(status: GoalRunRecord["status"]): boolean {
  return status === "running" || status === "verifying" || status === "paused";
}
