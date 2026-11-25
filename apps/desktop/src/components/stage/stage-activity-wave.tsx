import { memo, useCallback, useMemo } from "react";
import { useI18n } from "@/lib/i18n";
import type { ResolvedSubagentItem } from "@/lib/subagents";
import { cn } from "@/lib/utils";
import {
  selectActiveGoalForAgent,
  selectArmedTimerCountForAgent,
} from "@/state/selectors/session-selectors";
import {
  type OverviewSectionId,
  getOrbitState,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import {
  type ActiveSubagentSummary,
  type StageActivityBackgroundTaskSummary,
  type StageLiveActivityInput,
  type StageLiveActivityPart,
  type StageLiveActivityPartKind,
  type StageLiveActivitySummary,
  summarizeActiveSubagents,
  summarizeStageLiveActivity,
} from "./stage-activity-wave-model";

export {
  summarizeActiveSubagents,
  summarizeStageLiveActivity,
  type ActiveSubagentSummary,
  type StageActivityBackgroundTaskSummary,
  type StageLiveActivityInput,
  type StageLiveActivityPart,
  type StageLiveActivityPartKind,
  type StageLiveActivitySummary,
};

export interface StageActivityWaveProps extends StageLiveActivityInput {
  onClick?: () => void;
  /** Marks the control as an Overview toggle trigger so clicking does not dismiss an open narrow popover first. */
  overviewTrigger?: boolean;
  className?: string;
}

/**
 * Minimal bottom concentric ripple-wave activity line summarizing live background dependencies
 * (awaiting/running subagents, running background tasks, armed timers, or active goals)
 * without duplicating Overview HUD avatars or active turn spinners.
 */
export const StageActivityWave = memo(function StageActivityWave({
  runningSubagentsCount,
  awaitingSubagentsCount,
  runningBackgroundTasks,
  armedTimersCount,
  hasActiveGoal,
  isForegroundRunning = false,
  onClick,
  overviewTrigger = false,
  className,
}: StageActivityWaveProps) {
  const { locale } = useI18n();

  const summary = useMemo(
    () =>
      summarizeStageLiveActivity(
        {
          runningSubagentsCount,
          awaitingSubagentsCount,
          runningBackgroundTasks,
          armedTimersCount,
          hasActiveGoal,
          isForegroundRunning,
        },
        locale,
      ),
    [
      runningSubagentsCount,
      awaitingSubagentsCount,
      runningBackgroundTasks,
      armedTimersCount,
      hasActiveGoal,
      isForegroundRunning,
      locale,
    ],
  );

  if (!summary) return null;

  const isWarning = summary.tone === "warning";
  const rowClasses = cn(
    "group/wave inline-flex max-w-full min-h-[24px] min-w-0 items-center gap-2 py-0.5 text-left text-[13px] leading-[18px] tabular-nums select-none",
    onClick &&
      "cursor-pointer outline-none focus-visible:underline decoration-ink-muted underline-offset-4",
  );

  const innerContent = (
    <>
      <span
        data-slot="stage-activity-wave-orb"
        data-tone={summary.tone}
        aria-hidden="true"
        className={cn(
          "relative inline-flex size-[18px] shrink-0 items-center justify-center",
          isWarning
            ? "text-signal-warning"
            : cn("text-ink-secondary", onClick && "transition-colors group-hover/wave:text-ink-primary"),
        )}
      >
        <span className="orbit-activity-ripple-ring pointer-events-none absolute size-[7px] rounded-full border border-current" />
        <span className="orbit-activity-ripple-ring orbit-activity-ripple-ring-delayed pointer-events-none absolute size-[7px] rounded-full border border-current" />
        <span className="size-[5px] rounded-full bg-current" />
      </span>

      <span className="flex min-w-0 flex-1 items-baseline gap-1.5 overflow-hidden">
        <span className="min-w-0 truncate">
          {summary.parts.map((part, idx) => (
            <span key={part.kind}>
              {idx > 0 && (
                <span aria-hidden="true" className="text-ink-muted">
                  {" · "}
                </span>
              )}
              <span
                className={cn(
                  part.tone === "warning"
                    ? "font-medium text-signal-warning"
                    : "shimmer",
                )}
              >
                {part.text}
              </span>
            </span>
          ))}
        </span>

        {summary.secondaryHint && (
          <span
            className={cn(
              "min-w-0 shrink truncate text-ink-tertiary",
              onClick && "transition-colors group-hover/wave:text-ink-secondary",
            )}
          >
            <span aria-hidden="true" className="text-ink-muted">
              ·{" "}
            </span>
            {summary.secondaryHint}
          </span>
        )}
      </span>
    </>
  );

  return (
    <div
      data-slot="stage-activity-wave"
      data-tone={summary.tone}
      role="status"
      aria-live="polite"
      className={cn("min-w-0 px-[10px]", className)}
    >
      {onClick ? (
        <button
          type="button"
          onClick={onClick}
          data-overview-trigger={overviewTrigger ? "true" : undefined}
          title={summary.secondaryHint ? `${summary.text} · ${summary.secondaryHint}` : summary.text}
          className={rowClasses}
        >
          {innerContent}
        </button>
      ) : (
        <div
          title={summary.secondaryHint ? `${summary.text} · ${summary.secondaryHint}` : summary.text}
          className={rowClasses}
        >
          {innerContent}
        </div>
      )}
    </div>
  );
});

/**
 * Connected main-stage wrapper that isolates timer/goal/overview subscriptions from `StreamStage`,
 * makes the activity line interactive ONLY when the Overview preview card is hidden,
 * and preserves the active-turn bottom cushion when idle.
 */
export const StreamStageActivityWave = memo(function StreamStageActivityWave({
  hostId,
  agentId,
  sessionSubagents,
  runningBackgroundTasks,
  isForegroundRunning,
}: {
  hostId: string | null;
  agentId: string | null;
  sessionSubagents: readonly ResolvedSubagentItem[];
  runningBackgroundTasks: readonly StageActivityBackgroundTaskSummary[];
  isForegroundRunning: boolean;
}) {
  const liveSubagentActivity = useMemo(
    () => summarizeActiveSubagents(sessionSubagents),
    [sessionSubagents],
  );
  const armedTimersCount = useOrbitSelector((s) =>
    hostId && agentId
      ? selectArmedTimerCountForAgent(s.hostDetails[hostId], agentId)
      : 0,
  );
  const activeGoal = useOrbitSelector((s) =>
    hostId && agentId
      ? selectActiveGoalForAgent(s.hostDetails[hostId], agentId)
      : null,
  );
  const isOverviewVisible = useOrbitSelector((s) =>
    s.stageCanDockOverview ? s.overviewDesktopOpenPref : s.overviewPopoverOpen,
  );
  const hasActiveGoal = Boolean(activeGoal && activeGoal.status !== "paused");
  const hasLiveActivityWave =
    liveSubagentActivity.runningCount > 0 ||
    liveSubagentActivity.awaitingCount > 0 ||
    runningBackgroundTasks.length > 0 ||
    (!isForegroundRunning && (armedTimersCount > 0 || hasActiveGoal));

  const handleClick = useCallback(() => {
    const state = getOrbitState();
    const isOverviewOpen = state.stageCanDockOverview
      ? state.overviewDesktopOpenPref
      : state.overviewPopoverOpen;
    if (isOverviewOpen) return;

    const targetSection: OverviewSectionId =
      liveSubagentActivity.awaitingCount > 0 || liveSubagentActivity.runningCount > 0
        ? "subagents"
        : runningBackgroundTasks.length > 0
          ? "tasks"
          : "goals";
    const isSectionCollapsed = Boolean(state.overviewCollapsedSections[targetSection]);

    orbitActions.toggleOverviewPanel(true);
    if (isSectionCollapsed) {
      orbitActions.toggleOverviewSection(targetSection);
    }
  }, [
    liveSubagentActivity.awaitingCount,
    liveSubagentActivity.runningCount,
    runningBackgroundTasks.length,
  ]);

  if (!hasLiveActivityWave) {
    return isForegroundRunning ? <div aria-hidden="true" className="h-[32px] shrink-0" /> : null;
  }

  return (
    <StageActivityWave
      runningSubagentsCount={liveSubagentActivity.runningCount}
      awaitingSubagentsCount={liveSubagentActivity.awaitingCount}
      runningBackgroundTasks={runningBackgroundTasks}
      armedTimersCount={armedTimersCount}
      hasActiveGoal={hasActiveGoal}
      isForegroundRunning={isForegroundRunning}
      onClick={isOverviewVisible ? undefined : handleClick}
      overviewTrigger={!isOverviewVisible}
      className="mt-3 mb-1"
    />
  );
});
