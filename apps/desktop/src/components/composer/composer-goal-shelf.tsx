import { memo, useCallback, useEffect, useState } from "react";
import {
  AlertCircle,
  Check,
  CheckCircle2,
  ChevronDown,
  Circle,
  Pause,
  PauseCircle,
  Pencil,
  Play,
  Square,
  Target,
  Terminal,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  GOAL_SUCCEEDED_AUTO_DISMISS_MS,
  GOAL_SUCCEEDED_RECENT_WINDOW_MS,
  computeGoalElapsedSecs,
  countGoalCriteria,
  formatGoalDuration,
  isGoalActiveOrPaused,
} from "@/lib/goal-format";
import { useI18n } from "@/lib/i18n";
import { useLiveSecondClock } from "@/lib/live-clock";
import type { GoalControlAction, GoalRunRecord } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { selectDockGoalForAgent } from "@/state/selectors/session-selectors";

const dismissedSucceededGoalIds = new Set<string>();

const GoalElapsedLabel = memo(function GoalElapsedLabel({ goal }: { goal: GoalRunRecord }) {
  const isLive = goal.status === "running" || goal.status === "verifying";
  const nowMs = useLiveSecondClock(isLive);
  const secs = computeGoalElapsedSecs(goal, isLive ? nowMs : Date.now());
  return (
    <span className="shrink-0 font-mono text-[11px] text-ink-tertiary tabular-nums">
      {formatGoalDuration(secs)}
    </span>
  );
});

function GoalStatusIcon({ status }: { status: GoalRunRecord["status"] }) {
  if (status === "running") {
    return <OrbitSpinner size={13} className="size-[13px] text-signal-success motion-reduce:animate-none" />;
  }
  if (status === "verifying") {
    return <OrbitSpinner size={13} className="size-[13px] text-signal-info motion-reduce:animate-none" />;
  }
  if (status === "paused") {
    return <PauseCircle className="size-[14px] stroke-[1.8] text-signal-warning" />;
  }
  if (status === "succeeded") {
    return <CheckCircle2 className="size-[14px] stroke-[1.8] text-signal-success" />;
  }
  return <Target className="size-[14px] stroke-[1.75] text-ink-tertiary" />;
}

interface ComposerGoalShelfProps {
  hostId: string;
  agentId: string;
  hasPendingDecisions: boolean;
}

export const ComposerGoalShelf = memo(function ComposerGoalShelf({
  hostId,
  agentId,
  hasPendingDecisions,
}: ComposerGoalShelfProps) {
  const goal = useOrbitSelector((s) => selectDockGoalForAgent(s.hostDetails[hostId], agentId));
  const [dismissVersion, setDismissVersion] = useState(0);

  const handleDismissSucceeded = useCallback((goalId: string) => {
    dismissedSucceededGoalIds.add(goalId);
    setDismissVersion((v) => v + 1);
  }, []);

  if (!goal) return null;

  // Never show stale completed goals when reopening an old thread.
  if (goal.status === "succeeded") {
    if (dismissedSucceededGoalIds.has(goal.goal_id)) return null;
    const finishedAt = goal.finished_at_ms ?? goal.updated_at_ms;
    if (Date.now() - finishedAt > GOAL_SUCCEEDED_RECENT_WINDOW_MS) {
      return null;
    }
  }

  return (
    <VisibleGoalShelf
      key={`${hostId}::${agentId}::${goal.goal_id}::${dismissVersion}`}
      hostId={hostId}
      goal={goal}
      hasPendingDecisions={hasPendingDecisions}
      onDismissSucceeded={handleDismissSucceeded}
    />
  );
});

function VisibleGoalShelf({
  hostId,
  goal,
  hasPendingDecisions,
  onDismissSucceeded,
}: {
  hostId: string;
  goal: GoalRunRecord;
  hasPendingDecisions: boolean;
  onDismissSucceeded: (goalId: string) => void;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [hovered, setHovered] = useState(false);
  const [busyAction, setBusyAction] = useState<GoalControlAction | null>(null);

  const isRunning = goal.status === "running";
  const isVerifying = goal.status === "verifying";
  const isPaused = goal.status === "paused";
  const isSucceeded = goal.status === "succeeded";
  const isControllable = isGoalActiveOrPaused(goal.status);
  const effectiveOpen = Boolean(open && !hasPendingDecisions);

  // Auto-dismiss a live completion banner after a brief dwell unless inspected or hovered.
  useEffect(() => {
    if (!isSucceeded || effectiveOpen || hovered || hasPendingDecisions) return;
    const timer = window.setTimeout(() => {
      onDismissSucceeded(goal.goal_id);
    }, GOAL_SUCCEEDED_AUTO_DISMISS_MS);
    return () => window.clearTimeout(timer);
  }, [isSucceeded, effectiveOpen, hovered, hasPendingDecisions, goal.goal_id, onDismissSucceeded]);

  const handleControl = async (action: GoalControlAction) => {
    if (busyAction !== null) return;
    setBusyAction(action);
    try {
      await orbitActions.controlGoalRun(hostId, goal.goal_id, action);
    } finally {
      setBusyAction(null);
    }
  };

  const { passed, total } = countGoalCriteria(goal);
  const statusLabel = isVerifying
    ? t("goal.dockVerifying")
    : isPaused
      ? t("goal.dockPaused")
      : isSucceeded
        ? t("goal.dockSucceeded")
        : t("goal.dockRunning");

  return (
    <div className="w-full px-[24px] pb-2">
      <Collapsible
        data-slot="composer-goal-shelf"
        open={effectiveOpen}
        onOpenChange={(next) => {
          if (!hasPendingDecisions) setOpen(next);
        }}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        className={cn(
          "composer-queue-surface orbit-stage-enter mx-auto w-full max-w-[var(--orbit-stage-max-w,740px)] overflow-hidden rounded-2xl transition-[opacity,border-color] duration-180 ease-[var(--ease-out-quart)]",
          isPaused && "border-signal-warning-border/75",
          hasPendingDecisions && "opacity-75",
        )}
      >
        <div
          className={cn(
            "flex items-center transition-colors",
            !hasPendingDecisions && "hover:bg-accent/45",
          )}
        >
          <CollapsibleTrigger
            aria-disabled={hasPendingDecisions || undefined}
            className={cn(
              "group/goal flex min-h-[34px] min-w-0 flex-1 cursor-pointer items-center gap-2 pl-[14px] text-left text-[13px] outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50 aria-disabled:cursor-default",
              isControllable || isSucceeded ? "pr-2" : "pr-[14px]",
            )}
          >
            <span aria-hidden="true" className="flex size-4 shrink-0 items-center justify-center">
              <GoalStatusIcon status={goal.status} />
            </span>
            <span
              className={cn(
                "shrink-0 font-medium",
                isPaused
                  ? "text-signal-warning"
                  : isSucceeded
                    ? "text-signal-success"
                    : "text-ink-primary",
              )}
            >
              {statusLabel}
            </span>
            {!effectiveOpen && (
              <>
                <span aria-hidden="true" className="shrink-0 text-ink-faint">·</span>
                <span
                  className={cn(
                    "min-w-0 flex-1 truncate",
                    isRunning || isVerifying ? "text-ink-secondary" : "text-ink-tertiary",
                  )}
                >
                  {goal.objective}
                </span>
              </>
            )}
            {effectiveOpen && <span className="min-w-0 flex-1" />}
            {total > 0 && (
              <span
                title={t("goal.criteriaProgress", { done: passed, total })}
                className="shrink-0 font-mono text-[11px] text-ink-tertiary tabular-nums"
              >
                {passed}/{total}
              </span>
            )}
            <GoalElapsedLabel goal={goal} />
            <ChevronDown
              aria-hidden="true"
              className="size-3.5 shrink-0 text-ink-muted motion-safe:transition-transform duration-180 ease-[var(--ease-panel)] group-aria-expanded/goal:rotate-180"
            />
          </CollapsibleTrigger>

          <div className="mr-1.5 flex shrink-0 items-center gap-0.5">
            {isControllable && !isVerifying && goal.verify_command && (
              <IconButton
                size="xs"
                tooltip={t("overview.verifyNow")}
                disabled={busyAction !== null}
                onClick={() => void handleControl("verify_now")}
              >
                <CheckCircle2 className="size-3" />
              </IconButton>
            )}
            {isControllable && !isVerifying && (
              <IconButton
                size="xs"
                tooltip={isPaused ? t("overview.resumeGoal") : t("overview.pauseGoal")}
                disabled={busyAction !== null}
                onClick={() => void handleControl(isPaused ? "resume" : "pause")}
              >
                {isPaused ? <Play className="size-3" /> : <Pause className="size-3" />}
              </IconButton>
            )}
            {isControllable && (
              <IconButton
                size="xs"
                tooltip={t("goal.editAction")}
                disabled={busyAction !== null}
                onClick={() => orbitActions.openGoalEditDialog(hostId, goal)}
              >
                <Pencil className="size-3" />
              </IconButton>
            )}
            {isControllable && (
              <IconButton
                size="xs"
                tone="danger"
                tooltip={t("overview.stopGoal")}
                disabled={busyAction !== null}
                onClick={() => void handleControl("cancel")}
              >
                <Square className="size-[9px] fill-current" />
              </IconButton>
            )}
            {isSucceeded && (
              <IconButton
                size="xs"
                tooltip={t("plan.dismiss")}
                onClick={() => onDismissSucceeded(goal.goal_id)}
              >
                <X className="size-3" />
              </IconButton>
            )}
          </div>
        </div>

        <CollapsibleContent className="motion-reduce:transition-none">
          <div className="border-t border-border-subtle/80">
            <ScrollArea viewportClassName="max-h-[216px] overscroll-contain">
              <div className="space-y-2.5 px-[14px] py-2.5 text-[13px] leading-[1.45]">
                <p className="select-text whitespace-pre-wrap break-words text-ink-primary">
                  {goal.objective}
                </p>

                {isPaused && (goal.pause_reason || !goal.paused_by_user) && (
                  <div className="border-signal-warning-border bg-signal-warning-bg/55 flex items-start justify-between gap-2.5 rounded-[10px] border px-2.5 py-2 text-[12px]">
                    <div className="flex min-w-0 items-start gap-2">
                      <AlertCircle className="text-signal-warning mt-0.5 size-3.5 shrink-0" />
                      <div className="min-w-0 space-y-0.5">
                        {goal.pause_reason && (
                          <div className="text-ink-primary break-words">{goal.pause_reason}</div>
                        )}
                        {!goal.paused_by_user && (
                          <div className="text-ink-tertiary">{t("goal.autoResumeHint")}</div>
                        )}
                      </div>
                    </div>
                    <Button
                      type="button"
                      size="xs"
                      variant="outline"
                      disabled={busyAction !== null}
                      onClick={() => void handleControl("resume")}
                      className="shrink-0"
                    >
                      <Play className="mr-1 size-3" />
                      {t("goal.resumeShort")}
                    </Button>
                  </div>
                )}

                {total > 0 && (
                  <ul className="space-y-1">
                    {goal.criteria.map((c) => (
                      <li key={c.id} className="flex items-start gap-2 text-[12.5px] leading-[1.4]">
                        <span className="mt-[2px] flex size-[14px] shrink-0 items-center justify-center">
                          {c.passed ? (
                            <span className="flex size-[14px] items-center justify-center rounded-full bg-signal-success/14 text-signal-success">
                              <Check className="size-[9px] stroke-[2.6]" />
                            </span>
                          ) : (
                            <Circle className="size-[12px] stroke-[1.7] text-ink-faint" />
                          )}
                        </span>
                        <span
                          className={cn(
                            "min-w-0 flex-1 break-words",
                            c.passed
                              ? "text-ink-tertiary line-through decoration-ink-faint/70"
                              : "text-ink-secondary",
                          )}
                        >
                          {c.label}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}

                {goal.verify_command && (
                  <div className="space-y-1">
                    <div className="flex items-center gap-2 text-[11.5px]">
                      <Terminal className="size-3 shrink-0 text-ink-muted" />
                      <code className="min-w-0 flex-1 truncate font-mono text-ink-secondary">
                        $ {goal.verify_command}
                      </code>
                      {goal.last_verify_exit_code != null && (
                        <span
                          className={cn(
                            "shrink-0 rounded px-1.5 py-0.5 font-mono text-[10.5px] tabular-nums",
                            goal.last_verify_exit_code === 0
                              ? "bg-signal-success/12 text-signal-success"
                              : "bg-signal-danger-bg text-signal-danger",
                          )}
                        >
                          exit {goal.last_verify_exit_code}
                        </span>
                      )}
                    </div>
                    {goal.last_verify_exit_code != null &&
                      goal.last_verify_exit_code !== 0 &&
                      goal.last_verify_output && (
                        <pre className="bg-surface-sunken/70 border-border-subtle/70 max-h-[84px] overflow-auto rounded-[8px] border px-2 py-1.5 font-mono text-[11px] leading-[1.4] whitespace-pre-wrap break-all text-ink-tertiary">
                          {goal.last_verify_output.trim().slice(-600)}
                        </pre>
                      )}
                  </div>
                )}

                <div className="flex items-center justify-between gap-2 pt-0.5 text-[11.5px] text-ink-tertiary">
                  <div className="flex items-center gap-2">
                    <span className="font-mono tabular-nums">
                      {t("goal.turnCount", {
                        current: goal.current_iteration,
                        max: goal.max_iterations,
                      })}
                    </span>
                    <span className="opacity-45">·</span>
                    <GoalElapsedLabel goal={goal} />
                  </div>
                  {isControllable && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="xs"
                      className="h-[22px] px-2 text-[11.5px] text-ink-secondary hover:text-ink-primary"
                      onClick={() => orbitActions.openGoalEditDialog(hostId, goal)}
                    >
                      <Pencil className="mr-1 size-3" />
                      {t("goal.editAction")}
                    </Button>
                  )}
                </div>
              </div>
            </ScrollArea>
          </div>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}
