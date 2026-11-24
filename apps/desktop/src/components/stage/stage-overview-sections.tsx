import { memo, useState, type ReactNode } from "react";
import {
  AlarmClock,
  ArrowUpRight,
  Blocks,
  CheckCircle2,
  ChevronRight,
  Clock,
  Code2,
  ExternalLink,
  FileText,
  GitPullRequest,
  Globe,
  Link2,
  MessageCircleQuestion,
  Pause,
  Play,
  Square,
  Target,
  Timer,
  X,
} from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { IconButton } from "@/components/ui/icon-button";
import { ListRow } from "@/components/ui/list-row";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { useLiveSecondClock } from "@/lib/live-clock";
import {
  type ArtifactSummary,
  type ConditionalTimerRecord,
  type GoalControlAction,
  type GoalRunRecord,
  type SessionBackgroundTaskView,
  type SessionPullRequestView,
  formatTimerConditionLabel,
  formatTimerCountdown,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { formatDurationCompact } from "@/lib/timeline-coalesce";
import type { OverviewLinkItem } from "@/state/selectors/session-selectors";
import {
  type OverviewSectionId,
  type SideQuestionEntry,
  orbitActions,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function cleanBackgroundOutputPreview(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const kept: string[] = [];
  for (const rawLine of raw.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    if (
      /^Created At:\s*/i.test(line) ||
      /^Completed At:\s*/i.test(line) ||
      /^Tool is running as a background task\b/i.test(line) ||
      /^Task logs are available at:\s*/i.test(line) ||
      /^Task Description:\s*/i.test(line) ||
      /^Log output:\s*$/i.test(line) ||
      /^<truncated \d+ lines>$/i.test(line)
    ) {
      continue;
    }
    kept.push(rawLine);
  }
  const joined = kept.join("\n").trim();
  return joined.length > 0 ? joined : null;
}

export const TaskElapsedLabel = memo(function TaskElapsedLabel({
  startedAtMs,
  running,
}: {
  startedAtMs: number;
  running: boolean;
}) {
  const nowMs = useLiveSecondClock(running);
  const elapsed = Math.max(0, nowMs - startedAtMs);
  if (elapsed < 1000) return null;

  return (
    <span className="text-ink-muted shrink-0 font-mono text-[11px] tabular-nums">
      {formatDurationCompact(elapsed)}
    </span>
  );
});

const TimerCountdownLabel = memo(function TimerCountdownLabel({
  fireAtMs,
}: {
  fireAtMs: number;
}) {
  const nowMs = useLiveSecondClock(true);
  return (
    <span className="text-ink-primary shrink-0 font-mono text-[11px] font-medium tabular-nums">
      {formatTimerCountdown(fireAtMs, nowMs)}
    </span>
  );
});

export const OverviewGoalRow = memo(function OverviewGoalRow({
  hostId,
  goal,
}: {
  hostId: string;
  goal: GoalRunRecord;
}) {
  const { t } = useI18n();
  const [busyAction, setBusyAction] = useState<GoalControlAction | null>(null);
  const isPaused = goal.status === "paused";
  const isVerifying = goal.status === "verifying";

  const handleControl = async (action: GoalControlAction) => {
    setBusyAction(action);
    try {
      await orbitActions.controlGoalRun(hostId, goal.goal_id, action);
    } finally {
      setBusyAction(null);
    }
  };

  return (
    <div
      title={
        goal.verify_command
          ? `${goal.objective}\n${t("overview.goalVerifyPrefix", { command: goal.verify_command })}`
          : goal.objective
      }
      className="group/goalrow hover:bg-accent/70 relative flex min-h-[38px] w-full items-center gap-2 rounded-[8px] px-2 py-1.5 transition-colors"
    >
      <div className="flex min-w-0 flex-1 items-start gap-2">
        <span className="mt-[4px] flex size-[14px] shrink-0 items-center justify-center">
          <span
            className={cn(
              "size-[6px] rounded-full",
              isVerifying
                ? "bg-signal-info animate-pulse"
                : isPaused
                  ? "bg-signal-warning"
                  : "bg-signal-success animate-pulse",
            )}
          />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-ink-primary truncate text-[12px] leading-[16px] font-medium">
            {goal.objective}
          </div>
          <div className="text-ink-tertiary flex items-center gap-1.5 text-[11px] leading-[15px]">
            <Target className="size-[11px] shrink-0 opacity-75" />
            <span>
              {isVerifying
                ? t("overview.goalStatusVerifying")
                : isPaused
                  ? t("overview.goalStatusPaused")
                  : t("overview.goalStatusRunning")}
            </span>
            <span className="opacity-45">·</span>
            <span className="font-mono tabular-nums">
              {t("overview.goalIteration", {
                current: goal.current_iteration,
                max: goal.max_iterations,
              })}
            </span>
          </div>
        </div>
      </div>

      <div className="bg-[var(--overview-surface,var(--surface-raised))]/95 border-border-subtle pointer-events-none absolute top-1/2 right-1.5 flex -translate-y-1/2 items-center gap-0.5 rounded-[7px] border p-0.5 opacity-0 shadow-xs backdrop-blur-[6px] transition-opacity duration-150 group-focus-within/goalrow:pointer-events-auto group-focus-within/goalrow:opacity-100 group-hover/goalrow:pointer-events-auto group-hover/goalrow:opacity-100">
        {!isVerifying && (
          <IconButton
            size="xs"
            tooltip={t("overview.verifyNow")}
            disabled={busyAction !== null}
            onClick={() => void handleControl("verify_now")}
          >
            <CheckCircle2 className="size-3" />
          </IconButton>
        )}
        <IconButton
          size="xs"
          tooltip={isPaused ? t("overview.resumeGoal") : t("overview.pauseGoal")}
          disabled={busyAction !== null || isVerifying}
          onClick={() => void handleControl(isPaused ? "resume" : "pause")}
        >
          {isPaused ? <Play className="size-3" /> : <Pause className="size-3" />}
        </IconButton>
        <IconButton
          size="xs"
          tone="danger"
          tooltip={t("overview.stopGoal")}
          disabled={busyAction !== null}
          onClick={() => void handleControl("cancel")}
        >
          <Square className="size-[9px] fill-current" />
        </IconButton>
      </div>
    </div>
  );
});

export const ArmedTimerRow = memo(function ArmedTimerRow({
  hostId,
  timer,
}: {
  hostId: string;
  timer: ConditionalTimerRecord;
}) {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);
  const conditionLabel = formatTimerConditionLabel(timer.condition);

  return (
    <div
      title={timer.prompt}
      className="group/timerrow hover:bg-accent/70 relative flex h-[30px] w-full items-center gap-1.5 rounded-[8px] px-2 transition-colors"
    >
      <div className="flex min-w-0 flex-1 items-center gap-1.5 text-[12px]">
        <Timer className="text-signal-info size-[13px] shrink-0 stroke-[1.8]" />
        <TimerCountdownLabel fireAtMs={timer.fire_at_ms} />
        <span className="text-ink-muted shrink-0">·</span>
        <span className="text-ink-tertiary truncate text-[11px]">{conditionLabel}</span>
      </div>
      <div className="bg-[var(--overview-surface,var(--surface-raised))]/95 border-border-subtle pointer-events-none absolute top-1/2 right-1.5 flex -translate-y-1/2 items-center gap-0.5 rounded-[7px] border p-0.5 opacity-0 shadow-xs backdrop-blur-[6px] transition-opacity duration-150 group-focus-within/timerrow:pointer-events-auto group-focus-within/timerrow:opacity-100 group-hover/timerrow:pointer-events-auto group-hover/timerrow:opacity-100">
        <IconButton
          size="xs"
          tooltip={t("overview.runTimerNow")}
          disabled={busy}
          onClick={() => {
            if (busy) return;
            setBusy(true);
            void orbitActions.fireConditionalTimerNow(hostId, timer.timer_id).then(
              () => orbitActions.showToast(t("overview.timerTriggeredToast")),
              (err) => {
                setBusy(false);
                orbitActions.showToast(
                  t("overview.timerTriggerFailedToast", { error: String(err) }),
                );
              },
            );
          }}
        >
          <Play className="size-[10px] fill-current" />
        </IconButton>
        <IconButton
          size="xs"
          tone="danger"
          tooltip={t("overview.cancelTimer")}
          disabled={busy}
          onClick={() => {
            if (busy) return;
            setBusy(true);
            void orbitActions.cancelConditionalTimer(hostId, timer.timer_id).then(
              () => orbitActions.showToast(t("overview.timerCanceledToast")),
              (err) => {
                setBusy(false);
                orbitActions.showToast(
                  t("overview.timerCancelFailedToast", { error: String(err) }),
                );
              },
            );
          }}
        >
          <X className="size-3 stroke-[1.9]" />
        </IconButton>
      </div>
    </div>
  );
});

export const OverviewBackgroundTaskRow = memo(function OverviewBackgroundTaskRow({
  hostId,
  cwd,
  task,
  isExpanded,
  onToggleExpand,
}: {
  hostId: string;
  cwd: string;
  task: SessionBackgroundTaskView;
  isExpanded: boolean;
  onToggleExpand: (taskId: string) => void;
}) {
  const { t } = useI18n();
  const cleanPreview = cleanBackgroundOutputPreview(task.output_preview);
  const hasDetails = Boolean(task.command || cleanPreview || task.log_path);
  const hasLogAction = Boolean(task.log_path);

  return (
    <div
      className={cn(
        "group/bgtask flex w-full flex-col rounded-[8px] transition-colors",
        isExpanded ? "bg-surface-recessed/70" : "hover:bg-accent/70",
      )}
    >
      <div className="relative flex h-[30px] min-w-0 w-full items-center">
        <button
          type="button"
          aria-expanded={hasDetails ? isExpanded : undefined}
          onClick={() => {
            if (!hasDetails) return;
            onToggleExpand(task.id);
          }}
          className={cn(
            "flex h-full min-w-0 flex-1 items-center gap-2 px-2 text-left",
            hasDetails ? "cursor-pointer" : "cursor-default",
          )}
        >
          <span className="flex size-[14px] shrink-0 items-center justify-center">
            {task.status === "running" ? (
              <span className="bg-signal-success size-[6px] rounded-full" />
            ) : (
              <Clock className="text-ink-muted size-[13px] stroke-[1.7]" />
            )}
          </span>
          <span className="text-ink-primary min-w-0 flex-1 truncate text-[12px] leading-[18px]">
            {task.title}
          </span>
        </button>

        {typeof task.started_at_ms === "number" && (
          <div
            className={cn(
              "shrink-0 pr-2 transition-opacity duration-150",
              hasLogAction &&
                (isExpanded
                  ? "opacity-0"
                  : "group-focus-within/bgtask:opacity-0 group-hover/bgtask:opacity-0"),
            )}
          >
            <TaskElapsedLabel
              startedAtMs={task.started_at_ms}
              running={task.status === "running"}
            />
          </div>
        )}

        {hasLogAction && (
          <div
            className={cn(
              "bg-[var(--overview-surface,var(--surface-raised))]/95 border-border-subtle absolute top-1/2 right-1.5 flex -translate-y-1/2 items-center rounded-[7px] border p-0.5 shadow-xs backdrop-blur-[6px] transition-opacity duration-150",
              isExpanded
                ? "pointer-events-auto opacity-100"
                : "pointer-events-none opacity-0 group-focus-within/bgtask:pointer-events-auto group-focus-within/bgtask:opacity-100 group-hover/bgtask:pointer-events-auto group-hover/bgtask:opacity-100",
            )}
          >
            <IconButton
              size="xs"
              tooltip={t("overview.openLog")}
              onClick={() => {
                if (task.log_path) {
                  orbitActions.openFileInSidePanel(task.log_path, {
                    hostId,
                    cwd,
                  });
                }
              }}
            >
              <ArrowUpRight className="size-3" />
            </IconButton>
          </div>
        )}
      </div>

      {hasDetails && isExpanded && (
        <div className="selectable-text flex flex-col gap-1.5 px-2.5 pt-0.5 pb-2 font-mono text-[11px] leading-relaxed">
          {task.command && (
            <div className="text-ink-secondary break-all whitespace-pre-wrap">
              <span className="text-ink-muted select-none">$ </span>
              {task.command}
            </div>
          )}
          {cleanPreview && (
            <pre className="text-ink-tertiary max-h-[88px] overflow-hidden break-words whitespace-pre-wrap">
              {cleanPreview.slice(0, 320)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
});

export const OverviewSection = memo(function OverviewSection({
  id,
  label,
  count,
  collapsed,
  isRunning = false,
  hasAlert = false,
  runningTone = "spinner",
  trailingMeta,
  headerAction,
  headerActionVisibility = "hover",
  footer,
  children,
}: {
  id: OverviewSectionId;
  label: string;
  count?: number;
  collapsed: boolean;
  isRunning?: boolean;
  hasAlert?: boolean;
  runningTone?: "spinner" | "emerald" | "info" | "timer";
  trailingMeta?: ReactNode;
  headerAction?: ReactNode;
  headerActionVisibility?: "hover" | "always";
  footer?: ReactNode;
  children: ReactNode;
}) {
  const { t } = useI18n();
  const hoverReplacesMeta = Boolean(headerAction && headerActionVisibility === "hover");

  return (
    <Collapsible
      open={!collapsed}
      onOpenChange={() => orbitActions.toggleOverviewSection(id)}
      className={cn(
        "border-border-subtle border-t px-2 pt-1.5 first:border-t-0",
        !collapsed && footer ? "pb-0" : "pb-1.5",
      )}
    >
      <div className="group/sechead hover:bg-accent/65 flex h-[28px] w-full items-center rounded-[8px] pr-1 transition-colors">
        <CollapsibleTrigger className="group/trigger focus-visible:ring-ring/50 flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded-[8px] pr-1 pl-2 text-left outline-none focus-visible:ring-2">
          <span className="text-ink-tertiary group-hover/sechead:text-ink-primary truncate text-[11px] font-medium tracking-[0.03em] uppercase transition-colors">
            {label}
          </span>
          {typeof count === "number" && (
            <span className="text-ink-muted font-mono text-[11px] tabular-nums">
              {count}
            </span>
          )}
        </CollapsibleTrigger>

        <div className="flex shrink-0 items-center gap-1">
          {headerAction && (
            <div
              className={cn(
                "items-center",
                headerActionVisibility === "always"
                  ? "flex"
                  : "hidden group-focus-within/sechead:flex group-hover/sechead:flex",
              )}
            >
              {headerAction}
            </div>
          )}
          {trailingMeta && (
            <span
              className={cn(
                "flex items-center",
                hoverReplacesMeta &&
                  "group-focus-within/sechead:hidden group-hover/sechead:hidden",
              )}
            >
              {trailingMeta}
            </span>
          )}
          {collapsed && (hasAlert || isRunning) && (
            <span
              className={cn(
                "flex items-center",
                hoverReplacesMeta &&
                  "group-focus-within/sechead:hidden group-hover/sechead:hidden",
              )}
            >
              {hasAlert ? (
                <span className="bg-signal-warning size-[6px] shrink-0 rounded-full" />
              ) : runningTone === "emerald" ? (
                <span className="bg-signal-success size-[6px] shrink-0 rounded-full" />
              ) : runningTone === "info" ? (
                <span className="bg-signal-info size-[6px] shrink-0 rounded-full" />
              ) : runningTone === "timer" ? (
                <AlarmClock className="text-ink-faint size-[11.5px] shrink-0 stroke-[1.7]" />
              ) : (
                <OrbitSpinner size={12} className="text-ink-secondary" />
              )}
            </span>
          )}
          <CollapsibleTrigger
            aria-label={t("overview.toggleSection", { label })}
            className="group/trigger text-ink-muted inline-flex size-[20px] cursor-pointer items-center justify-center rounded-[6px] outline-none"
          >
            <ChevronRight className="size-3 shrink-0 transition-transform duration-200 ease-[var(--ease-panel)] group-data-[panel-open]/trigger:rotate-90" />
          </CollapsibleTrigger>
        </div>
      </div>

      <CollapsibleContent>
        <div className="pt-1">{children}</div>
      </CollapsibleContent>
      {!collapsed && footer}
    </Collapsible>
  );
});

export const OverviewExpandFooter = memo(function OverviewExpandFooter({
  expanded,
  hiddenCount,
  onToggle,
}: {
  expanded: boolean;
  hiddenCount: number;
  onToggle: () => void;
}) {
  const { t } = useI18n();
  return (
    <div
      className={cn(
        expanded
          ? "bg-[var(--overview-surface,var(--surface-raised))]/95 sticky bottom-0 z-[2] -mx-2 pt-1 pr-2 pb-2 pl-4 backdrop-blur-[6px]"
          : "pt-0.5 pb-1.5 pl-2",
      )}
    >
      <button
        type="button"
        onClick={onToggle}
        className="text-ink-tertiary hover:text-ink-primary inline-flex cursor-pointer items-center py-0.5 text-[11px] font-medium transition-colors"
      >
        {expanded ? t("overview.showLess") : t("overview.showMore", { count: hiddenCount })}
      </button>
    </div>
  );
});

export const OverviewPullRequestRow = memo(function OverviewPullRequestRow({
  pr,
}: {
  pr: SessionPullRequestView;
}) {
  const { t } = useI18n();
  const displayTitle = pr.title?.trim() || pr.repo || pr.url;
  const fullTooltip = pr.title?.trim()
    ? `${pr.repo ? `${pr.repo} ` : ""}#${pr.number} · ${pr.title}`
    : `${pr.repo} #${pr.number}`;

  return (
    <div className="group/prrow relative">
      <ListRow
        density="compact"
        title={fullTooltip}
        aria-label={`${t("overview.openPullRequest")}: ${fullTooltip}`}
        onClick={() => void orbitIpc.openExternalBrowserUrl(pr.url)}
        leading={<GitPullRequest className="size-[13.5px] stroke-[1.75]" />}
        trailing={
          <ExternalLink className="text-ink-muted group-hover/prrow:text-ink-primary size-3 shrink-0 opacity-0 transition-opacity duration-150 group-focus-within/prrow:opacity-100 group-hover/prrow:opacity-100" />
        }
      >
        <span className="flex min-w-0 items-baseline gap-1.5">
          {pr.number > 0 && (
            <span className="text-ink-primary shrink-0 font-mono text-[11.5px] font-medium tabular-nums">
              #{pr.number}
            </span>
          )}
          <span className="text-ink-secondary group-hover/prrow:text-ink-primary min-w-0 truncate text-[12px] transition-colors">
            {displayTitle}
          </span>
        </span>
      </ListRow>
    </div>
  );
});

export const OverviewSideQuestionRow = memo(function OverviewSideQuestionRow({
  entry,
  selected,
}: {
  entry: SideQuestionEntry;
  selected: boolean;
}) {
  return (
    <ListRow
      density="compact"
      selected={selected}
      title={entry.question}
      onClick={() => orbitActions.openSideQuestionTab({ entryId: entry.id })}
      leading={
        entry.status === "streaming" ? (
          <OrbitSpinner size={12} className="text-ink-secondary" />
        ) : entry.status === "error" ? (
          <span className="bg-signal-danger size-[6px] rounded-full" />
        ) : (
          <MessageCircleQuestion className="size-[13.5px] stroke-[1.7]" />
        )
      }
    >
      <span className="text-[12px]">{entry.question}</span>
    </ListRow>
  );
});

export const OverviewArtifactRow = memo(function OverviewArtifactRow({
  hostId,
  artifact,
  selected,
}: {
  hostId: string;
  artifact: ArtifactSummary;
  selected: boolean;
}) {
  const { t } = useI18n();
  const leadingIcon =
    artifact.kind === "markdown" ? (
      <FileText className="size-[13.5px] stroke-[1.7]" />
    ) : artifact.kind === "code" || artifact.kind === "html" ? (
      <Code2 className="size-[13.5px] stroke-[1.7]" />
    ) : (
      <Blocks className="size-[13.5px] stroke-[1.7]" />
    );

  return (
    <ListRow
      density="compact"
      selected={selected}
      title={t("overview.openArtifact", { title: artifact.title })}
      onClick={() =>
        orbitActions.openArtifactsTab({
          hostId,
          workspaceId: artifact.workspace_id,
          artifactId: artifact.artifact_id,
        })
      }
      leading={leadingIcon}
      trailing={
        artifact.version > 0 ? (
          <span className="text-ink-muted font-mono text-[10.5px] tabular-nums">
            v{artifact.version}
          </span>
        ) : undefined
      }
    >
      <span className="text-[12px]">{artifact.title}</span>
    </ListRow>
  );
});

export const OverviewLinkRow = memo(function OverviewLinkRow({
  item,
  selected,
}: {
  item: OverviewLinkItem;
  selected: boolean;
}) {
  const { t } = useI18n();
  const isLocal = item.kind === "local";

  return (
    <div className="group/linkrow relative">
      <ListRow
        density="compact"
        selected={selected}
        title={isLocal ? `${t("overview.localPreview")} · ${item.url}` : item.url}
        onClick={() => {
          if (isLocal) {
            orbitActions.openBrowserTab(item.url);
          } else {
            void orbitIpc.openExternalBrowserUrl(item.url);
          }
        }}
        leading={
          isLocal ? (
            <Globe className="size-[13.5px] stroke-[1.75]" />
          ) : (
            <Link2 className="size-[13.5px] stroke-[1.7]" />
          )
        }
        trailing={
          !isLocal ? (
            <ExternalLink className="text-ink-muted group-hover/linkrow:text-ink-primary size-3 shrink-0 opacity-0 transition-opacity duration-150 group-focus-within/linkrow:opacity-100 group-hover/linkrow:opacity-100" />
          ) : undefined
        }
      >
        <span
          className={cn(
            "block truncate text-[12px]",
            isLocal && "font-mono text-[11.5px]",
          )}
        >
          {item.label}
        </span>
      </ListRow>

      {isLocal && (
        <div className="bg-[var(--overview-surface,var(--surface-raised))]/95 border-border-subtle pointer-events-none absolute top-1/2 right-1.5 flex -translate-y-1/2 items-center rounded-[7px] border p-0.5 opacity-0 shadow-xs backdrop-blur-[6px] transition-opacity duration-150 group-focus-within/linkrow:pointer-events-auto group-focus-within/linkrow:opacity-100 group-hover/linkrow:pointer-events-auto group-hover/linkrow:opacity-100">
          <IconButton
            size="xs"
            tooltip={t("overview.openExternally")}
            onClick={() => void orbitIpc.openExternalBrowserUrl(item.url)}
          >
            <ExternalLink className="size-3" />
          </IconButton>
        </div>
      )}
    </div>
  );
});
