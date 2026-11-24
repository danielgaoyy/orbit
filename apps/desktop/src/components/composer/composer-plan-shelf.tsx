import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  Check,
  CheckCircle2,
  ChevronDown,
  Circle,
  CirclePause,
  ListTodo,
  Minus,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SimpleTooltip } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Trace, TraceIcon, TraceLine, TraceTitle } from "@/components/ui/trace";
import { useI18n } from "@/lib/i18n";
import type { SessionPlanStepView } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";
import {
  PLAN_AUTO_DISMISS_MS,
  createPlanShelfCache,
  describePlan,
  reconcilePlanShelf,
  traceStatus,
  type PlanRowState,
} from "./composer-plan-shelf-model";

const sessionPreferences = createPlanShelfCache();

function PlanHeaderIcon({
  hasFailure,
  allSettled,
  isRunning,
  open,
}: {
  hasFailure: boolean;
  allSettled: boolean;
  isRunning: boolean;
  open: boolean;
}) {
  if (hasFailure) {
    return <AlertCircle className="size-[14px] stroke-[1.8] text-signal-danger" />;
  }
  if (allSettled) {
    return <CheckCircle2 className="size-[14px] stroke-[1.8] text-signal-success" />;
  }
  if (isRunning && !open) {
    return <OrbitSpinner size={13} className="size-[13px] text-ink-secondary motion-reduce:animate-none" />;
  }
  return <ListTodo className="size-[14px] stroke-[1.75] text-ink-tertiary" />;
}

function StepIcon({ state }: { state: PlanRowState }) {
  switch (state) {
    case "running":
      return <OrbitSpinner size={12} className="size-[12px] text-ink-primary motion-reduce:animate-none" />;
    case "paused":
      return <CirclePause className="size-[13px] text-ink-muted" />;
    case "completed":
      return (
        <span className="flex size-[14px] items-center justify-center rounded-full bg-signal-success/14 text-signal-success">
          <Check className="size-[9px] stroke-[2.6]" />
        </span>
      );
    case "skipped":
      return <Minus className="size-[12px] text-ink-faint" />;
    case "failed":
      return <AlertCircle className="size-[13px] text-signal-danger" />;
    case "pending":
      return <Circle className="size-[12px] stroke-[1.7] text-ink-faint" />;
  }
}

interface ShelfProps {
  hostId: string;
  agentId: string;
  planSteps: readonly SessionPlanStepView[];
  sourceTurn?: number | null;
  latestTurnAnchorSeq?: number | null;
  isForegroundRunning: boolean;
  hasPendingDecisions: boolean;
}

export const ComposerPlanShelf = memo(function ComposerPlanShelf({
  hostId,
  agentId,
  ...props
}: ShelfProps) {
  const scope = JSON.stringify([hostId, agentId]);
  return <SessionPlanShelf key={scope} scope={scope} {...props} />;
});

function SessionPlanShelf({
  scope,
  planSteps,
  sourceTurn = null,
  latestTurnAnchorSeq = null,
  isForegroundRunning,
  hasPendingDecisions,
}: Omit<ShelfProps, "hostId" | "agentId"> & { scope: string }) {
  const { t } = useI18n();
  const plan = useMemo(
    () => describePlan(planSteps, isForegroundRunning, latestTurnAnchorSeq),
    [planSteps, isForegroundRunning, latestTurnAnchorSeq],
  );
  const [saved, setSaved] = useState(() => sessionPreferences.get(scope));
  const [hovered, setHovered] = useState(false);
  const preference = reconcilePlanShelf(saved, plan, sourceTurn);
  if (preference !== saved) setSaved(preference);
  const open = Boolean(preference?.open && !hasPendingDecisions);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    // A keyboard-scrollable list loses focus when its panel disappears.
    if (!open && viewportRef.current?.contains(document.activeElement)) {
      triggerRef.current?.focus({ preventScroll: true });
    }
  }, [open]);

  useLayoutEffect(() => {
    sessionPreferences.save(scope, preference);
  }, [scope, preference]);

  // Auto-dismiss cleanly after a brief dwell once a live plan finishes all steps.
  // Pause auto-dismiss while the reader is hovering or manually inspecting the list.
  const shouldScheduleAutoDismiss = Boolean(
    plan &&
      preference &&
      !preference.dismissed &&
      preference.settledLive &&
      plan.shouldAutoDismiss &&
      !open &&
      !hovered &&
      !hasPendingDecisions,
  );
  useEffect(() => {
    if (!shouldScheduleAutoDismiss || !preference) return;
    const timer = window.setTimeout(() => {
      setSaved((prev) =>
        prev && prev.version === preference.version && prev.latestSeq === preference.latestSeq
          ? { ...prev, dismissed: true, settledLive: false }
          : prev,
      );
    }, PLAN_AUTO_DISMISS_MS);
    return () => window.clearTimeout(timer);
  }, [shouldScheduleAutoDismiss, preference]);

  if (!plan || !preference || preference.dismissed) return null;

  const { rows, done, skipped, headline } = plan;
  const progress = t("plan.progress", { done, total: rows.length });
  const skippedHint = skipped ? t("plan.skippedCount", { count: skipped }) : "";
  const summary = headline?.title ?? t(skipped ? "plan.finished" : "plan.allCompleted");
  const statusLabel =
    headline && (headline.state === "failed" || headline.state === "paused")
      ? t(`plan.${headline.state}`)
      : null;
  const isActivelyRunning = headline?.state === "running";

  return (
    <div className="w-full px-[24px] pb-2">
      <Collapsible
        data-slot="composer-plan-shelf"
        open={open}
        onOpenChange={(nextOpen) => {
          if (!hasPendingDecisions) {
            setSaved({
              ...preference,
              open: nextOpen,
              settledLive: nextOpen ? false : preference.settledLive,
            });
          }
        }}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        className={cn(
          "composer-queue-surface orbit-stage-enter mx-auto w-full max-w-[var(--orbit-stage-max-w,740px)] overflow-hidden rounded-2xl transition-[opacity,border-color] duration-180 ease-[var(--ease-out-quart)]",
          hasPendingDecisions && "opacity-75",
        )}
      >
        <div className="flex items-center">
          <CollapsibleTrigger
            ref={triggerRef}
            aria-disabled={hasPendingDecisions || undefined}
            className="group/plan flex min-h-[34px] min-w-0 flex-1 cursor-pointer items-center gap-2 px-[14px] text-left text-[13px] outline-none transition-colors hover:bg-accent/60 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50 aria-disabled:cursor-default"
          >
            <span aria-hidden="true" className="flex size-4 shrink-0 items-center justify-center">
              <PlanHeaderIcon
                hasFailure={plan.hasFailure}
                allSettled={plan.allSettled}
                isRunning={isActivelyRunning}
                open={open}
              />
            </span>
            <span className="shrink-0 font-medium text-ink-primary">{t("plan.label")}</span>
            {!open && (
              <>
                <span aria-hidden="true" className="shrink-0 text-ink-faint">·</span>
                <span
                  className={cn(
                    "min-w-0 flex-1 truncate",
                    plan.hasFailure
                      ? "text-signal-danger"
                      : plan.allSettled
                        ? "text-signal-success"
                        : isActivelyRunning
                          ? "text-ink-secondary"
                          : "text-ink-tertiary",
                  )}
                >
                  {summary}
                  {statusLabel && <span className="text-ink-muted"> ({statusLabel})</span>}
                </span>
              </>
            )}
            {open && <span className="min-w-0 flex-1" />}
            <span
              role="progressbar"
              aria-label={progress}
              aria-valuemin={0}
              aria-valuemax={rows.length}
              aria-valuenow={done}
              title={skippedHint || progress}
              className="shrink-0 font-mono text-[11px] text-ink-tertiary tabular-nums"
            >
              <span aria-hidden="true">
                {done}/{rows.length}
              </span>
            </span>
            <ChevronDown
              aria-hidden="true"
              className="size-3.5 shrink-0 text-ink-muted motion-safe:transition-transform duration-180 ease-[var(--ease-panel)] group-aria-expanded/plan:rotate-180"
            />
          </CollapsibleTrigger>
          {plan.canDismiss && (
            <SimpleTooltip label={t("plan.dismiss")} side="top">
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                className="mr-1.5 size-[22px] rounded-[6px] text-ink-muted hover:text-ink-primary"
                aria-label={t("plan.dismiss")}
                onClick={() => setSaved({ ...preference, dismissed: true, settledLive: false })}
              >
                <X className="size-3" />
              </Button>
            </SimpleTooltip>
          )}
        </div>
        <CollapsibleContent className="motion-reduce:transition-none">
          <div className="border-t border-border-subtle/80">
            <ScrollArea
              viewportRef={viewportRef}
              viewportClassName="max-h-[172px] overscroll-contain"
            >
              <Trace role="list" aria-label={t("plan.label")} className="gap-1 px-[14px] py-2">
                {rows.map(({ id, state, title }) => (
                  <TraceLine
                    key={id}
                    role="listitem"
                    status={traceStatus[state]}
                    className="min-h-[20px] items-start gap-2 text-[13px] leading-[1.45]"
                  >
                    <TraceIcon className="mt-[2px] size-[14px]">
                      <StepIcon state={state} />
                    </TraceIcon>
                    <TraceTitle
                      className={cn(
                        "whitespace-pre-wrap wrap-anywhere",
                        state === "running" && "font-medium text-ink-primary",
                        state === "completed" &&
                          "text-ink-tertiary line-through decoration-ink-faint/70",
                        state === "skipped" &&
                          "text-ink-muted line-through decoration-ink-faint/60",
                        state === "pending" && "text-ink-secondary",
                        state === "paused" && "text-ink-secondary",
                      )}
                    >
                      {title}
                      <span
                        className={cn(
                          state !== "paused" && state !== "skipped" && state !== "failed"
                            ? "sr-only"
                            : "ml-1 text-[11px] font-normal text-ink-muted no-underline",
                        )}
                      >
                        ({t(`plan.${state}`)})
                      </span>
                    </TraceTitle>
                  </TraceLine>
                ))}
              </Trace>
            </ScrollArea>
          </div>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}
