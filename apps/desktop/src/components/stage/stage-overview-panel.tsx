import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import {
  ArrowUpRight,
  ChevronRight,
  Clock,
  File,
  FileDiff,
  FileText,
  Square,
  SquareTerminal,
  type LucideIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { SimpleTooltip } from "@/components/ui/overlays";
import { Separator } from "@/components/ui/separator";
import {
  type AgentCardView,
  type DiffFileView,
  type SessionInsightsView,
  isAgentBackgroundWaiting,
  isAgentForegroundRunning,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { formatDurationCompact } from "@/lib/timeline-coalesce";
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
    <span className="text-muted-foreground/60 tabular-nums shrink-0 font-mono text-[11px]">
      {formatDurationCompact(elapsed)}
    </span>
  );
}

const OverviewActionRow = memo(function OverviewActionRow({
  icon: Icon,
  label,
  trailing,
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  trailing?: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      onClick={onClick}
      className="group/ovrow text-foreground/88 hover:bg-accent/65 hover:text-foreground h-[32px] w-full justify-between gap-2 rounded-[8px] px-2 text-[13px] font-normal shadow-none transition-colors"
    >
      <span className="flex min-w-0 items-center gap-2.5">
        <Icon className="text-muted-foreground group-hover/ovrow:text-foreground/85 size-[15px] shrink-0 stroke-[1.65] transition-colors" />
        <span className="truncate">{label}</span>
      </span>
      {trailing && <span className="shrink-0">{trailing}</span>}
    </Button>
  );
});

const OverviewLayerSection = memo(function OverviewLayerSection({
  id,
  label,
  count,
  collapsed,
  isRunning = false,
  hasAlert = false,
  runningTone = "spinner",
  headerAction,
  children,
}: {
  id: OverviewSectionId;
  label: string;
  count: number;
  collapsed: boolean;
  isRunning?: boolean;
  hasAlert?: boolean;
  runningTone?: "spinner" | "emerald";
  headerAction?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <>
      <Separator className="bg-black/[0.058] dark:bg-white/[0.07]" />
      <Collapsible
        open={!collapsed}
        onOpenChange={() => orbitActions.toggleOverviewSection(id)}
        className="px-2.5 py-2"
      >
        <div className="group/sechead hover:bg-accent/65 flex h-[30px] w-full items-center rounded-[8px] pr-1.5 transition-colors">
          <CollapsibleTrigger className="group/trigger focus-visible:ring-ring/50 flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded-[8px] pr-1 pl-2 text-left outline-none focus-visible:ring-2">
            <span className="text-muted-foreground group-hover/sechead:text-foreground truncate text-[12.5px] font-medium transition-colors">
              {label}
            </span>
            <Badge
              variant="secondary"
              className="text-muted-foreground tabular-nums h-[18px] min-w-[18px] rounded-full border-transparent bg-black/[0.05] px-1.5 py-0 text-[11px] leading-none font-medium dark:bg-white/[0.08]"
            >
              {count}
            </Badge>
          </CollapsibleTrigger>

          <div className="flex shrink-0 items-center gap-1">
            {headerAction && (
              <div className="hidden items-center group-focus-within/sechead:flex group-hover/sechead:flex">
                {headerAction}
              </div>
            )}
            {collapsed && (hasAlert || isRunning) && (
              <span
                className={cn(
                  "flex items-center",
                  headerAction &&
                    "group-focus-within/sechead:hidden group-hover/sechead:hidden",
                )}
              >
                {hasAlert ? (
                  <span className="size-[6px] shrink-0 rounded-full bg-amber-500" />
                ) : runningTone === "emerald" ? (
                  <span className="size-[6px] shrink-0 rounded-full bg-emerald-500/85" />
                ) : (
                  <OrbitSpinner size={13} className="text-foreground/65" />
                )}
              </span>
            )}
            <CollapsibleTrigger
              aria-label={`Toggle ${label}`}
              className="group/trigger text-muted-foreground/70 inline-flex size-[20px] cursor-pointer items-center justify-center rounded-[5px] outline-none"
            >
              <ChevronRight className="size-3.5 shrink-0 transition-transform duration-[220ms] ease-[var(--ease-panel)] group-data-[panel-open]/trigger:rotate-90" />
            </CollapsibleTrigger>
          </div>
        </div>

        <CollapsibleContent>
          <div className="space-y-0.5 pt-1">{children}</div>
        </CollapsibleContent>
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
}

const StageOverviewBody = memo(function StageOverviewBody({
  hostId,
  agentId,
  cwd,
  insights: baseInsights,
  sessionTurnDiffs,
  currentAgent,
}: StageOverviewContentProps) {
  const collapsedSections = useOrbitSelector((s) => s.overviewCollapsedSections);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const hostDetail = hostDetails[hostId] ?? null;
  const gitReview = useOrbitSelector((s) =>
    cwd ? (s.gitReviews[`${hostId}::${cwd}`] ?? null) : null,
  );
  const terminalTabs = useOrbitSelector((s) => s.terminalTabs);
  const [expandedTasks, setExpandedTasks] = useState<Record<string, boolean>>({});
  const [stoppingBg, setStoppingBg] = useState(false);

  const currentWorkspace = useMemo(() => {
    if (!hostDetail || !currentAgent) return null;
    return hostDetail.workspaces.find((w) => w.workspace_id === currentAgent.workspace_id) ?? null;
  }, [hostDetail, currentAgent]);
  const currentBoardRev =
    (currentAgent ? hostDetail?.workspace_boards?.[currentAgent.workspace_id]?.revision : 0) ??
    currentWorkspace?.board_revision ??
    0;
  const showGroupNotesRow = Boolean(
    currentAgent &&
      !currentAgent.parent_agent_id &&
      (currentWorkspace?.is_curated || currentBoardRev > 0),
  );

  const insights = baseInsights;
  const backgroundTasks = insights.background_tasks;
  const hasRunningBgTask = backgroundTasks.some((t) => t.status === "running");
  const canStopBackground =
    hasRunningBgTask &&
    isAgentBackgroundWaiting(currentAgent) &&
    !isAgentForegroundRunning(currentAgent);

  useEffect(() => {
    setStoppingBg(false);
  }, [agentId, backgroundTasks.length]);

  const reviewSummary = useMemo(() => {
    const gitFiles = gitReview?.diff_files ?? [];
    const statusFiles = gitReview?.status?.files ?? [];
    const diffByPath = new Map(gitFiles.map((f) => [f.path, f]));

    if (statusFiles.length > 0 || gitFiles.length > 0) {
      const mergedMap = new Map<
        string,
        { path: string; additions: number; deletions: number }
      >();
      for (const sf of statusFiles) {
        const df = diffByPath.get(sf.path);
        if (!mergedMap.has(sf.path)) {
          mergedMap.set(sf.path, {
            path: sf.path,
            additions: df?.additions ?? 0,
            deletions: df?.deletions ?? 0,
          });
        }
      }
      for (const df of gitFiles) {
        if (!mergedMap.has(df.path)) {
          mergedMap.set(df.path, {
            path: df.path,
            additions: df.additions,
            deletions: df.deletions,
          });
        }
      }
      const allMerged = Array.from(mergedMap.values());
      const additions = allMerged.reduce((acc, f) => acc + f.additions, 0);
      const deletions = allMerged.reduce((acc, f) => acc + f.deletions, 0);
      return {
        scope: "working_tree" as const,
        count: allMerged.length,
        additions,
        deletions,
      };
    }

    if (sessionTurnDiffs.length > 0) {
      const additions = sessionTurnDiffs.reduce((acc, f) => acc + f.additions, 0);
      const deletions = sessionTurnDiffs.reduce((acc, f) => acc + f.deletions, 0);
      return {
        scope: "session_turns" as const,
        count: sessionTurnDiffs.length,
        additions,
        deletions,
      };
    }

    return {
      scope: "working_tree" as const,
      count: 0,
      additions: 0,
      deletions: 0,
    };
  }, [gitReview, sessionTurnDiffs]);

  const terminalCount = useMemo(() => {
    const openForHost = terminalTabs.filter((t) => t.hostId === hostId);
    const openIds = new Set(openForHost.map((t) => t.terminalId).filter(Boolean));
    const detachedCount = (hostDetail?.terminals ?? []).filter(
      (t) => !openIds.has(t.terminal_id),
    ).length;
    return openForHost.length + detachedCount;
  }, [terminalTabs, hostId, hostDetail?.terminals]);

  const [showAllTasks, setShowAllTasks] = useState(false);

  const orderedTasks = useMemo(() => {
    const running = backgroundTasks.filter((t) => t.status === "running");
    const rest = backgroundTasks.filter((t) => t.status !== "running");
    return [...running, ...rest];
  }, [backgroundTasks]);

  const visibleTasks = showAllTasks ? orderedTasks : orderedTasks.slice(0, 1);
  const hiddenTaskCount = Math.max(0, orderedTasks.length - visibleTasks.length);

  return (
    <div className="flex min-h-0 max-h-full flex-1 flex-col overflow-y-auto select-none">
      {/* Layer 1: Primary Overview Shortcuts */}
      <div className="px-2.5 pt-2.5 pb-2">
        <div className="flex h-[24px] items-center px-2 pb-0.5">
          <span className="text-muted-foreground text-[12px] font-medium">Overview</span>
        </div>

        <div className="space-y-0.5">
          <OverviewActionRow
            icon={FileDiff}
            label="Review"
            onClick={() =>
              orbitActions.openReviewTab({ path: null, scope: reviewSummary.scope })
            }
            trailing={
              reviewSummary.count > 0 ? (
                reviewSummary.additions > 0 || reviewSummary.deletions > 0 ? (
                  <span className="tabular-nums inline-flex items-center gap-1 font-mono text-[11px]">
                    {reviewSummary.additions > 0 && (
                      <span className="text-emerald-600 dark:text-emerald-400">
                        +{reviewSummary.additions}
                      </span>
                    )}
                    {reviewSummary.deletions > 0 && (
                      <span className="text-rose-600 dark:text-rose-400">
                        -{reviewSummary.deletions}
                      </span>
                    )}
                  </span>
                ) : (
                  <Badge
                    variant="secondary"
                    className="text-muted-foreground tabular-nums h-[18px] min-w-[18px] rounded-full border-transparent bg-black/[0.05] px-1.5 py-0 text-[11px] leading-none font-medium dark:bg-white/[0.08]"
                  >
                    {reviewSummary.count}
                  </Badge>
                )
              ) : undefined
            }
          />

          <OverviewActionRow
            icon={File}
            label="Files"
            onClick={() => orbitActions.openFilesExplorerTab()}
          />

          <OverviewActionRow
            icon={SquareTerminal}
            label="Terminal"
            onClick={() => orbitActions.openTerminalTab()}
            trailing={
              terminalCount > 0 ? (
                <Badge
                  variant="secondary"
                  className="text-muted-foreground tabular-nums h-[18px] min-w-[18px] rounded-full border-transparent bg-black/[0.05] px-1.5 py-0 text-[11px] leading-none font-medium dark:bg-white/[0.08]"
                >
                  {terminalCount}
                </Badge>
              ) : undefined
            }
          />

          {showGroupNotesRow && currentAgent && (
            <OverviewActionRow
              icon={FileText}
              label="Group notes"
              onClick={() =>
                orbitActions.openWorkspaceBoardTab(
                  hostId,
                  currentAgent.workspace_id,
                  currentWorkspace?.title,
                )
              }
              trailing={
                currentBoardRev > 0 ? (
                  <span className="text-muted-foreground/70 font-mono text-[11px]">
                    v{currentBoardRev}
                  </span>
                ) : undefined
              }
            />
          )}
        </div>
      </div>

      {/* Layer 2: Background Tasks (minimal single-line rows; actions appear quietly on hover) */}
      {backgroundTasks.length > 0 && (
        <OverviewLayerSection
          id="tasks"
          label="Background Tasks"
          count={backgroundTasks.length}
          collapsed={collapsedSections.tasks}
          isRunning={hasRunningBgTask}
          runningTone="emerald"
        >
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
                  isExpanded
                    ? "bg-black/[0.025] dark:bg-white/[0.03]"
                    : "hover:bg-accent/65",
                )}
              >
                <div className="relative flex h-[32px] min-w-0 w-full items-center">
                  <button
                    type="button"
                    aria-expanded={hasDetails ? isExpanded : undefined}
                    onClick={() => {
                      if (!hasDetails) return;
                      setExpandedTasks((prev) => ({ ...prev, [task.id]: !prev[task.id] }));
                    }}
                    className={cn(
                      "flex h-full min-w-0 flex-1 items-center gap-2.5 px-2 text-left",
                      hasDetails ? "cursor-pointer" : "cursor-default",
                    )}
                  >
                    <span className="flex size-[15px] shrink-0 items-center justify-center">
                      {task.status === "running" ? (
                        <span className="size-[6px] rounded-full bg-emerald-500/85" />
                      ) : (
                        <Clock className="text-muted-foreground/75 size-[13.5px] stroke-[1.7]" />
                      )}
                    </span>
                    <span className="text-foreground/86 min-w-0 flex-1 truncate font-mono text-[12px] leading-[18px]">
                      {task.title}
                    </span>
                  </button>

                  {/* Resting: quiet elapsed timer; Hover/Expanded: minimalist icon actions */}
                  <div className="flex shrink-0 items-center pr-1.5">
                    {typeof task.started_at_ms === "number" && (
                      <div
                        className={cn(
                          "pr-0.5 transition-opacity duration-150",
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
                          <SimpleTooltip label="Open log">
                            <button
                              type="button"
                              aria-label="Open task log"
                              onClick={() => {
                                if (task.log_path) {
                                  orbitActions.openFileInSidePanel(task.log_path, {
                                    hostId,
                                    cwd,
                                  });
                                }
                              }}
                              className="text-muted-foreground hover:text-foreground hover:bg-black/[0.06] dark:hover:bg-white/[0.08] inline-flex size-[22px] cursor-pointer items-center justify-center rounded-[6px] transition-colors"
                            >
                              <ArrowUpRight className="size-[13px] stroke-[1.8]" />
                            </button>
                          </SimpleTooltip>
                        )}
                        {showStopAction && (
                          <SimpleTooltip
                            label={
                              backgroundTasks.length > 1
                                ? "Stop background tasks"
                                : "Stop background task"
                            }
                          >
                            <button
                              type="button"
                              disabled={stoppingBg}
                              aria-label="Stop background task"
                              onClick={() => {
                                if (stoppingBg) return;
                                setStoppingBg(true);
                                void orbitIpc.interruptAgent(hostId, agentId).then(
                                  () => {
                                    orbitActions.showToast("Stopped background tasks");
                                  },
                                  (err) => {
                                    setStoppingBg(false);
                                    orbitActions.showToast(
                                      `Failed to stop background tasks: ${String(err)}`,
                                    );
                                  },
                                );
                              }}
                              className={cn(
                                "text-muted-foreground hover:bg-rose-500/12 hover:text-rose-600 dark:hover:text-rose-400 inline-flex size-[22px] cursor-pointer items-center justify-center rounded-[6px] transition-colors",
                                stoppingBg && "pointer-events-none opacity-45",
                              )}
                            >
                              <Square className="size-[9.5px] fill-current" />
                            </button>
                          </SimpleTooltip>
                        )}
                      </div>
                    )}
                  </div>
                </div>

                {hasDetails && isExpanded && (
                  <div className="selectable-text space-y-1.5 px-2.5 pt-0.5 pb-2 font-mono text-[11px] leading-relaxed">
                    {task.command && (
                      <div className="text-foreground/78 break-all whitespace-pre-wrap">
                        <span className="text-muted-foreground/50 select-none">$ </span>
                        {task.command}
                      </div>
                    )}
                    {cleanPreview && (
                      <pre className="text-muted-foreground/75 max-h-[88px] overflow-hidden break-words whitespace-pre-wrap">
                        {cleanPreview.slice(0, 320)}
                      </pre>
                    )}
                    {!task.command && !cleanPreview && task.log_path && (
                      <button
                        type="button"
                        onClick={() => {
                          if (task.log_path) {
                            orbitActions.openFileInSidePanel(task.log_path, { hostId, cwd });
                          }
                        }}
                        className="text-muted-foreground hover:text-foreground inline-flex max-w-full cursor-pointer items-center gap-1 text-left transition-colors"
                      >
                        <span className="truncate">{task.log_path}</span>
                        <ArrowUpRight className="size-3 shrink-0" />
                      </button>
                    )}
                  </div>
                )}
              </div>
            );
          })}
          {orderedTasks.length > 1 && (
            <div className="pt-0.5 pl-2">
              <button
                type="button"
                onClick={() => setShowAllTasks((v) => !v)}
                className="text-muted-foreground/75 hover:text-foreground inline-flex cursor-pointer items-center py-0.5 text-[11.5px] leading-[16px] font-medium transition-colors"
              >
                {showAllTasks ? "Show less" : `Show ${hiddenTaskCount} more`}
              </button>
            </div>
          )}
        </OverviewLayerSection>
      )}
    </div>
  );
});

/**
 * Responsive Floating Overview Card:
 * - Spacious stage (`canDock = true`): floats cleanly in the top-right corner inside the unified
 *   stage so the stage scrollbar remains on the far right edge and the card adapts to content height.
 * - Compact/split stage (`canDock = false`): opens on demand as the same floating card anchored
 *   at the top-right corner of the stage.
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
    const syncTop = () => {
      const nextTop = Math.round(el.getBoundingClientRect().top) + 12;
      setDockedTopPx((prev) => (prev === nextTop ? prev : nextTop));
    };
    const scheduleSyncTop = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        syncTop();
      });
    };
    syncTop();
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(scheduleSyncTop) : null;
    ro?.observe(el);
    return () => {
      ro?.disconnect();
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [stageContainerRef, dockedOverviewMounted]);

  const isPopoverVisible = !canDock && popoverOpen;
  const [popoverMounted, setPopoverMounted] = useState(isPopoverVisible);
  const [popoverVisualOpen, setPopoverVisualOpen] = useState(isPopoverVisible);

  // Symmetric interruptible enter & exit for compact floating popover
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

  // Auto-dismiss floating popover cleanly on outside click or Escape
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

  // Clear popover flag once space opens back up for docked presentation
  useEffect(() => {
    if (canDock && popoverOpen) {
      orbitActions.setOverviewPopoverOpen(false);
    }
  }, [canDock, popoverOpen]);

  return (
    <>
      {/* 1. Spacious Stage: Viewport-Anchored Top-Right Card (pure GPU coordinated, zero settle flash) */}
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
            "fixed right-4 z-[15] w-[312px] origin-top-right will-change-[transform,opacity]",
            !dockedOverviewVisualOpen && "pointer-events-none",
          )}
        >
          <Card
            style={{ maxHeight: `calc(100vh - ${dockedTopPx + 12}px)` }}
            className="overview-card-surface flex h-fit w-full flex-col gap-0 overflow-hidden rounded-[14px] p-0"
          >
            <StageOverviewBody
              hostId={hostId}
              agentId={agentId}
              cwd={cwd}
              insights={insights}
              sessionTurnDiffs={sessionTurnDiffs}
              currentAgent={currentAgent}
            />
          </Card>
        </aside>
      )}

      {/* 2. Compact / Split Stage: On-Demand Top-Right Floating Popover Card */}
      {popoverMounted && (
        <div
          ref={popoverRef}
          role="dialog"
          aria-label="Session overview"
          inert={!popoverVisualOpen}
          className={cn(
            "absolute top-3 right-4 z-30 w-[312px] max-w-[calc(100%-32px)] origin-top-right will-change-[transform,opacity]",
            popoverVisualOpen
              ? "translate-x-0 translate-y-0 scale-100 opacity-100 transition-[opacity,transform] duration-[190ms] ease-[var(--ease-out-expo)]"
              : "pointer-events-none translate-x-1 -translate-y-1.5 scale-[0.97] opacity-0 transition-[opacity,transform] duration-[145ms] ease-[var(--ease-exit)]",
          )}
        >
          <Card className="overview-card-surface floating-surface flex h-fit max-h-[min(560px,calc(100%-24px))] w-full flex-col gap-0 overflow-hidden rounded-[14px] p-0">
            <StageOverviewBody
              hostId={hostId}
              agentId={agentId}
              cwd={cwd}
              insights={insights}
              sessionTurnDiffs={sessionTurnDiffs}
              currentAgent={currentAgent}
            />
          </Card>
        </div>
      )}
    </>
  );
});
