import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  Check,
  ChevronRight,
  Clock,
  FileCode2,
  FolderTree,
  GitCompareArrows,
  Globe,
  Layers,
  Loader2,
  MessageCircleQuestion,
  Plus,
  Search,
  SquareTerminal,
  Terminal as TerminalIcon,
  X,
  type LucideIcon,
} from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { SimpleTooltip } from "@/components/ui/overlays";
import { modSymbol } from "@/lib/keybindings";
import type { AgentCardView, TimelineItemView } from "@/lib/orbit-ipc";
import { deriveSessionInsights } from "@/lib/session-insights";
import {
  type TurnFileDiffSummary,
  basenameOfPath,
} from "@/lib/timeline-coalesce";
import {
  type OverviewSectionId,
  type QueuedPromptItem,
  orbitActions,
  sideQuestionScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_QUEUED: readonly QueuedPromptItem[] = [];

function shortDirName(p: string): string {
  if (!p || p === "~") return "~";
  const clean = p.replace(/\/+$/, "");
  return clean.split("/").pop() || clean;
}

const OverviewSection = memo(function OverviewSection({
  id,
  label,
  icon: Icon,
  collapsed,
  meta,
  action,
  pulseDot = false,
  children,
}: {
  id: OverviewSectionId;
  label: string;
  icon: LucideIcon;
  collapsed: boolean;
  meta?: React.ReactNode;
  action?: React.ReactNode;
  pulseDot?: boolean;
  children: React.ReactNode;
}) {
  return (
    <section className="space-y-0.5">
      <div className="group/ovsec flex h-[26px] items-center justify-between gap-1 rounded-md px-1.5 transition-colors hover:bg-black/[0.035] dark:hover:bg-white/[0.045]">
        <button
          type="button"
          onClick={() => orbitActions.toggleOverviewSection(id)}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
        >
          <ChevronRight
            className={cn(
              "text-muted-foreground/75 h-[11px] w-[11px] shrink-0 transition-transform duration-150",
              !collapsed && "rotate-90",
            )}
          />
          <Icon className="text-muted-foreground h-[13px] w-[13px] shrink-0 stroke-[1.75]" />
          <span className="text-foreground/82 truncate text-[12px] font-medium">
            {label}
          </span>
          {pulseDot && (
            <span className="bg-primary h-1.5 w-1.5 shrink-0 animate-pulse rounded-full" />
          )}
        </button>

        <div className="flex shrink-0 items-center gap-1">
          {meta && (
            <span className="text-muted-foreground tabular-nums text-[11px]">{meta}</span>
          )}
          {action}
        </div>
      </div>

      <div className="orbit-collapse-grid" data-open={!collapsed}>
        <div className="orbit-collapse-inner">
          <div className="space-y-0.5 pt-0.5 pl-2">{children}</div>
        </div>
      </div>
    </section>
  );
});

interface StageOverviewContentProps {
  hostId: string;
  agentId: string;
  cwd: string;
  items: readonly TimelineItemView[];
  sessionTurnDiffs: readonly TurnFileDiffSummary[];
  currentAgent: AgentCardView | null;
  onClose: () => void;
  closeTooltip: string;
}

const StageOverviewBody = memo(function StageOverviewBody({
  hostId,
  agentId,
  cwd,
  items,
  sessionTurnDiffs,
  currentAgent,
  onClose,
  closeTooltip,
}: StageOverviewContentProps) {
  const collapsedSections = useOrbitSelector((s) => s.overviewCollapsedSections);
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const gitReview = useOrbitSelector((s) =>
    cwd ? (s.gitReviews[`${hostId}::${cwd}`] ?? null) : null,
  );
  const terminalTabs = useOrbitSelector((s) => s.terminalTabs);
  const queuedPrompts = useOrbitSelector(
    (s) => s.queuedPrompts[`${hostId}::${agentId}`] ?? EMPTY_QUEUED,
  );
  const sideQuestionCount = useOrbitSelector((s) => {
    const scope = sideQuestionScopeKey(s);
    return s.sideQuestionsByScope[scope]?.length ?? 0;
  });

  const mod = modSymbol();

  const insights = useMemo(
    () =>
      deriveSessionInsights({
        items,
        cwd,
        currentAgent,
        hostDetail,
        queuedPrompts,
      }),
    [items, cwd, currentAgent, hostDetail, queuedPrompts],
  );

  const sessionDiffs = sessionTurnDiffs;

  const reviewSummary = useMemo(() => {
    const gitFiles = gitReview?.diff_files ?? [];
    const statusFiles = gitReview?.status?.files ?? [];
    const diffByPath = new Map(gitFiles.map((f) => [f.path, f]));

    if (statusFiles.length > 0 || gitFiles.length > 0) {
      const mergedMap = new Map<
        string,
        { path: string; additions: number; deletions: number; status: string }
      >();
      for (const sf of statusFiles) {
        const df = diffByPath.get(sf.path);
        const existing = mergedMap.get(sf.path);
        if (!existing) {
          mergedMap.set(sf.path, {
            path: sf.path,
            additions: df?.additions ?? 0,
            deletions: df?.deletions ?? 0,
            status: sf.status,
          });
        }
      }
      for (const df of gitFiles) {
        if (!mergedMap.has(df.path)) {
          mergedMap.set(df.path, {
            path: df.path,
            additions: df.additions,
            deletions: df.deletions,
            status: "M",
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
        files: allMerged.slice(0, 5),
      };
    }
    if (sessionDiffs.length > 0) {
      const additions = sessionDiffs.reduce((acc, f) => acc + f.additions, 0);
      const deletions = sessionDiffs.reduce((acc, f) => acc + f.deletions, 0);
      return {
        scope: "session_turns" as const,
        count: sessionDiffs.length,
        additions,
        deletions,
        files: sessionDiffs.slice(0, 5).map((f) => ({
          path: f.path,
          additions: f.additions,
          deletions: f.deletions,
          status: "E",
        })),
      };
    }
    return {
      scope: "working_tree" as const,
      count: 0,
      additions: 0,
      deletions: 0,
      files: [],
    };
  }, [gitReview, sessionDiffs]);

  const hostTerminals = useMemo(() => {
    const openForHost = terminalTabs.filter((t) => t.hostId === hostId);
    const openIds = new Set(openForHost.map((t) => t.terminalId).filter(Boolean));
    const detached = (hostDetail?.terminals ?? []).filter(
      (t) => !openIds.has(t.terminal_id),
    );
    return { openForHost, detached, total: openForHost.length + detached.length };
  }, [terminalTabs, hostId, hostDetail?.terminals]);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden select-none">
      {/* Quiet Header */}
      <div className="flex h-[32px] shrink-0 items-center justify-between px-3 pt-1">
        <span className="text-muted-foreground/85 text-[11px] font-medium tracking-[0.04em] uppercase">
          Overview
        </span>
        <SimpleTooltip label={closeTooltip} shortcut={`${mod}⌥O`}>
          <button
            type="button"
            aria-label={closeTooltip}
            onClick={onClose}
            className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
          >
            <X className="h-[13px] w-[13px] stroke-[1.8]" />
          </button>
        </SimpleTooltip>
      </div>

      <ScrollArea className="min-h-0 flex-1" viewportClassName="space-y-3.5 px-2.5 pt-1.5 pb-4">
        {/* 1. Review (Code Diff) */}
        <OverviewSection
          id="review"
          label="Review"
          icon={GitCompareArrows}
          collapsed={collapsedSections.review}
          meta={
            reviewSummary.count > 0 ? (
              <span className="inline-flex items-center gap-1 font-mono text-[10.5px]">
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
                {reviewSummary.additions === 0 && reviewSummary.deletions === 0 && (
                  <span>{reviewSummary.count}</span>
                )}
              </span>
            ) : (
              <span className="opacity-65">Clean</span>
            )
          }
        >
          {reviewSummary.files.length === 0 ? (
            <button
              type="button"
              onClick={() => orbitActions.openReviewTab({ scope: "working_tree", path: null })}
              className="text-muted-foreground hover:text-foreground hover:bg-accent/55 flex w-full cursor-pointer items-center justify-between rounded-md px-2 py-1 text-left text-[12px] transition-colors"
            >
              <span>Inspect diff…</span>
              <span className="font-mono text-[10.5px] opacity-60">{mod}D</span>
            </button>
          ) : (
            <>
              {reviewSummary.files.map((f) => {
                const Icon = resolveSemanticFileIcon(f.path);
                return (
                  <button
                    key={f.path}
                    type="button"
                    title={f.path}
                    onClick={() =>
                      orbitActions.openReviewTab({
                        path: f.path,
                        scope: reviewSummary.scope,
                      })
                    }
                    className="text-foreground/80 hover:text-foreground hover:bg-accent/55 flex w-full cursor-pointer items-center justify-between gap-1.5 rounded-md px-2 py-1 text-left text-[12px] transition-colors"
                  >
                    <span className="flex min-w-0 items-center gap-1.5">
                      <Icon className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0 stroke-[1.7]" />
                      <span className="truncate">{basenameOfPath(f.path)}</span>
                    </span>
                    <span className="tabular-nums shrink-0 font-mono text-[10.5px]">
                      {f.additions > 0 || f.deletions > 0 ? (
                        <span className="inline-flex items-center gap-1">
                          {f.additions > 0 && (
                            <span className="text-emerald-600 dark:text-emerald-400">
                              +{f.additions}
                            </span>
                          )}
                          {f.deletions > 0 && (
                            <span className="text-rose-600 dark:text-rose-400">
                              -{f.deletions}
                            </span>
                          )}
                        </span>
                      ) : (
                        <span className="text-muted-foreground">{f.status}</span>
                      )}
                    </span>
                  </button>
                );
              })}

              {reviewSummary.count > reviewSummary.files.length && (
                <button
                  type="button"
                  onClick={() =>
                    orbitActions.openReviewTab({ path: null, scope: reviewSummary.scope })
                  }
                  className="text-muted-foreground hover:text-foreground hover:bg-accent/45 w-full cursor-pointer rounded-md px-2 py-1 text-left text-[11.5px] transition-colors"
                >
                  View all {reviewSummary.count} files →
                </button>
              )}
            </>
          )}
        </OverviewSection>

        {/* 2. Files */}
        <OverviewSection
          id="files"
          label="Files"
          icon={FileCode2}
          collapsed={collapsedSections.files}
          meta={insights.touchedFiles.length > 0 ? insights.touchedFiles.length : undefined}
          action={
            <SimpleTooltip label="Quick Open File" shortcut={`${mod}P`}>
              <button
                type="button"
                aria-label="Quick open file"
                onClick={(e) => {
                  e.stopPropagation();
                  orbitActions.setQuickOpenFilesOpen(true);
                }}
                className="text-muted-foreground hover:text-foreground inline-flex h-[20px] w-[20px] cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover/ovsec:opacity-100 hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              >
                <Search className="h-[11.5px] w-[11.5px]" />
              </button>
            </SimpleTooltip>
          }
        >
          {insights.touchedFiles.slice(0, 6).map((file) => {
            const Icon = resolveSemanticFileIcon(file.displayPath);
            return (
              <button
                key={file.displayPath}
                type="button"
                title={file.displayPath}
                onClick={() =>
                  orbitActions.openFileInSidePanel(file.path, { hostId, cwd })
                }
                className="text-foreground/80 hover:text-foreground hover:bg-accent/55 flex w-full cursor-pointer items-center justify-between gap-1.5 rounded-md px-2 py-1 text-left text-[12px] transition-colors"
              >
                <span className="flex min-w-0 items-center gap-1.5">
                  <Icon className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0 stroke-[1.7]" />
                  <span className="truncate">{basenameOfPath(file.displayPath)}</span>
                </span>
                <span className="text-muted-foreground/70 shrink-0 font-mono text-[10px]">
                  {file.action === "created" ? "New" : file.action === "edited" ? "Mod" : "Read"}
                </span>
              </button>
            );
          })}

          <button
            type="button"
            onClick={() => orbitActions.openFilesExplorerTab()}
            className="text-muted-foreground hover:text-foreground hover:bg-accent/55 flex w-full cursor-pointer items-center justify-between rounded-md px-2 py-1 text-left text-[11.5px] transition-colors"
          >
            <span className="flex items-center gap-1.5">
              <FolderTree className="h-[12px] w-[12px] opacity-75" />
              <span>Browse workspace…</span>
            </span>
            <span className="font-mono text-[10.5px] opacity-60">{mod}P</span>
          </button>
        </OverviewSection>

        {/* 3. Terminal */}
        <OverviewSection
          id="terminal"
          label="Terminal"
          icon={SquareTerminal}
          collapsed={collapsedSections.terminal}
          meta={hostTerminals.total > 0 ? hostTerminals.total : undefined}
          action={
            <SimpleTooltip label="New Terminal">
              <button
                type="button"
                aria-label="New terminal"
                onClick={(e) => {
                  e.stopPropagation();
                  orbitActions.openTerminalTab({ spawnIfEmpty: false });
                  orbitActions.spawnNewTerminalTab(hostId, cwd || "~");
                }}
                className="text-muted-foreground hover:text-foreground inline-flex h-[20px] w-[20px] cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover/ovsec:opacity-100 hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              >
                <Plus className="h-[12px] w-[12px]" />
              </button>
            </SimpleTooltip>
          }
        >
          {hostTerminals.total === 0 ? (
            <button
              type="button"
              onClick={() => orbitActions.openTerminalTab()}
              className="text-muted-foreground hover:text-foreground hover:bg-accent/55 flex w-full cursor-pointer items-center justify-between rounded-md px-2 py-1 text-left text-[12px] transition-colors"
            >
              <span>Open shell…</span>
              <span className="font-mono text-[10.5px] opacity-60">{mod}J</span>
            </button>
          ) : (
            <>
              {hostTerminals.openForHost.map((t) => (
                <button
                  key={t.key}
                  type="button"
                  onClick={() => {
                    orbitActions.setActiveTerminalTab(t.key);
                    orbitActions.openTerminalTab({ spawnIfEmpty: false });
                  }}
                  className="text-foreground/80 hover:text-foreground hover:bg-accent/55 flex w-full cursor-pointer items-center justify-between gap-1.5 rounded-md px-2 py-1 text-left text-[12px] transition-colors"
                >
                  <span className="flex min-w-0 items-center gap-1.5">
                    <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-emerald-500/80" />
                    <span className="truncate font-mono text-[11.5px]">
                      {t.title} · {shortDirName(t.cwd)}
                    </span>
                  </span>
                </button>
              ))}
              {hostTerminals.detached.slice(0, 3).map((t) => (
                <button
                  key={t.terminal_id}
                  type="button"
                  onClick={() =>
                    orbitActions.attachExistingTerminal(hostId, t.terminal_id, t.cwd, t.title)
                  }
                  className="text-muted-foreground hover:text-foreground hover:bg-accent/55 flex w-full cursor-pointer items-center justify-between gap-1.5 rounded-md px-2 py-1 text-left text-[12px] transition-colors"
                >
                  <span className="flex min-w-0 items-center gap-1.5">
                    <TerminalIcon className="h-[11.5px] w-[11.5px] shrink-0 opacity-70" />
                    <span className="truncate font-mono text-[11.5px]">
                      {t.title} · {shortDirName(t.cwd)}
                    </span>
                  </span>
                </button>
              ))}
            </>
          )}
        </OverviewSection>

        {/* 4. Background Tasks (Conditional — 0px footprint when idle) */}
        {insights.backgroundTasks.length > 0 && (
          <OverviewSection
            id="tasks"
            label="Background Tasks"
            icon={Clock}
            collapsed={collapsedSections.tasks}
            meta={insights.backgroundTasks.length}
            pulseDot={insights.backgroundTasks.some((t) => t.status === "running")}
          >
            {insights.backgroundTasks.map((task) => {
              const queuedIdx =
                task.kind === "queued"
                  ? queuedPrompts.findIndex((q) => `queue:${q.id}` === task.id)
                  : -1;
              return (
                <div
                  key={task.id}
                  className="group/bgtask text-foreground/82 hover:bg-accent/55 flex w-full items-start gap-1.5 rounded-md px-2 py-1 transition-colors"
                >
                  <button
                    type="button"
                    title={task.command || task.title}
                    onClick={() => {
                      if (task.kind === "shell") {
                        orbitActions.openTerminalTab();
                      }
                    }}
                    className="flex min-w-0 flex-1 cursor-pointer items-start gap-1.5 text-left"
                  >
                    {task.status === "running" ? (
                      <Loader2 className="text-muted-foreground mt-0.5 h-[12px] w-[12px] shrink-0 animate-spin" />
                    ) : (
                      <Clock className="text-muted-foreground mt-0.5 h-[12px] w-[12px] shrink-0" />
                    )}
                    <div className="min-w-0 flex-1">
                      <div
                        className={cn(
                          "truncate font-mono text-[11.5px]",
                          task.status === "running"
                            ? "agent-text-shimmer"
                            : "text-foreground/80",
                        )}
                      >
                        {task.title}
                      </div>
                      <div className="text-muted-foreground/75 truncate text-[10.5px]">
                        {task.subtitle}
                      </div>
                    </div>
                  </button>
                  {queuedIdx >= 0 && (
                    <button
                      type="button"
                      title="Cancel queued prompt"
                      onClick={() => orbitActions.removeQueuedPrompt(queuedIdx)}
                      className="text-muted-foreground hover:text-foreground mt-0.5 inline-flex h-[18px] w-[18px] shrink-0 cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover/bgtask:opacity-100 hover:bg-black/[0.08] dark:hover:bg-white/[0.12]"
                    >
                      <X className="h-[11px] w-[11px]" />
                    </button>
                  )}
                </div>
              );
            })}
          </OverviewSection>
        )}

        {/* 5. Subagents (Conditional — 0px footprint when empty) */}
        {insights.subagents.length > 0 && (
          <OverviewSection
            id="subagents"
            label="Subagents"
            icon={Layers}
            collapsed={collapsedSections.subagents}
            meta={insights.subagents.length}
            pulseDot={insights.subagents.some((s) => s.status === "running")}
          >
            {insights.subagents.map((sa) => (
              <button
                key={sa.id}
                type="button"
                title={sa.promptPreview || sa.role}
                onClick={() =>
                  orbitActions.openSubagentTab({
                    callId: sa.id,
                    role: sa.role,
                    promptPreview: sa.promptPreview,
                    childAgentId: sa.childAgentId,
                  })
                }
                className="text-foreground/82 hover:text-foreground hover:bg-accent/55 flex w-full cursor-pointer items-center justify-between gap-1.5 rounded-md px-2 py-1 text-left text-[12px] transition-colors"
              >
                <span className="flex min-w-0 items-center gap-1.5">
                  {sa.status === "running" ? (
                    <Loader2 className="text-primary h-[11.5px] w-[11.5px] shrink-0 animate-spin" />
                  ) : sa.status === "completed" ? (
                    <Check className="h-[11.5px] w-[11.5px] shrink-0 text-emerald-600 dark:text-emerald-400" />
                  ) : (
                    <AlertTriangle className="h-[11.5px] w-[11.5px] shrink-0 text-rose-500" />
                  )}
                  <span
                    className={cn(
                      "truncate",
                      sa.status === "running" && "agent-text-shimmer",
                    )}
                  >
                    {sa.role}
                  </span>
                </span>
              </button>
            ))}
          </OverviewSection>
        )}

        {/* Subtle Utility Shortcuts: Browser & Side Question (/btw) */}
        <div className="border-border/80 space-y-0.5 border-t pt-2.5">
          <button
            type="button"
            onClick={() => orbitActions.openBrowserTab(insights.detectedLocalUrls[0])}
            className="text-muted-foreground hover:text-foreground hover:bg-accent/55 flex h-[27px] w-full cursor-pointer items-center justify-between gap-2 rounded-md px-2 text-left text-[12px] transition-colors"
          >
            <span className="flex min-w-0 items-center gap-1.5">
              <Globe className="h-[13px] w-[13px] shrink-0 stroke-[1.75]" />
              <span className="truncate">Browser</span>
            </span>
            {insights.detectedLocalUrls.length > 0 && (
              <span className="inline-flex shrink-0 items-center gap-1 text-[10.5px] text-emerald-600 dark:text-emerald-400">
                <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                Live
              </span>
            )}
          </button>

          <button
            type="button"
            onClick={() => orbitActions.openSideQuestionTab()}
            className="text-muted-foreground hover:text-foreground hover:bg-accent/55 flex h-[27px] w-full cursor-pointer items-center justify-between gap-2 rounded-md px-2 text-left text-[12px] transition-colors"
          >
            <span className="flex min-w-0 items-center gap-1.5">
              <MessageCircleQuestion className="h-[13px] w-[13px] shrink-0 stroke-[1.75]" />
              <span className="truncate">Side Question</span>
            </span>
            <span className="tabular-nums font-mono text-[10.5px] opacity-65">
              {sideQuestionCount > 0 ? sideQuestionCount : `${mod};`}
            </span>
          </button>
        </div>
      </ScrollArea>
    </div>
  );
});

/**
 * Responsive In-Stage Overview Panel:
 * - Spacious stage (`canDock = true`): docks cleanly on the right margin with smooth horizontal glide & fade.
 * - Compact/split stage (`canDock = false`): automatically yields horizontal space and opens on demand as a
 *   restrained floating popover anchored at the top-right corner of the stage.
 */
export const StageOverviewPanel = memo(function StageOverviewPanel({
  canDock,
  hostId,
  agentId,
  cwd,
  items,
  sessionTurnDiffs,
  currentAgent,
}: {
  canDock: boolean;
  hostId: string;
  agentId: string;
  cwd: string;
  items: readonly TimelineItemView[];
  sessionTurnDiffs: readonly TurnFileDiffSummary[];
  currentAgent: AgentCardView | null;
}) {
  const desktopPrefOpen = useOrbitSelector((s) => s.overviewDesktopOpenPref);
  const popoverOpen = useOrbitSelector((s) => s.overviewPopoverOpen);
  const popoverRef = useRef<HTMLDivElement | null>(null);

  const isDockedVisible = canDock && desktopPrefOpen;
  const isPopoverVisible = !canDock && popoverOpen;
  const [mountDockedBody, setMountDockedBody] = useState(isDockedVisible);

  // Unmount docked body cleanly after its 195ms glide-out so closed Overview incurs zero streaming cost
  useEffect(() => {
    if (isDockedVisible) {
      setMountDockedBody(true);
      return;
    }
    const timer = window.setTimeout(() => setMountDockedBody(false), 210);
    return () => window.clearTimeout(timer);
  }, [isDockedVisible]);

  // Auto-dismiss floating popover cleanly on outside click or Escape
  useEffect(() => {
    if (!isPopoverVisible) return;
    const onPointerDown = (ev: MouseEvent) => {
      const target = ev.target as Node | null;
      if (popoverRef.current && target && !popoverRef.current.contains(target)) {
        // Ignore clicks on the TopBar overview toggle itself so toggle doesn't immediately reopen
        const el = target instanceof Element ? target.closest("[data-overview-trigger]") : null;
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
      {/* 1. Spacious Stage: Smoothly Gliding Docked Right Rail */}
      <aside
        aria-label="Session overview"
        inert={!isDockedVisible}
        style={{
          width: "232px",
          marginRight: isDockedVisible ? "0px" : "-232px",
        }}
        className={cn(
          "border-border/75 bg-canvas relative flex h-full shrink-0 flex-col overflow-hidden border-l pt-2",
          !isDockedVisible && "pointer-events-none",
          "transition-[margin-right,opacity,transform] duration-[195ms] ease-[cubic-bezier(0.22,1,0.36,1)]",
          isDockedVisible
            ? "translate-x-0 opacity-100"
            : "translate-x-2.5 opacity-0",
        )}
      >
        {mountDockedBody && (
          <StageOverviewBody
            hostId={hostId}
            agentId={agentId}
            cwd={cwd}
            items={items}
            sessionTurnDiffs={sessionTurnDiffs}
            currentAgent={currentAgent}
            onClose={() => orbitActions.toggleOverviewPanel(false)}
            closeTooltip="Hide overview"
          />
        )}
      </aside>

      {/* 2. Compact / Split Stage: Floating Top-Right Popover Overlay */}
      {isPopoverVisible && (
        <div
          ref={popoverRef}
          role="dialog"
          aria-label="Session overview"
          className="bg-elevated/95 border-border floating-surface orbit-overview-popover absolute top-3 right-3 z-30 flex max-h-[min(560px,calc(100%-24px))] w-[248px] flex-col overflow-hidden rounded-2xl border backdrop-blur-md"
        >
          <StageOverviewBody
            hostId={hostId}
            agentId={agentId}
            cwd={cwd}
            items={items}
            sessionTurnDiffs={sessionTurnDiffs}
            currentAgent={currentAgent}
            onClose={() => orbitActions.setOverviewPopoverOpen(false)}
            closeTooltip="Close overview"
          />
        </div>
      )}
    </>
  );
});
