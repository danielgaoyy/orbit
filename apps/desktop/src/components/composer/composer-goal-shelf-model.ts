import {
  GOAL_SUCCEEDED_AUTO_DISMISS_MS,
  GOAL_SUCCEEDED_FRESH_HANDOFF_MS,
  isGoalActiveOrPaused,
} from "../../lib/goal-format.ts";
import type { GoalRunRecord } from "../../lib/orbit-ipc.ts";

export { GOAL_SUCCEEDED_AUTO_DISMISS_MS, GOAL_SUCCEEDED_FRESH_HANDOFF_MS };

export const GOAL_DISMISSED_STORAGE_KEY = "orbit.dismissedGoalIds.v1";
const MAX_DISMISSED_GOAL_IDS = 64;
const MAX_CACHED_GOAL_SCOPES = 64;

export interface GoalShelfPreference {
  goalId: string;
  open: boolean;
  dismissed: boolean;
  observedActive: boolean;
  settledLive: boolean;
  settledTurnAnchorSeq: number | null;
}

export interface GoalShelfStorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function defaultStorage(): GoalShelfStorageLike | null {
  try {
    return typeof window !== "undefined" && window.localStorage ? window.localStorage : null;
  } catch {
    return null;
  }
}

export function createDismissedGoalStore(storage: GoalShelfStorageLike | null = defaultStorage()) {
  const ordered: string[] = [];
  const lookup = new Set<string>();

  if (storage) {
    try {
      const raw = storage.getItem(GOAL_DISMISSED_STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
          for (const item of parsed) {
            if (typeof item === "string" && item.trim() && !lookup.has(item)) {
              ordered.push(item);
              lookup.add(item);
            }
          }
          while (ordered.length > MAX_DISMISSED_GOAL_IDS) {
            const oldest = ordered.shift();
            if (oldest) lookup.delete(oldest);
          }
        }
      }
    } catch {
      // Corrupt or unavailable storage degrades gracefully to in-memory tracking.
    }
  }

  return {
    has(goalId: string): boolean {
      return lookup.has(goalId);
    },
    add(goalId: string): void {
      if (!goalId || lookup.has(goalId)) return;
      ordered.push(goalId);
      lookup.add(goalId);
      while (ordered.length > MAX_DISMISSED_GOAL_IDS) {
        const oldest = ordered.shift();
        if (oldest) lookup.delete(oldest);
      }
      if (storage) {
        try {
          storage.setItem(GOAL_DISMISSED_STORAGE_KEY, JSON.stringify(ordered));
        } catch {
          // Ignore storage quota / privacy mode failures.
        }
      }
    },
  };
}

export function createGoalShelfCache(limit = MAX_CACHED_GOAL_SCOPES) {
  const entries = new Map<string, GoalShelfPreference>();
  return {
    get(scope: string): GoalShelfPreference | null {
      return entries.get(scope) ?? null;
    },
    save(scope: string, pref: GoalShelfPreference | null): void {
      if (!pref) {
        entries.delete(scope);
        return;
      }
      entries.delete(scope);
      entries.set(scope, pref);
      while (entries.size > limit) {
        const oldest = entries.keys().next().value;
        if (oldest === undefined) break;
        entries.delete(oldest);
      }
    },
  };
}

/**
 * Reconciles Goal Dock visibility and disclosure state across live transitions, reloads,
 * manual dismissals, and subsequent user turns (matching Codex + Orbit Plan Shelf semantics).
 */
export function reconcileGoalShelf(
  prev: GoalShelfPreference | null,
  goal: GoalRunRecord | null,
  latestTurnAnchorSeq: number | null,
  isDismissed: (goalId: string) => boolean,
  nowMs: number,
): GoalShelfPreference | null {
  if (!goal) return null;

  const sameGoal = prev?.goalId === goal.goal_id;

  if (isGoalActiveOrPaused(goal.status)) {
    const nextOpen = sameGoal ? prev.open : false;
    if (
      sameGoal &&
      !prev.dismissed &&
      prev.observedActive &&
      !prev.settledLive &&
      prev.settledTurnAnchorSeq === null &&
      prev.open === nextOpen
    ) {
      return prev;
    }
    return {
      goalId: goal.goal_id,
      open: nextOpen,
      dismissed: false,
      observedActive: true,
      settledLive: false,
      settledTurnAnchorSeq: null,
    };
  }

  if (goal.status === "succeeded") {
    if (isDismissed(goal.goal_id) || (sameGoal && prev.dismissed)) {
      if (sameGoal && prev.dismissed && !prev.settledLive) return prev;
      return {
        goalId: goal.goal_id,
        open: false,
        dismissed: true,
        observedActive: sameGoal ? prev.observedActive : false,
        settledLive: false,
        settledTurnAnchorSeq: sameGoal ? prev.settledTurnAnchorSeq : null,
      };
    }

    const wasObservedActive = Boolean(sameGoal && prev.observedActive);
    const wasSettledLive = Boolean(sameGoal && prev.settledLive);
    const finishedAt = goal.finished_at_ms ?? goal.updated_at_ms;
    const isFreshHandoff = !sameGoal && nowMs - finishedAt <= GOAL_SUCCEEDED_FRESH_HANDOFF_MS;

    if (!wasObservedActive && !wasSettledLive && !isFreshHandoff) {
      return {
        goalId: goal.goal_id,
        open: false,
        dismissed: true,
        observedActive: false,
        settledLive: false,
        settledTurnAnchorSeq: null,
      };
    }

    let settledAnchor = wasSettledLive ? prev!.settledTurnAnchorSeq : latestTurnAnchorSeq;
    // If settled while an optimistic turn (anchor_seq === 0) was still committing, bind to its
    // committed non-zero anchor once it arrives instead of treating it as a brand-new user turn.
    if (wasSettledLive && settledAnchor === 0 && latestTurnAnchorSeq !== null && latestTurnAnchorSeq > 0) {
      settledAnchor = latestTurnAnchorSeq;
    }

    const startedNewTurnAfterSettlement =
      wasSettledLive &&
      settledAnchor !== null &&
      settledAnchor > 0 &&
      latestTurnAnchorSeq !== null &&
      (latestTurnAnchorSeq === 0 || latestTurnAnchorSeq > settledAnchor);

    if (startedNewTurnAfterSettlement) {
      return {
        goalId: goal.goal_id,
        open: false,
        dismissed: true,
        observedActive: true,
        settledLive: false,
        settledTurnAnchorSeq: settledAnchor,
      };
    }

    const nextOpen = sameGoal ? prev.open : false;
    if (
      sameGoal &&
      !prev.dismissed &&
      prev.settledLive &&
      prev.open === nextOpen &&
      prev.settledTurnAnchorSeq === settledAnchor
    ) {
      return prev;
    }

    return {
      goalId: goal.goal_id,
      open: nextOpen,
      dismissed: false,
      observedActive: true,
      settledLive: true,
      settledTurnAnchorSeq: settledAnchor,
    };
  }

  return null;
}
