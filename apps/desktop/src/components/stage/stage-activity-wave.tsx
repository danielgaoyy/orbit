import { memo, useCallback, useMemo } from "react";
import { ChevronRight } from "lucide-react";
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
import { openSubagent } from "./subagent-crew-dock";
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
  actionHint?: string;
  /** Marks the control as an Overview toggle trigger so clicking does not dismiss an open narrow popover first. */
  overviewTrigger?: boolean;
  className?: string;
}

/**
 * Calm bottom concentric ripple-wave activity row summarizing live background dependencies
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
  actionHint,
  overviewTrigger = false,
  className,
}: StageActivityWaveProps) {
  const { t, locale } = useI18n();

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
    "group/wave -mx-[7px] flex min-h-[28px] w-[calc(100%+14px)] min-w-0 items-center gap-2 rounded-[8px] px-[7px] py-1 text-left text-[13px] leading-[18px] tabular-nums select-none",
    onClick &&
      "orbit-press cursor-pointer outline-none transition-colors hover:bg-accent/55 focus-visible:ring-2 focus-visible:ring-ring/50",
  );

  const innerContent = (
    <>
      <span
        data-slot="stage-activity-wave-orb"
        data-tone={summary.tone}
        aria-hidden="true"
        className={cn(
          "relative inline-flex size-[18px] shrink-0 items-center justify-center",
          isWarning ? "text-signal-warning" : "text-ink-secondary",
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
          <span className="min-w-0 shrink truncate text-ink-tertiary">
            <span aria-hidden="true" className="text-ink-muted">
              ·{" "}
            </span>
            {summary.secondaryHint}
          </span>
        )}
      </span>

      {onClick && (
        <span
          aria-hidden="true"
          className="ml-auto inline-flex shrink-0 items-center gap-0.5 text-[11px] text-ink-tertiary opacity-0 transition-opacity duration-[150ms] group-hover/wave:opacity-100 group-focus-visible/wave:opacity-100"
        >
          <span>{actionHint ?? t("overview.title")}</span>
          <ChevronRight className="size-3 stroke-[1.8]" />
        </span>
      )}
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
 * Connected main-stage wrapper that isolates timer/goal store subscriptions from `StreamStage`,
 * routes clicks cleanly to Overview HUD (opening/expanding the active section) or the sole
 * active subagent tab, and preserves the active-turn bottom cushion when idle.
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
  const hasActiveGoal = Boolean(activeGoal && activeGoal.status !== "paused");
  const hasLiveActivityWave =
    liveSubagentActivity.runningCount > 0 ||
    liveSubagentActivity.awaitingCount > 0 ||
    runningBackgroundTasks.length > 0 ||
    (!isForegroundRunning && (armedTimersCount > 0 || hasActiveGoal));

  const handleClick = useCallback(() => {
    const state = getOrbitState();
    const targetSection: OverviewSectionId =
      liveSubagentActivity.awaitingCount > 0 || liveSubagentActivity.runningCount > 0
        ? "subagents"
        : runningBackgroundTasks.length > 0
          ? "tasks"
          : "goals";
    const isOverviewOpen = state.stageCanDockOverview
      ? state.overviewDesktopOpenPref
      : state.overviewPopoverOpen;
    const isSectionCollapsed = Boolean(state.overviewCollapsedSections[targetSection]);

    if (!isOverviewOpen) {
      orbitActions.toggleOverviewPanel(true);
      if (isSectionCollapsed) {
        orbitActions.toggleOverviewSection(targetSection);
      }
      return;
    }

    if (isSectionCollapsed) {
      orbitActions.toggleOverviewSection(targetSection);
      return;
    }

    if (
      targetSection === "subagents" &&
      liveSubagentActivity.soleActive &&
      runningBackgroundTasks.length === 0
    ) {
      openSubagent(agentId ?? undefined, liveSubagentActivity.soleActive);
    }
  }, [
    liveSubagentActivity.awaitingCount,
    liveSubagentActivity.runningCount,
    liveSubagentActivity.soleActive,
    runningBackgroundTasks.length,
    agentId,
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
      onClick={handleClick}
      overviewTrigger
      className="mt-3 mb-1"
    />
  );
});
