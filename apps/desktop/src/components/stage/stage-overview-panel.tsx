import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import {
  ArrowUpRight,
  Blocks,
  CheckCircle2,
  ChevronRight,
  Clock,
  File,
  FileDiff,
  FileText,
  Pause,
  Play,
  Square,
  SquareTerminal,
  Target,
  Timer,
  X,
} from "lucide-react";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import {
  SubagentCrewList,
  archiveSettledSubagentsList,
} from "@/components/stage/subagent-crew-dock";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { DiffStat } from "@/components/ui/diff-stat";
import { IconButton } from "@/components/ui/icon-button";
import { ListRow } from "@/components/ui/list-row";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { Separator } from "@/components/ui/separator";
import {
  type AgentCardView,
  type ConditionalTimerRecord,
  type DiffFileView,
  type GoalControlAction,
  type GoalRunRecord,
  type SessionInsightsView,
  formatTimerConditionLabel,
  formatTimerCountdown,
  isAgentBackgroundWaiting,
  isAgentForegroundRunning,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { formatDurationCompact } from "@/lib/timeline-coalesce";
import {
  selectSessionRuntimeActivity,
  selectSessionSubagents,
  selectWorkspaceToolSummary,
} from "@/state/selectors/session-selectors";
import {
  type OverviewSectionId,
  orbitActions,
  useOrbitSelector,
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

function TaskElapsedLabel({ startedAtMs }: { startedAtMs: number }) {
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  const elapsed = Math.max(0, nowMs - startedAtMs);
  if (elapsed < 1000) return null;

  return (
    <span className="text-ink-muted tabular-nums shrink-0 font-mono text-[11px]">
      {formatDurationCompact(elapsed)}
    </span>
  );
}

const OverviewGoalRow = memo(function OverviewGoalRow({
  hostId,
  goal,
}: {
  hostId: string;
  goal: GoalRunRecord;
}) {
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
      title={goal.verify_command ? `${goal.objective}\nVerify: ${goal.verify_command}` : goal.objective}
      className="group/goalrow hover:bg-accent/70 flex min-h-[38px] w-full items-center justify-between gap-2 rounded-[8px] px-2 py-1.5 transition-colors"
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
            <span>{isVerifying ? "Verifying" : isPaused ? "Paused" : "Goal"}</span>
            <span className="opacity-45">·</span>
            <span className="font-mono tabular-nums">
              Iter {goal.current_iteration}/{goal.max_iterations}
            </span>
          </div>
        </div>
      </div>

      <div className="hidden shrink-0 items-center gap-0.5 group-focus-within/goalrow:flex group-hover/goalrow:flex">
        {!isVerifying && (
          <IconButton
            size="xs"
            tooltip="Verify now"
            disabled={busyAction !== null}
            onClick={() => void handleControl("verify_now")}
          >
            <CheckCircle2 className="size-3" />
          </IconButton>
        )}
        <IconButton
          size="xs"
          tooltip={isPaused ? "Resume goal" : "Pause goal"}
          disabled={busyAction !== null || isVerifying}
          onClick={() => void handleControl(isPaused ? "resume" : "pause")}
        >
          {isPaused ? <Play className="size-3" /> : <Pause className="size-3" />}
        </IconButton>
        <IconButton
          size="xs"
          tone="danger"
          tooltip="Stop goal"
          disabled={busyAction !== null}
          onClick={() => void handleControl("cancel")}
        >
          <Square className="size-[9px] fill-current" />
        </IconButton>
      </div>
    </div>
  );
});

const ArmedTimerRow = memo(function ArmedTimerRow({
  hostId,
  timer,
  nowMs,
}: {
  hostId: string;
  timer: ConditionalTimerRecord;
  nowMs: number;
}) {
  const [busy, setBusy] = useState(false);
  const countdown = formatTimerCountdown(timer.fire_at_ms, nowMs);
  const conditionLabel = formatTimerConditionLabel(timer.condition);

  return (
    <div
      title={timer.prompt}
      className="group/timerrow hover:bg-accent/70 flex h-[30px] w-full items-center justify-between gap-1.5 rounded-[8px] px-2 transition-colors"
    >
      <div className="flex min-w-0 items-center gap-1.5 text-[12px]">
        <Timer className="text-signal-warning size-[13px] shrink-0 stroke-[1.8]" />
        <span className="text-ink-primary tabular-nums shrink-0 font-mono text-[11px] font-medium">
          {countdown}
        </span>
        <span className="text-ink-muted shrink-0">·</span>
        <span className="text-ink-tertiary truncate text-[11px]">{conditionLabel}</span>
      </div>
      <div className="hidden shrink-0 items-center gap-0.5 group-focus-within/timerrow:flex group-hover/timerrow:flex">
        <IconButton
          size="xs"
          tooltip="Run now"
          disabled={busy}
          onClick={() => {
            if (busy) return;
            setBusy(true);
            void orbitActions.fireConditionalTimerNow(hostId, timer.timer_id).then(
              () => orbitActions.showToast("Timer triggered"),
              (err) => {
                setBusy(false);
                orbitActions.showToast(`Failed to trigger timer: ${String(err)}`);
              },
            );
          }}
        >
          <Play className="size-[10px] fill-current" />
        </IconButton>
        <IconButton
          size="xs"
          tone="danger"
          tooltip="Cancel timer"
          disabled={busy}
          onClick={() => {
            if (busy) return;
            setBusy(true);
            void orbitActions.cancelConditionalTimer(hostId, timer.timer_id).then(
              () => orbitActions.showToast("Timer canceled"),
              (err) => {
                setBusy(false);
                orbitActions.showToast(`Failed to cancel timer: ${String(err)}`);
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

const OverviewSection = memo(function OverviewSection({
  id,
  label,
  count,
  collapsed,
  isRunning = false,
  hasAlert = false,
  runningTone = "spinner",
  trailingMeta,
  headerAction,
  footer,
  children,
}: {
  id: OverviewSectionId;
  label: string;
  count?: number;
  collapsed: boolean;
  isRunning?: boolean;
  hasAlert?: boolean;
  runningTone?: "spinner" | "emerald";
  trailingMeta?: React.ReactNode;
  headerAction?: React.ReactNode;
  footer?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <>
      <Separator className="bg-border-subtle" />
      <Collapsible
        open={!collapsed}
        onOpenChange={() => orbitActions.toggleOverviewSection(id)}
        className={cn("px-2 pt-1.5", !collapsed && footer ? "pb-0" : "pb-1.5")}
      >
        <div className="group/sechead hover:bg-accent/65 flex h-[28px] w-full items-center rounded-[8px] pr-1 transition-colors">
          <CollapsibleTrigger className="group/trigger focus-visible:ring-ring/50 flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded-[8px] pr-1 pl-2 text-left outline-none focus-visible:ring-2">
            <span className="text-ink-tertiary group-hover/sechead:text-ink-primary truncate text-[11px] font-medium tracking-[0.03em] uppercase transition-colors">
              {label}
            </span>
            {typeof count === "number" && (
              <span className="text-ink-muted tabular-nums font-mono text-[11px]">
                {count}
              </span>
            )}
          </CollapsibleTrigger>

          <div className="flex shrink-0 items-center gap-1">
            {headerAction && (
              <div className="hidden items-center group-focus-within/sechead:flex group-hover/sechead:flex">
                {headerAction}
              </div>
            )}
            {trailingMeta && (
              <span
                className={cn(
                  "flex items-center",
                  headerAction && "group-focus-within/sechead:hidden group-hover/sechead:hidden",
                )}
              >
                {trailingMeta}
              </span>
            )}
            {collapsed && (hasAlert || isRunning) && (
              <span
                className={cn(
                  "flex items-center",
                  headerAction && "group-focus-within/sechead:hidden group-hover/sechead:hidden",
                )}
              >
                {hasAlert ? (
                  <span className="bg-signal-warning size-[6px] shrink-0 rounded-full" />
                ) : runningTone === "emerald" ? (
                  <span className="bg-signal-success size-[6px] shrink-0 rounded-full" />
                ) : (
                  <OrbitSpinner size={12} className="text-ink-secondary" />
                )}
              </span>
            )}
            <CollapsibleTrigger
              aria-label={`Toggle ${label}`}
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
    </>
  );
});

interface StageOverviewContentProps {
  hostId: string;
  agentId: string;
  cwd: string;
  insights: SessionInsightsView;
  sessionTurnDiffs: readonly DiffFileView[];
  currentAgent: AgentCardView | null;
  cardMaxHeightExpr: string;
}

const StageOverviewBody = memo(function StageOverviewBody({
  hostId,
  agentId,
  cwd,
  insights,
  sessionTurnDiffs,
  currentAgent,
  cardMaxHeightExpr,
}: StageOverviewContentProps) {
  const collapsedSections = useOrbitSelector((s) => s.overviewCollapsedSections);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const fleet = useOrbitSelector((s) => s.fleet);
  const gitReviews = useOrbitSelector((s) => s.gitReviews);
  const terminalTabs = useOrbitSelector((s) => s.terminalTabs);
  const archivedSubagents = useOrbitSelector((s) => s.archivedSubagents);
  const activeStage = useOrbitSelector((s) => s.stages[`${hostId}::${agentId}`] ?? null);

  const [expandedTasks, setExpandedTasks] = useState<Record<string, boolean>>({});
  const [stoppingBg, setStoppingBg] = useState(false);
  const [showAllTasks, setShowAllTasks] = useState(false);

  const selectorSlice = useMemo(
    () => ({
      hostDetails,
      fleet,
      gitReviews,
      terminalTabs,
      archivedSubagents,
      stages: activeStage ? { [`${hostId}::${agentId}`]: activeStage } : {},
    }),
    [hostDetails, fleet, gitReviews, terminalTabs, archivedSubagents, activeStage, hostId, agentId],
  );

  const toolSummary = useMemo(
    () =>
      selectWorkspaceToolSummary(
        selectorSlice,
        hostId,
        agentId,
        cwd,
        sessionTurnDiffs,
        currentAgent,
      ),
    [selectorSlice, hostId, agentId, cwd, sessionTurnDiffs, currentAgent],
  );

  const subagentSummary = useMemo(
    () => selectSessionSubagents(selectorSlice, hostId, agentId),
    [selectorSlice, hostId, agentId],
  );

  const runtimeActivity = useMemo(
    () => selectSessionRuntimeActivity(selectorSlice, hostId, agentId),
    [selectorSlice, hostId, agentId],
  );

  const backgroundTasks = insights.background_tasks;
  const hasRunningBgTask = backgroundTasks.some((t) => t.status === "running");
  const canStopBackground =
    hasRunningBgTask &&
    isAgentBackgroundWaiting(currentAgent) &&
    !isAgentForegroundRunning(currentAgent);

  useEffect(() => {
    setStoppingBg(false);
    setShowAllTasks(false);
  }, [agentId, backgroundTasks.length]);

  const armedTimers = runtimeActivity.armedTimers;
  const activeGoal = runtimeActivity.activeGoal;
  const hasArmedTimers = armedTimers.length > 0;
  const [timerNowMs, setTimerNowMs] = useState(() => Date.now());

  useEffect(() => {
    if (!hasArmedTimers) return;
    setTimerNowMs(Date.now());
    const id = window.setInterval(() => setTimerNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [hasArmedTimers]);

  const layer1Ref = useRef<HTMLDivElement | null>(null);
  const [layer1HeightPx, setLayer1HeightPx] = useState(148);

  useLayoutEffect(() => {
    const el = layer1Ref.current;
    if (!el) return;
    const syncHeight = () => {
      const next = Math.ceil(el.getBoundingClientRect().height);
      if (next > 0) {
        setLayer1HeightPx((prev) => (prev === next ? prev : next));
      }
    };
    syncHeight();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(syncHeight);
    ro.observe(el);
    return () => ro.disconnect();
  }, [toolSummary.showGroupNotes, toolSummary.artifactCount, cwd]);

  const orderedTasks = useMemo(() => {
    const running = backgroundTasks.filter((t) => t.status === "running");
    const rest = backgroundTasks.filter((t) => t.status !== "running");
    return [...running, ...rest];
  }, [backgroundTasks]);

  const runningTaskCount = orderedTasks.filter((t) => t.status === "running").length;
  const defaultVisibleTaskCount =
    runningTaskCount > 0 ? Math.min(Math.max(runningTaskCount, 2), 3) : 2;
  const visibleTasks = showAllTasks
    ? orderedTasks
    : orderedTasks.slice(0, defaultVisibleTaskCount);
  const hiddenTaskCount = Math.max(0, orderedTasks.length - visibleTasks.length);

  const settledSubagents = useMemo(
    () =>
      subagentSummary.visible.filter(
        (sa) => sa.status !== "running" && sa.status !== "awaiting_permission",
      ),
    [subagentSummary.visible],
  );

  const touchedFiles = toolSummary.touchedFiles;
  const hasGoalOrTimers = Boolean(activeGoal) || hasArmedTimers;
  const hasSecondarySections =
    subagentSummary.visible.length > 0 ||
    hasGoalOrTimers ||
    backgroundTasks.length > 0 ||
    touchedFiles.length > 0;

  return (
    <>
      {/* 1. Fixed Top Workspace Quick Bar */}
      <div ref={layer1Ref} className="shrink-0 px-2 pt-2.5 pb-1.5 select-none">
        <div className="flex h-[22px] items-center justify-between px-2 pb-0.5">
          <span className="text-ink-tertiary text-[11px] font-medium tracking-[0.04em] uppercase">
            Overview
          </span>
        </div>

        <div className="space-y-0.5">
          <ListRow
            density="compact"
            leading={<FileDiff className="size-[14px] stroke-[1.7]" />}
            onClick={() =>
              orbitActions.openReviewTab({ path: null, scope: toolSummary.reviewScope })
            }
            trailing={
              toolSummary.reviewCount > 0 ? (
                toolSummary.reviewAdditions > 0 || toolSummary.reviewDeletions > 0 ? (
                  <DiffStat
                    additions={toolSummary.reviewAdditions}
                    deletions={toolSummary.reviewDeletions}
                  />
                ) : (
                  <Badge variant="secondary" className="h-[18px] rounded-full px-1.5 font-mono tabular-nums">
                    {toolSummary.reviewCount}
                  </Badge>
                )
              ) : undefined
            }
          >
            Review
          </ListRow>

          <ListRow
            density="compact"
            leading={<File className="size-[14px] stroke-[1.7]" />}
            onClick={() => orbitActions.openFilesExplorerTab()}
          >
            Files
          </ListRow>

          <ListRow
            density="compact"
            leading={<SquareTerminal className="size-[14px] stroke-[1.7]" />}
            onClick={() => orbitActions.openTerminalTab()}
            trailing={
              toolSummary.terminalCount > 0 ? (
                <Badge variant="secondary" className="h-[18px] rounded-full px-1.5 font-mono tabular-nums">
                  {toolSummary.terminalCount}
                </Badge>
              ) : undefined
            }
          >
            Terminal
          </ListRow>

          {toolSummary.artifactCount > 0 && (
            <ListRow
              density="compact"
              leading={<Blocks className="size-[14px] stroke-[1.7]" />}
              onClick={() => orbitActions.openArtifactsTab({ hostId })}
              trailing={
                <Badge variant="secondary" className="h-[18px] rounded-full px-1.5 font-mono tabular-nums">
                  {toolSummary.artifactCount}
                </Badge>
              }
            >
              Artifacts
            </ListRow>
          )}

          {toolSummary.showGroupNotes && toolSummary.workspaceId && (
            <ListRow
              density="compact"
              leading={<FileText className="size-[14px] stroke-[1.7]" />}
              onClick={() =>
                orbitActions.openWorkspaceBoardTab(
                  hostId,
                  toolSummary.workspaceId!,
                  toolSummary.groupTitle ?? undefined,
                )
              }
              trailing={
                toolSummary.groupBoardRevision > 0 ? (
                  <span className="text-ink-muted font-mono text-[11px] tabular-nums">
                    v{toolSummary.groupBoardRevision}
                  </span>
                ) : undefined
              }
            >
              Group notes
            </ListRow>
          )}
        </div>
      </div>

      {/* Shared Scrollable Body for Live Situational Sections */}
      {hasSecondarySections && (
        <div
          style={{ maxHeight: `calc(${cardMaxHeightExpr} - ${layer1HeightPx + 2}px)` }}
          className="orbit-native-scroll min-h-0 flex-1 overflow-y-auto overscroll-contain select-none"
        >
          {/* 2. Living Subagents Crew (OneWorks 3D Avatars) */}
          {subagentSummary.visible.length > 0 && (
            <OverviewSection
              id="subagents"
              label="Subagents"
              count={subagentSummary.visible.length}
              collapsed={collapsedSections.subagents}
              isRunning={subagentSummary.activeCount > 0}
              hasAlert={subagentSummary.awaitingCount > 0}
              headerAction={
                settledSubagents.length > 0 ? (
                  <button
                    type="button"
                    onClick={() => archiveSettledSubagentsList(hostId, agentId, settledSubagents)}
                    className="text-ink-tertiary hover:text-ink-primary cursor-pointer rounded-[6px] px-1.5 py-0.5 text-[11px] font-medium transition-colors"
                  >
                    Archive done
                  </button>
                ) : undefined
              }
            >
              <SubagentCrewList
                hostId={hostId}
                agentId={agentId}
                subagents={subagentSummary.visible}
              />
            </OverviewSection>
          )}

          {/* 3. Active Goal & Armed Timers (Conditional) */}
          {hasGoalOrTimers && (
            <>
              <Separator className="bg-border-subtle" />
              <div className="space-y-0.5 px-2 py-1.5">
                <div className="flex h-[24px] items-center justify-between px-2">
                  <span className="text-ink-tertiary text-[11px] font-medium tracking-[0.03em] uppercase">
                    {activeGoal && hasArmedTimers
                      ? "Goal & Timers"
                      : activeGoal
                        ? "Active Goal"
                        : "Armed Timers"}
                  </span>
                  <span className="text-ink-muted font-mono text-[11px] tabular-nums">
                    {(activeGoal ? 1 : 0) + armedTimers.length}
                  </span>
                </div>

                {activeGoal && <OverviewGoalRow hostId={hostId} goal={activeGoal} />}

                {armedTimers.slice(0, 3).map((timer) => (
                  <ArmedTimerRow
                    key={timer.timer_id}
                    hostId={hostId}
                    timer={timer}
                    nowMs={timerNowMs}
                  />
                ))}

                {armedTimers.length > 3 && (
                  <button
                    type="button"
                    onClick={() => orbitActions.openAutomations()}
                    className="text-ink-tertiary hover:text-ink-primary hover:bg-accent/70 flex h-[24px] w-full cursor-pointer items-center justify-between rounded-[6px] px-2 text-[11px] font-medium transition-colors"
                  >
                    <span>+{armedTimers.length - 3} more timers</span>
                    <ArrowUpRight className="size-3 shrink-0" />
                  </button>
                )}
              </div>
            </>
          )}

          {/* 4. Background Tasks (Conditional) */}
          {backgroundTasks.length > 0 && (
            <OverviewSection
              id="tasks"
              label="Background Tasks"
              count={backgroundTasks.length}
              collapsed={collapsedSections.tasks}
              isRunning={hasRunningBgTask}
              runningTone="emerald"
              footer={
                orderedTasks.length > defaultVisibleTaskCount ? (
                  <div
                    className={cn(
                      showAllTasks
                        ? "bg-surface-raised/95 sticky bottom-0 z-[2] -mx-2 pt-1 pr-2 pb-2 pl-4 backdrop-blur-[6px]"
                        : "pt-0.5 pb-1.5 pl-2",
                    )}
                  >
                    <button
                      type="button"
                      onClick={() => setShowAllTasks((v) => !v)}
                      className="text-ink-tertiary hover:text-ink-primary inline-flex cursor-pointer items-center py-0.5 text-[11px] font-medium transition-colors"
                    >
                      {showAllTasks ? "Show less" : `Show ${hiddenTaskCount} more`}
                    </button>
                  </div>
                ) : undefined
              }
            >
              <div className="space-y-0.5 pb-1">
                {visibleTasks.map((task) => {
                  const cleanPreview = cleanBackgroundOutputPreview(task.output_preview);
                  const hasDetails = Boolean(task.command || cleanPreview || task.log_path);
                  const isExpanded = Boolean(expandedTasks[task.id]);
                  const showStopAction = canStopBackground && task.status === "running";
                  const hasHoverActions = Boolean(task.log_path || showStopAction);
                  return (
                    <div
                      key={task.id}
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
                            setExpandedTasks((prev) => ({ ...prev, [task.id]: !prev[task.id] }));
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

                        <div className="flex shrink-0 items-center pr-1.5">
                          {typeof task.started_at_ms === "number" && (
                            <div
                              className={cn(
                                "pr-0.5",
                                hasHoverActions &&
                                  (isExpanded
                                    ? "hidden"
                                    : "group-hover/bgtask:hidden group-focus-within/bgtask:hidden"),
                              )}
                            >
                              <TaskElapsedLabel startedAtMs={task.started_at_ms} />
                            </div>
                          )}

                          {hasHoverActions && (
                            <div
                              className={cn(
                                "items-center gap-0.5",
                                isExpanded
                                  ? "flex"
                                  : "hidden group-hover/bgtask:flex group-focus-within/bgtask:flex",
                              )}
                            >
                              {task.log_path && (
                                <IconButton
                                  size="xs"
                                  tooltip="Open log"
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
                              )}
                              {showStopAction && (
                                <IconButton
                                  size="xs"
                                  tone="danger"
                                  tooltip="Stop background task"
                                  disabled={stoppingBg}
                                  onClick={() => {
                                    if (stoppingBg) return;
                                    setStoppingBg(true);
                                    void orbitIpc.interruptAgent(hostId, agentId).then(
                                      () => orbitActions.showToast("Stopped background tasks"),
                                      (err) => {
                                        setStoppingBg(false);
                                        orbitActions.showToast(
                                          `Failed to stop background tasks: ${String(err)}`,
                                        );
                                      },
                                    );
                                  }}
                                >
                                  <Square className="size-[9px] fill-current" />
                                </IconButton>
                              )}
                            </div>
                          )}
                        </div>
                      </div>

                      {hasDetails && isExpanded && (
                        <div className="selectable-text space-y-1.5 px-2.5 pt-0.5 pb-2 font-mono text-[11px] leading-relaxed">
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
                })}
              </div>
            </OverviewSection>
          )}

          {/* 5. Touched Files (Conditional, collapsed by default for clean density) */}
          {touchedFiles.length > 0 && (
            <OverviewSection
              id="touched_files"
              label="Touched Files"
              count={touchedFiles.length}
              collapsed={collapsedSections.touched_files}
              trailingMeta={
                toolSummary.touchedAdditions > 0 || toolSummary.touchedDeletions > 0 ? (
                  <DiffStat
                    size="xs"
                    additions={toolSummary.touchedAdditions}
                    deletions={toolSummary.touchedDeletions}
                  />
                ) : undefined
              }
            >
              <div className="space-y-0.5 pb-1">
                {touchedFiles.slice(0, 8).map((file) => {
                  const basename = file.display_path.split("/").pop() || file.display_path;
                  return (
                    <ListRow
                      key={file.path}
                      density="compact"
                      title={file.display_path}
                      onClick={() =>
                        orbitActions.openFileInSidePanel(file.path, { hostId, cwd })
                      }
                      leading={
                        <span
                          className={cn(
                            "size-[6px] rounded-full",
                            file.action === "created"
                              ? "bg-signal-success"
                              : file.action === "edited"
                                ? "bg-signal-info"
                                : "bg-ink-muted/60",
                          )}
                        />
                      }
                      trailing={
                        file.additions > 0 || file.deletions > 0 ? (
                          <DiffStat
                            size="xs"
                            additions={file.additions}
                            deletions={file.deletions}
                          />
                        ) : undefined
                      }
                    >
                      <span className="font-mono text-[12px]">{basename}</span>
                    </ListRow>
                  );
                })}
                {touchedFiles.length > 8 && (
                  <button
                    type="button"
                    onClick={() =>
                      orbitActions.openReviewTab({ path: null, scope: toolSummary.reviewScope })
                    }
                    className="text-ink-tertiary hover:text-ink-primary flex h-[24px] w-full cursor-pointer items-center justify-between rounded-[6px] px-2 text-[11px] font-medium transition-colors"
                  >
                    <span>View all {touchedFiles.length} files</span>
                    <ArrowUpRight className="size-3 shrink-0" />
                  </button>
                )}
              </div>
            </OverviewSection>
          )}
        </div>
      )}
    </>
  );
});

/**
 * Antigravity-inspired 288px Session Overview HUD:
 * - Spacious stage (`canDock = true`): docks cleanly at top-right (left of scrollbar).
 * - Compact/split stage (`canDock = false`): opens on demand via TopBar Overview toggle (`⌘⌥O`),
 *   while TopBar relays living Subagent avatars whenever Overview is hidden.
 */
export const StageOverviewPanel = memo(function StageOverviewPanel({
  canDock,
  hostId,
  agentId,
  cwd,
  insights,
  sessionTurnDiffs,
  currentAgent,
  stageContainerRef,
}: {
  canDock: boolean;
  hostId: string;
  agentId: string;
  cwd: string;
  insights: SessionInsightsView;
  sessionTurnDiffs: readonly DiffFileView[];
  currentAgent: AgentCardView | null;
  stageContainerRef?: RefObject<HTMLDivElement | null>;
}) {
  const popoverOpen = useOrbitSelector((s) => s.overviewPopoverOpen);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const [dockedTopPx, setDockedTopPx] = useState(52);
  const [stageHeightPx, setStageHeightPx] = useState(640);
  const {
    dockedOverviewMounted,
    dockedOverviewVisualOpen,
    dockedOverviewTransform,
    dockedOverviewTransition,
  } = useWorkspaceMotion();

  useLayoutEffect(() => {
    const el = stageContainerRef?.current;
    if (!el) return;
    let rafId = 0;
    const syncRect = () => {
      const rect = el.getBoundingClientRect();
      const nextTop = Math.round(rect.top) + 12;
      const nextHeight = Math.round(rect.height);
      setDockedTopPx((prev) => (prev === nextTop ? prev : nextTop));
      if (nextHeight > 0) {
        setStageHeightPx((prev) => (prev === nextHeight ? prev : nextHeight));
      }
    };
    const scheduleSyncRect = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        syncRect();
      });
    };
    syncRect();
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(scheduleSyncRect) : null;
    ro?.observe(el);
    return () => {
      ro?.disconnect();
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [stageContainerRef, dockedOverviewMounted, popoverOpen]);

  const isPopoverVisible = !canDock && popoverOpen;
  const [popoverMounted, setPopoverMounted] = useState(isPopoverVisible);
  const [popoverVisualOpen, setPopoverVisualOpen] = useState(isPopoverVisible);

  useEffect(() => {
    if (isPopoverVisible) {
      setPopoverMounted(true);
      let raf2 = 0;
      const raf1 = window.requestAnimationFrame(() => {
        raf2 = window.requestAnimationFrame(() => {
          setPopoverVisualOpen(true);
        });
      });
      return () => {
        window.cancelAnimationFrame(raf1);
        if (raf2) window.cancelAnimationFrame(raf2);
      };
    }
    setPopoverVisualOpen(false);
    const timer = window.setTimeout(() => setPopoverMounted(false), 165);
    return () => window.clearTimeout(timer);
  }, [isPopoverVisible]);

  useEffect(() => {
    if (!isPopoverVisible) return;
    const onPointerDown = (ev: MouseEvent) => {
      const target = ev.target as Node | null;
      if (popoverRef.current && target && !popoverRef.current.contains(target)) {
        const el =
          target instanceof Element
            ? target.closest("[data-overview-trigger],[data-slot='dropdown-menu-content']")
            : null;
        if (!el) {
          orbitActions.setOverviewPopoverOpen(false);
        }
      }
    };
    const onKeyDown = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") {
        orbitActions.setOverviewPopoverOpen(false);
      }
    };
    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [isPopoverVisible]);

  useEffect(() => {
    if (canDock && popoverOpen) {
      orbitActions.setOverviewPopoverOpen(false);
    }
  }, [canDock, popoverOpen]);

  const dockedCardMaxHeightExpr = `min(calc(100vh - ${dockedTopPx + 84}px), 620px)`;
  const popoverMaxHeightPx = Math.min(560, Math.max(240, stageHeightPx - 24));
  const popoverCardMaxHeightExpr = `${popoverMaxHeightPx}px`;

  return (
    <>
      {dockedOverviewMounted && (
        <aside
          aria-label="Session overview"
          inert={!dockedOverviewVisualOpen}
          style={{
            top: `${dockedTopPx}px`,
            transform: dockedOverviewTransform,
            opacity: dockedOverviewVisualOpen ? 1 : 0,
            transition: dockedOverviewTransition,
          }}
          className={cn(
            "fixed right-4 z-[15] w-[288px] origin-top-right will-change-[transform,opacity]",
            !dockedOverviewVisualOpen && "pointer-events-none",
          )}
        >
          <Card
            style={{ maxHeight: dockedCardMaxHeightExpr }}
            className="overview-card-surface flex w-full flex-col gap-0 overflow-hidden rounded-[12px] p-0"
          >
            <StageOverviewBody
              hostId={hostId}
              agentId={agentId}
              cwd={cwd}
              insights={insights}
              sessionTurnDiffs={sessionTurnDiffs}
              currentAgent={currentAgent}
              cardMaxHeightExpr={dockedCardMaxHeightExpr}
            />
          </Card>
        </aside>
      )}

      {popoverMounted && (
        <div
          ref={popoverRef}
          role="dialog"
          aria-label="Session overview"
          inert={!popoverVisualOpen}
          style={{ maxHeight: popoverCardMaxHeightExpr }}
          className={cn(
            "absolute top-3 right-4 z-30 w-[288px] max-w-[calc(100%-32px)] origin-top-right will-change-[transform,opacity]",
            popoverVisualOpen
              ? "translate-x-0 translate-y-0 scale-100 opacity-100 transition-[opacity,transform] duration-[190ms] ease-[var(--ease-out-expo)]"
              : "pointer-events-none translate-x-1 -translate-y-1.5 scale-[0.97] opacity-0 transition-[opacity,transform] duration-[145ms] ease-[var(--ease-exit)]",
          )}
        >
          <Card
            style={{ maxHeight: popoverCardMaxHeightExpr }}
            className="overview-card-surface floating-surface flex w-full flex-col gap-0 overflow-hidden rounded-[12px] p-0"
          >
            <StageOverviewBody
              hostId={hostId}
              agentId={agentId}
              cwd={cwd}
              insights={insights}
              sessionTurnDiffs={sessionTurnDiffs}
              currentAgent={currentAgent}
              cardMaxHeightExpr={popoverCardMaxHeightExpr}
            />
          </Card>
        </div>
      )}
    </>
  );
});
