import { memo, useState } from "react";
import { CheckCircle2, Pause, Play, Square, Target } from "lucide-react";
import { SimpleTooltip } from "@/components/ui/overlays";
import type { GoalControlAction, GoalRunRecord } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface GoalBannerProps {
  hostId: string | null;
  agentId: string | null;
}

export const GoalBanner = memo(function GoalBanner({ hostId, agentId }: GoalBannerProps) {
  const [busyAction, setBusyAction] = useState<GoalControlAction | null>(null);

  const activeGoal: GoalRunRecord | null = useOrbitSelector((s) => {
    if (!hostId || !agentId) return null;
    const goals = s.hostDetails[hostId]?.goal_runs ?? [];
    return (
      goals.find(
        (g) =>
          g.agent_id === agentId &&
          (g.status === "running" || g.status === "verifying" || g.status === "paused"),
      ) ?? null
    );
  });

  if (!hostId || !activeGoal) return null;

  const isPaused = activeGoal.status === "paused";
  const isVerifying = activeGoal.status === "verifying";

  const statusDotClass = isVerifying
    ? "bg-sky-500 animate-pulse"
    : isPaused
      ? "bg-amber-500"
      : "bg-emerald-500 animate-pulse";

  const statusLabel = isVerifying ? "Verifying" : isPaused ? "Paused" : "Goal";

  const handleControl = async (action: GoalControlAction) => {
    setBusyAction(action);
    try {
      await orbitActions.controlGoalRun(hostId, activeGoal.goal_id, action);
    } finally {
      setBusyAction(null);
    }
  };

  return (
    <div className="mx-auto mb-1.5 w-full max-w-[var(--orbit-stage-max-w,768px)] px-4 sm:px-6">
      <div className="border-border/80 bg-elevated/85 flex h-8 items-center justify-between gap-2.5 rounded-xl border px-3 backdrop-blur-sm">
        <div className="flex min-w-0 items-center gap-2">
          <span
            title={activeGoal.status}
            className={cn("size-1.5 shrink-0 rounded-full", statusDotClass)}
          />
          <Target className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
          <span className="text-muted-foreground shrink-0 text-[11px] font-medium">
            {statusLabel}
          </span>
          <span
            title={
              activeGoal.verify_command
                ? `${activeGoal.objective}\nVerify: ${activeGoal.verify_command}`
                : activeGoal.objective
            }
            className="text-foreground/90 min-w-0 truncate text-[12px]"
          >
            {activeGoal.objective}
          </span>
          <span className="text-muted-foreground/80 tabular-nums shrink-0 font-mono text-[11px]">
            Iter {activeGoal.current_iteration}/{activeGoal.max_iterations}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-0.5">
          {!isVerifying && (
            <SimpleTooltip label="Verify now">
              <button
                type="button"
                aria-label="Verify goal now"
                disabled={busyAction !== null}
                onClick={() => void handleControl("verify_now")}
                className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-6 cursor-pointer items-center gap-1 rounded-md px-1.5 text-[11px] font-medium transition-colors disabled:opacity-45"
              >
                <CheckCircle2 className="size-3 stroke-[1.75]" />
                <span>Verify</span>
              </button>
            </SimpleTooltip>
          )}
          <SimpleTooltip label={isPaused ? "Resume goal" : "Pause goal"}>
            <button
              type="button"
              aria-label={isPaused ? "Resume goal" : "Pause goal"}
              disabled={busyAction !== null || isVerifying}
              onClick={() => void handleControl(isPaused ? "resume" : "pause")}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-6 cursor-pointer items-center justify-center rounded-md transition-colors disabled:opacity-45"
            >
              {isPaused ? (
                <Play className="size-3 stroke-[1.75]" />
              ) : (
                <Pause className="size-3 stroke-[1.75]" />
              )}
            </button>
          </SimpleTooltip>
          <SimpleTooltip label="Stop goal">
            <button
              type="button"
              aria-label="Stop goal"
              disabled={busyAction !== null}
              onClick={() => void handleControl("cancel")}
              className="text-muted-foreground hover:bg-accent inline-flex size-6 cursor-pointer items-center justify-center rounded-md transition-colors hover:text-rose-500 disabled:opacity-45"
            >
              <Square className="size-3 stroke-[1.75]" />
            </button>
          </SimpleTooltip>
        </div>
      </div>
    </div>
  );
});
