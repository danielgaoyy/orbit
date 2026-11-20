import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import {
  AlertCircle,
  ChevronRight,
  CircleCheck,
  Clock,
  File,
  FileDiff,
  Loader2,
  SquareTerminal,
  type LucideIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Separator } from "@/components/ui/separator";
import type {
  AgentCardView,
  DiffFileView,
  SessionInsightsView,
} from "@/lib/orbit-ipc";
import {
  type OverviewSectionId,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

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
  children,
}: {
  id: OverviewSectionId;
  label: string;
  count: number;
  collapsed: boolean;
  isRunning?: boolean;
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
        <CollapsibleTrigger className="group/trigger hover:bg-accent/65 focus-visible:ring-ring/50 flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 text-left transition-colors outline-none focus-visible:ring-2">
          <span className="flex min-w-0 items-center gap-1.5">
            <span className="text-muted-foreground group-hover/trigger:text-foreground truncate text-[12.5px] font-medium transition-colors">
              {label}
            </span>
            <Badge
              variant="secondary"
              className="text-muted-foreground tabular-nums h-[18px] min-w-[18px] rounded-full border-transparent bg-black/[0.05] px-1.5 py-0 text-[11px] leading-none font-medium dark:bg-white/[0.08]"
            >
              {count}
            </Badge>
          </span>
          <span className="flex shrink-0 items-center gap-1.5">
            {collapsed && isRunning && (
              <Loader2 className="text-muted-foreground size-3 shrink-0 animate-spin" />
            )}
            <ChevronRight className="text-muted-foreground/70 size-3.5 shrink-0 transition-transform duration-[220ms] ease-[var(--ease-panel)] group-data-[panel-open]/trigger:rotate-90" />
          </span>
        </CollapsibleTrigger>

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
}: StageOverviewContentProps) {
  const collapsedSections = useOrbitSelector((s) => s.overviewCollapsedSections);
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const gitReview = useOrbitSelector((s) =>
    cwd ? (s.gitReviews[`${hostId}::${cwd}`] ?? null) : null,
  );
  const terminalTabs = useOrbitSelector((s) => s.terminalTabs);
  const [expandedTasks, setExpandedTasks] = useState<Record<string, boolean>>({});

  const insights = baseInsights;
  const backgroundTasks = insights.background_tasks;

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

  const [showAllSubagents, setShowAllSubagents] = useState(false);
  const [showAllTasks, setShowAllTasks] = useState(false);

  const orderedSubagents = useMemo(() => {
    const running = insights.subagents.filter((s) => s.status === "running");
    const rest = insights.subagents.filter((s) => s.status !== "running");
    return [...running, ...rest];
  }, [insights.subagents]);

  const visibleSubagents = showAllSubagents ? orderedSubagents : orderedSubagents.slice(0, 1);
  const hiddenSubagentCount = Math.max(0, orderedSubagents.length - visibleSubagents.length);

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
        </div>
      </div>

      {/* Layer 2: Subagents (progressive disclosure when present) */}
      {insights.subagents.length > 0 && (
        <OverviewLayerSection
          id="subagents"
          label="Subagents"
          count={insights.subagents.length}
          collapsed={collapsedSections.subagents}
          isRunning={insights.subagents.some((s) => s.status === "running")}
        >
          {visibleSubagents.map((sa) => (
            <Button
              key={sa.id}
              type="button"
              variant="ghost"
              size="sm"
              title={sa.prompt_preview || sa.role}
              onClick={() =>
                orbitActions.openSubagentTab({
                  callId: sa.id,
                  role: sa.role,
                  promptPreview: sa.prompt_preview,
                  childAgentId: sa.child_agent_id,
                  providerSubagentId: sa.provider_subagent_id ?? null,
                  parentAgentId: agentId,
                })
              }
              className="text-foreground/86 hover:bg-accent/65 hover:text-foreground h-[30px] w-full justify-start gap-2 rounded-[8px] px-2 text-[13px] font-normal shadow-none transition-colors"
            >
              {sa.status === "running" ? (
                <Loader2 className="text-muted-foreground size-[14px] shrink-0 animate-spin" />
              ) : sa.status === "completed" ? (
                <CircleCheck className="size-[14px] shrink-0 text-emerald-600/85 dark:text-emerald-400/85" />
              ) : (
                <AlertCircle className="size-[14px] shrink-0 text-rose-500/85" />
              )}
              <span
                className={cn(
                  "truncate",
                  sa.status === "running" && "agent-text-shimmer",
                )}
              >
                {sa.role}
              </span>
            </Button>
          ))}
          {orderedSubagents.length > 1 && (
            <button
              type="button"
              onClick={() => setShowAllSubagents((v) => !v)}
              className="text-muted-foreground hover:text-foreground hover:bg-accent/55 flex h-[26px] w-full cursor-pointer items-center justify-center rounded-[8px] text-[11.5px] font-medium transition-colors"
            >
              {showAllSubagents ? "Show less" : `Show ${hiddenSubagentCount} more`}
            </button>
          )}
        </OverviewLayerSection>
      )}

      {/* Layer 3: Background Tasks (progressive disclosure when active) */}
      {backgroundTasks.length > 0 && (
        <OverviewLayerSection
          id="tasks"
          label="Background Tasks"
          count={backgroundTasks.length}
          collapsed={collapsedSections.tasks}
          isRunning={backgroundTasks.some((t) => t.status === "running")}
        >
          {visibleTasks.map((task) => {
            const hasDetails = Boolean(task.command || task.output_preview || task.log_path);
            const isExpanded = Boolean(expandedTasks[task.id]);
            return (
              <div
                key={task.id}
                className="group/bgtask hover:bg-accent/65 flex w-full flex-col rounded-[8px] transition-colors"
              >
                <button
                  type="button"
                  title={task.command || task.title}
                  onClick={() => {
                    if (!hasDetails) return;
                    setExpandedTasks((prev) => ({ ...prev, [task.id]: !prev[task.id] }));
                  }}
                  className={cn(
                    "flex min-w-0 w-full items-start gap-2 px-2 py-1.5 text-left",
                    hasDetails ? "cursor-pointer" : "cursor-default",
                  )}
                >
                  {task.status === "running" ? (
                    <Loader2 className="text-muted-foreground mt-0.5 size-3.5 shrink-0 animate-spin" />
                  ) : (
                    <Clock className="text-muted-foreground mt-0.5 size-3.5 shrink-0" />
                  )}
                  <div className="min-w-0 flex-1">
                    <div
                      className={cn(
                        "truncate font-mono text-[12.5px]",
                        task.status === "running"
                          ? "agent-text-shimmer"
                          : "text-foreground/85",
                      )}
                    >
                      {task.title}
                    </div>
                    <div className="text-muted-foreground/80 truncate text-[11px]">
                      {task.subtitle}
                    </div>
                  </div>
                  {hasDetails && (
                    <ChevronRight
                      className={cn(
                        "text-muted-foreground/60 mt-0.5 size-3.5 shrink-0 transition-transform",
                        isExpanded && "rotate-90",
                      )}
                    />
                  )}
                </button>
                {hasDetails && isExpanded && (
                  <div className="border-border/60 selectable-text mx-2 mb-2 space-y-1.5 rounded-md border bg-black/[0.03] px-2 py-1.5 font-mono text-[11px] dark:bg-white/[0.04]">
                    {task.command && (
                      <div className="text-foreground/85 break-all whitespace-pre-wrap">
                        $ {task.command}
                      </div>
                    )}
                    {task.log_path && (
                      <div className="text-muted-foreground break-all">
                        Log: {task.log_path}
                      </div>
                    )}
                    {task.output_preview && (
                      <pre className="text-muted-foreground max-h-[120px] overflow-auto break-words whitespace-pre-wrap">
                        {task.output_preview.slice(0, 600)}
                      </pre>
                    )}
                  </div>
                )}
              </div>
            );
          })}
          {orderedTasks.length > 1 && (
            <button
              type="button"
              onClick={() => setShowAllTasks((v) => !v)}
              className="text-muted-foreground hover:text-foreground hover:bg-accent/55 flex h-[26px] w-full cursor-pointer items-center justify-center rounded-[8px] text-[11.5px] font-medium transition-colors"
            >
              {showAllTasks ? "Show less" : `Show ${hiddenTaskCount} more`}
            </button>
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
    dockedOverviewOffsetPx,
    dockedOverviewTransition,
  } = useWorkspaceMotion();

  useLayoutEffect(() => {
    const el = stageContainerRef?.current;
    if (!el) return;
    const syncTop = () => {
      const nextTop = Math.round(el.getBoundingClientRect().top) + 12;
      setDockedTopPx((prev) => (prev === nextTop ? prev : nextTop));
    };
    syncTop();
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(syncTop) : null;
    ro?.observe(el);
    window.addEventListener("resize", syncTop);
    return () => {
      ro?.disconnect();
      window.removeEventListener("resize", syncTop);
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
            transform: dockedOverviewVisualOpen
              ? `translate3d(${dockedOverviewOffsetPx}px, 0, 0) scale(1)`
              : `translate3d(${dockedOverviewOffsetPx}px, 0, 0) scale(0.985)`,
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
