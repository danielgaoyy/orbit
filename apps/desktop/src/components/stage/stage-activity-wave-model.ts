import { type OrbitLocale, translate } from "../../lib/i18n/index.ts";

export interface StageActivityBackgroundTaskSummary {
  readonly id: string;
  readonly title?: string | null;
  readonly command?: string | null;
}

export interface StageLiveActivityInput {
  readonly runningSubagentsCount: number;
  readonly awaitingSubagentsCount: number;
  readonly runningBackgroundTasks: readonly StageActivityBackgroundTaskSummary[];
  readonly armedTimersCount: number;
  readonly hasActiveGoal: boolean;
  readonly isForegroundRunning?: boolean;
}

export type StageLiveActivityPartKind =
  | "awaiting_subagents"
  | "running_subagents"
  | "background_tasks"
  | "timers"
  | "goal";

export interface StageLiveActivityPart {
  readonly kind: StageLiveActivityPartKind;
  readonly text: string;
  readonly tone: "warning" | "normal";
}

export interface StageLiveActivitySummary {
  readonly parts: readonly StageLiveActivityPart[];
  readonly text: string;
  readonly secondaryHint: string | null;
  readonly tone: "warning" | "normal";
  readonly primarySection: "subagents" | "tasks" | "goals";
}

export interface ActiveSubagentSummary<T extends { readonly status: string }> {
  readonly runningCount: number;
  readonly awaitingCount: number;
  readonly soleActive: T | null;
  readonly firstActive: T | null;
}

/**
 * Counts live (`running` / `awaiting_permission`) subagents in one pass without allocating,
 * tracking both the sole active subagent (if exactly 1 is active) and the highest-priority
 * active subagent (`awaiting_permission` before `running`) for direct click navigation.
 */
export function summarizeActiveSubagents<T extends { readonly status: string }>(
  subagents: readonly T[],
): ActiveSubagentSummary<T> {
  let runningCount = 0;
  let awaitingCount = 0;
  let soleActive: T | null = null;
  let firstAwaiting: T | null = null;
  let firstRunning: T | null = null;

  for (const sa of subagents) {
    if (sa.status === "awaiting_permission") {
      awaitingCount += 1;
      if (!firstAwaiting) firstAwaiting = sa;
      soleActive = runningCount + awaitingCount === 1 ? sa : null;
    } else if (sa.status === "running") {
      runningCount += 1;
      if (!firstRunning) firstRunning = sa;
      soleActive = runningCount + awaitingCount === 1 ? sa : null;
    }
  }

  return {
    runningCount,
    awaitingCount,
    soleActive,
    firstActive: firstAwaiting ?? firstRunning,
  };
}

const MAX_SECONDARY_HINT_CHARS = 60;

function formatConciseTaskHint(task: StageActivityBackgroundTaskSummary | undefined): string | null {
  if (!task) return null;
  const raw = (task.title?.trim() || task.command?.trim() || "").replace(/\s+/g, " ").trim();
  if (!raw) return null;
  if (raw.length <= MAX_SECONDARY_HINT_CHARS) return raw;
  return `${raw.slice(0, MAX_SECONDARY_HINT_CHARS - 1).trimEnd()}…`;
}

/**
 * Summarizes live background dependencies affecting a conversation viewport into a single
 * calm status line aligned with `StageOverviewPanel`.
 *
 * Returns `null` when idle (zero chrome). While the foreground turn is actively streaming,
 * suppresses non-blocking background items (running subagents, background tasks, timers,
 * standalone goals) so the stream never stacks a second animated status row beneath the live
 * step disclosure; blocking `awaiting_permission` subagents always surface immediately.
 */
export function summarizeStageLiveActivity(
  input: StageLiveActivityInput,
  locale: OrbitLocale = "en",
): StageLiveActivitySummary | null {
  const isForegroundRunning = Boolean(input.isForegroundRunning);
  const awaitingSubagents = Math.max(0, Math.floor(input.awaitingSubagentsCount));
  const runningSubagents = isForegroundRunning
    ? 0
    : Math.max(0, Math.floor(input.runningSubagentsCount));
  const bgTasks = isForegroundRunning ? [] : input.runningBackgroundTasks;
  const bgCount = bgTasks.length;
  const armedTimers = isForegroundRunning ? 0 : Math.max(0, Math.floor(input.armedTimersCount));
  const showGoal =
    !isForegroundRunning &&
    input.hasActiveGoal &&
    runningSubagents === 0 &&
    awaitingSubagents === 0 &&
    bgCount === 0 &&
    armedTimers === 0;

  const parts: StageLiveActivityPart[] = [];

  if (awaitingSubagents > 0) {
    parts.push({
      kind: "awaiting_subagents",
      text:
        awaitingSubagents === 1
          ? translate(locale, "stage.activitySubagentAwaitingOne")
          : translate(locale, "stage.activitySubagentAwaitingMany", { count: awaitingSubagents }),
      tone: "warning",
    });
  }

  if (runningSubagents > 0) {
    parts.push({
      kind: "running_subagents",
      text:
        runningSubagents === 1
          ? translate(locale, "stage.activitySubagentRunningOne")
          : translate(locale, "stage.activitySubagentRunningMany", { count: runningSubagents }),
      tone: "normal",
    });
  }

  if (bgCount > 0) {
    parts.push({
      kind: "background_tasks",
      text:
        bgCount === 1
          ? translate(locale, "stage.activityBgTaskRunningOne")
          : translate(locale, "stage.activityBgTaskRunningMany", { count: bgCount }),
      tone: "normal",
    });
  }

  if (armedTimers > 0) {
    parts.push({
      kind: "timers",
      text:
        armedTimers === 1
          ? translate(locale, "stage.activityTimerWaitingOne")
          : translate(locale, "stage.activityTimerWaitingMany", { count: armedTimers }),
      tone: "normal",
    });
  }

  if (showGoal) {
    parts.push({
      kind: "goal",
      text: translate(locale, "stage.activityGoalActive"),
      tone: "normal",
    });
  }

  if (parts.length === 0) return null;

  const secondaryHint =
    bgCount === 1 && runningSubagents === 0 && awaitingSubagents === 0
      ? formatConciseTaskHint(bgTasks[0])
      : null;

  const primarySection: StageLiveActivitySummary["primarySection"] =
    awaitingSubagents > 0 || runningSubagents > 0
      ? "subagents"
      : bgCount > 0
        ? "tasks"
        : "goals";

  return {
    parts,
    text: parts.map((part) => part.text).join(" · "),
    secondaryHint,
    tone: awaitingSubagents > 0 ? "warning" : "normal",
    primarySection,
  };
}
