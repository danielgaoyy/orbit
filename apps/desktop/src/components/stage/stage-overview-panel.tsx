import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import {
  ArrowUpRight,
  File,
  FileDiff,
  FileText,
  Square,
  SquareTerminal,
  X,
} from "lucide-react";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import {
  ArmedTimerRow,
  OverviewArtifactRow,
  OverviewBackgroundTaskRow,
  OverviewExpandFooter,
  OverviewGoalRow,
  OverviewLinkRow,
  OverviewPullRequestRow,
  OverviewSection,
  OverviewSideQuestionRow,
} from "@/components/stage/stage-overview-sections";
import {
  SubagentCrewList,
  archiveSettledSubagentsList,
} from "@/components/stage/subagent-crew-dock";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { DiffStat } from "@/components/ui/diff-stat";
import { IconButton } from "@/components/ui/icon-button";
import { ListRow } from "@/components/ui/list-row";
import { SimpleTooltip } from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import {
  type AgentCardView,
  type ArtifactSummary,
  type ConditionalTimerRecord,
  type DiffFileView,
  type HostCardView,
  type HostDetailView,
  type SessionInsightsView,
  type SessionPullRequestView,
  isAgentBackgroundWaiting,
  isAgentForegroundRunning,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  buildOverviewLinkItems,
  doesTouchedFileMatchOpenPath,
  doesUrlMatchOpenBrowserUrl,
  findGitReviewForCwd,
  formatTouchedFilesForOverview,
  hasArmedTimersForAgent,
  hasCrossHostChildAgents,
  hasLocalSubagentRecords,
  mergeSessionPullRequests,
  selectActiveGoalForAgent,
  selectScopedTerminalCount,
  selectSessionSubagents,
  selectWorkspaceArtifactsList,
  selectWorkspaceToolSummary,
  splitFormattedTouchedFiles,
} from "@/state/selectors/session-selectors";
import {
  type SideQuestionEntry,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_HOST_DETAILS: Record<string, HostDetailView> = Object.freeze({});
const EMPTY_FLEET_HOSTS: readonly HostCardView[] = Object.freeze([]);
const EMPTY_TIMERS: readonly ConditionalTimerRecord[] = Object.freeze([]);
const EMPTY_ARTIFACTS: readonly ArtifactSummary[] = Object.freeze([]);
const EMPTY_SIDE_QUESTIONS: readonly SideQuestionEntry[] = Object.freeze([]);
const EMPTY_PRS: readonly SessionPullRequestView[] = Object.freeze([]);

const DEFAULT_VISIBLE_TOUCHED_FILES = 8;
const DEFAULT_VISIBLE_LIST_ITEMS = 3;

interface StageOverviewContentProps {
  hostId: string;
  agentId: string;
  cwd: string;
  insights: SessionInsightsView;
  sessionTurnDiffs: readonly DiffFileView[];
  agentWorkspaceId: string | null;
  agentParentId: string | null;
  agentBackgroundWaiting: boolean;
  agentForegroundRunning: boolean;
}

const StageOverviewBody = memo(function StageOverviewBody({
  hostId,
  agentId,
  cwd,
  insights,
  sessionTurnDiffs,
  agentWorkspaceId,
  agentParentId,
  agentBackgroundWaiting,
  agentForegroundRunning,
}: StageOverviewContentProps) {
  const { t } = useI18n();
  const mod = modSymbol();
  const sessionScopeKey = `${hostId}::${agentId}`;

  const collapsedSections = useOrbitSelector((s) => s.overviewCollapsedSections);
  const resolvedWorkspaceId = useOrbitSelector(
    (s) =>
      agentWorkspaceId ??
      s.hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId)?.workspace_id ??
      null,
  );
  const resolvedParentId = useOrbitSelector(
    (s) =>
      agentParentId ??
      s.hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId)?.parent_agent_id ??
      null,
  );
  const hostArtifacts = useOrbitSelector((s) => {
    const list = s.hostDetails[hostId]?.artifacts;
    return list && list.length > 0 ? list : EMPTY_ARTIFACTS;
  });
  const sideQuestions = useOrbitSelector(
    (s) => s.sideQuestionsByScope[sessionScopeKey] ?? EMPTY_SIDE_QUESTIONS,
  );
  const createdPrs = useOrbitSelector(
    (s) => s.sessionCreatedPrs[sessionScopeKey] ?? EMPTY_PRS,
  );
  const groupBoardRevision = useOrbitSelector((s) => {
    const detail = s.hostDetails[hostId];
    if (!detail || !resolvedWorkspaceId) return 0;
    const ws = detail.workspaces.find((w) => w.workspace_id === resolvedWorkspaceId);
    return detail.workspace_boards?.[resolvedWorkspaceId]?.revision ?? ws?.board_revision ?? 0;
  });
  const showGroupNotes = useOrbitSelector((s) => {
    if (!resolvedWorkspaceId || resolvedParentId) return false;
    const detail = s.hostDetails[hostId];
    if (!detail) return false;
    const ws = detail.workspaces.find((w) => w.workspace_id === resolvedWorkspaceId);
    const rev = detail.workspace_boards?.[resolvedWorkspaceId]?.revision ?? ws?.board_revision ?? 0;
    return Boolean(ws?.is_curated || rev > 0);
  });
  const groupTitle = useOrbitSelector((s) => {
    if (!resolvedWorkspaceId) return null;
    return (
      s.hostDetails[hostId]?.workspaces.find((w) => w.workspace_id === resolvedWorkspaceId)?.title ??
      null
    );
  });
  const activeGoal = useOrbitSelector((s) =>
    selectActiveGoalForAgent(s.hostDetails[hostId], agentId),
  );
  const hostTimersForAgent = useOrbitSelector((s) =>
    hasArmedTimersForAgent(s.hostDetails[hostId], agentId)
      ? (s.hostDetails[hostId]?.conditional_timers ?? EMPTY_TIMERS)
      : EMPTY_TIMERS,
  );
  const localSubagentHostDetail = useOrbitSelector((s) =>
    hasLocalSubagentRecords(s.hostDetails[hostId], hostId, agentId)
      ? (s.hostDetails[hostId] ?? null)
      : null,
  );
  const crossHostDetails = useOrbitSelector((s) =>
    hasCrossHostChildAgents(s.hostDetails, hostId, agentId)
      ? s.hostDetails
      : EMPTY_HOST_DETAILS,
  );
  const crossHostFleetHosts = useOrbitSelector((s) =>
    hasCrossHostChildAgents(s.hostDetails, hostId, agentId)
      ? s.fleet.hosts
      : EMPTY_FLEET_HOSTS,
  );
  const gitReview = useOrbitSelector((s) => findGitReviewForCwd(s.gitReviews, hostId, cwd));
  const terminalCount = useOrbitSelector((s) =>
    selectScopedTerminalCount(s.terminalTabs, s.hostDetails[hostId]?.terminals, hostId, cwd),
  );
  const archivedSubagents = useOrbitSelector((s) => s.archivedSubagents);
  const activeSideTab = useOrbitSelector((s) =>
    s.sidePanelOpen
      ? (s.sidePanelTabs.find((tab) => tab.id === s.activeSidePanelTabId) ?? null)
      : null,
  );

  const [expandedTasks, setExpandedTasks] = useState<Record<string, boolean>>({});
  const [stoppingBg, setStoppingBg] = useState(false);
  const [showAllPrs, setShowAllPrs] = useState(false);
  const [showAllSubagents, setShowAllSubagents] = useState(false);
  const [showAllSideQuestions, setShowAllSideQuestions] = useState(false);
  const [showAllTasks, setShowAllTasks] = useState(false);
  const [showAllArtifacts, setShowAllArtifacts] = useState(false);
  const [showAllLinks, setShowAllLinks] = useState(false);
  const [showAllTouchedFiles, setShowAllTouchedFiles] = useState(false);

  const workspaceArtifacts = useMemo(
    () => selectWorkspaceArtifactsList(hostArtifacts, resolvedWorkspaceId, agentId),
    [hostArtifacts, resolvedWorkspaceId, agentId],
  );

  const scopedSubagentHostDetails = useMemo(
    () =>
      crossHostDetails !== EMPTY_HOST_DETAILS
        ? crossHostDetails
        : localSubagentHostDetail
          ? { [hostId]: localSubagentHostDetail }
          : EMPTY_HOST_DETAILS,
    [crossHostDetails, localSubagentHostDetail, hostId],
  );

  const selectorSlice = useMemo(
    () => ({
      hostDetails: scopedSubagentHostDetails,
      fleet: {
        revision: 0,
        total_pending_approvals: 0,
        hosts: crossHostFleetHosts as HostCardView[],
        projects: [],
      },
      archivedSubagents,
    }),
    [scopedSubagentHostDetails, crossHostFleetHosts, archivedSubagents],
  );

  const toolSummary = useMemo(
    () =>
      selectWorkspaceToolSummary(
        selectorSlice,
        hostId,
        agentId,
        cwd,
        sessionTurnDiffs,
        null,
        insights.touched_files,
        {
          gitReview,
          terminalCount,
          artifactCount: workspaceArtifacts.length,
          showGroupNotes,
          groupBoardRevision,
          groupTitle,
          workspaceId: resolvedWorkspaceId,
        },
      ),
    [
      selectorSlice,
      hostId,
      agentId,
      cwd,
      sessionTurnDiffs,
      insights.touched_files,
      gitReview,
      terminalCount,
      workspaceArtifacts.length,
      showGroupNotes,
      groupBoardRevision,
      groupTitle,
      resolvedWorkspaceId,
    ],
  );

  const subagentSummary = useMemo(
    () => selectSessionSubagents(selectorSlice, hostId, agentId, insights.subagents),
    [selectorSlice, hostId, agentId, insights.subagents],
  );

  const pullRequests = useMemo(
    () => mergeSessionPullRequests(insights.pull_requests, createdPrs),
    [insights.pull_requests, createdPrs],
  );
  const visiblePrs = showAllPrs
    ? pullRequests
    : pullRequests.slice(0, DEFAULT_VISIBLE_LIST_ITEMS);
  const hiddenPrCount = Math.max(0, pullRequests.length - visiblePrs.length);

  const orderedSideQuestions = useMemo(
    () =>
      sideQuestions.length <= 1
        ? sideQuestions
        : [...sideQuestions].sort((a, b) => b.createdAtMs - a.createdAtMs),
    [sideQuestions],
  );
  const hasStreamingSideQuestion = useMemo(
    () => orderedSideQuestions.some((q) => q.status === "streaming"),
    [orderedSideQuestions],
  );
  const visibleSideQuestions = showAllSideQuestions
    ? orderedSideQuestions
    : orderedSideQuestions.slice(0, DEFAULT_VISIBLE_LIST_ITEMS);
  const hiddenSideQuestionCount = Math.max(
    0,
    orderedSideQuestions.length - visibleSideQuestions.length,
  );

  const visibleArtifacts = showAllArtifacts
    ? workspaceArtifacts
    : workspaceArtifacts.slice(0, DEFAULT_VISIBLE_LIST_ITEMS);
  const hiddenArtifactCount = Math.max(0, workspaceArtifacts.length - visibleArtifacts.length);

  const overviewLinks = useMemo(
    () => buildOverviewLinkItems(insights.detected_local_urls, insights.detected_links),
    [insights.detected_local_urls, insights.detected_links],
  );
  const visibleLinks = showAllLinks
    ? overviewLinks
    : overviewLinks.slice(0, DEFAULT_VISIBLE_LIST_ITEMS);
  const hiddenLinkCount = Math.max(0, overviewLinks.length - visibleLinks.length);

  const armedTimers = useMemo(
    () =>
      hostTimersForAgent.length === 0
        ? EMPTY_TIMERS
        : hostTimersForAgent.filter((timer) => timer.agent_id === agentId && timer.state === "armed"),
    [hostTimersForAgent, agentId],
  );

  const backgroundTasks = insights.background_tasks;
  const orderedTasks = useMemo(() => {
    const running = backgroundTasks.filter((task) => task.status === "running");
    const rest = backgroundTasks.filter((task) => task.status !== "running");
    return [...running, ...rest];
  }, [backgroundTasks]);

  const runningTaskCount = useMemo(
    () => orderedTasks.filter((task) => task.status === "running").length,
    [orderedTasks],
  );
  const hasRunningBgTask = runningTaskCount > 0;
  const canStopBackground =
    hasRunningBgTask && agentBackgroundWaiting && !agentForegroundRunning;

  useEffect(() => {
    setStoppingBg(false);
  }, [agentId, runningTaskCount]);

  useEffect(() => {
    setShowAllPrs(false);
    setShowAllSubagents(false);
    setShowAllSideQuestions(false);
    setShowAllTasks(false);
    setShowAllArtifacts(false);
    setShowAllLinks(false);
    setShowAllTouchedFiles(false);
    setExpandedTasks({});
  }, [agentId]);

  const toggleOrOpenSideTab = useCallback((isCurrentlyActive: boolean, openFn: () => void) => {
    if (isCurrentlyActive) {
      orbitActions.toggleSidePanel(false);
      orbitActions.setOverviewPopoverOpen(false);
      return;
    }
    openFn();
  }, []);

  const handleToggleTaskExpand = useCallback((taskId: string) => {
    setExpandedTasks((prev) => ({ ...prev, [taskId]: !prev[taskId] }));
  }, []);

  const hasArmedTimers = armedTimers.length > 0;
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
  const defaultVisibleSubagentCount = Math.max(subagentSummary.activeCount, 5);
  const displayedSubagents = showAllSubagents
    ? subagentSummary.visible
    : subagentSummary.visible.slice(0, defaultVisibleSubagentCount);
  const hiddenSubagentCount = Math.max(0, subagentSummary.visible.length - displayedSubagents.length);

  const formattedTouchedFiles = useMemo(
    () => formatTouchedFilesForOverview(toolSummary.touchedFiles),
    [toolSummary.touchedFiles],
  );
  const { modified: modifiedFiles, read: readFiles } = useMemo(
    () => splitFormattedTouchedFiles(formattedTouchedFiles),
    [formattedTouchedFiles],
  );
  const hasModifiedFiles = modifiedFiles.length > 0;
  const activeFileList = hasModifiedFiles ? modifiedFiles : readFiles;
  const visibleTouchedFiles = showAllTouchedFiles
    ? activeFileList
    : activeFileList.slice(0, DEFAULT_VISIBLE_TOUCHED_FILES);
  const hiddenTouchedFileCount = Math.max(0, activeFileList.length - visibleTouchedFiles.length);

  const hasGoalOrTimers = Boolean(activeGoal) || hasArmedTimers;
  const goalSectionLabel =
    activeGoal && hasArmedTimers
      ? t("overview.goalAndTimers")
      : activeGoal
        ? t("overview.activeGoal")
        : t("overview.armedTimers");
  const goalSectionCount = (activeGoal ? 1 : 0) + armedTimers.length;
  const goalIsActive = Boolean(activeGoal && activeGoal.status !== "paused");
  const goalIsPaused = Boolean(activeGoal && activeGoal.status === "paused");

  const hasSecondarySections =
    pullRequests.length > 0 ||
    subagentSummary.visible.length > 0 ||
    orderedSideQuestions.length > 0 ||
    hasGoalOrTimers ||
    backgroundTasks.length > 0 ||
    workspaceArtifacts.length > 0 ||
    overviewLinks.length > 0 ||
    formattedTouchedFiles.length > 0;

  const isReviewActive = activeSideTab?.kind === "review";
  const isFilesActive = activeSideTab?.kind === "files";
  const isTerminalActive = activeSideTab?.kind === "terminal";
  const isSideQuestionActive = activeSideTab?.kind === "side_question";
  const isBoardActive =
    activeSideTab?.kind === "board" && activeSideTab.workspaceId === toolSummary.workspaceId;

  return (
    <>
      {/* 1. Fixed Top Workspace Quick Bar */}
      <div className="shrink-0 px-2 pt-2 pb-1.5 select-none">
        <div className="flex h-[24px] items-center justify-between pr-0.5 pl-2 pb-0.5">
          <span className="text-ink-tertiary text-[11px] font-medium tracking-[0.04em] uppercase">
            {t("overview.title")}
          </span>
          <IconButton
            size="xs"
            tooltip={t("topbar.hideOverview")}
            shortcut={`${mod}⌥O`}
            aria-label={t("topbar.hideOverview")}
            onClick={() => orbitActions.toggleOverviewPanel()}
            className="text-ink-muted hover:text-ink-primary"
          >
            <X className="size-3 stroke-[1.85]" />
          </IconButton>
        </div>

        <div className="flex flex-col gap-0.5">
          <SimpleTooltip label={t("overview.review")} shortcut={`${mod}D`} side="left">
            <ListRow
              density="compact"
              selected={isReviewActive}
              leading={<FileDiff className="size-[14px] stroke-[1.7]" />}
              onClick={() =>
                toggleOrOpenSideTab(isReviewActive, () =>
                  orbitActions.openReviewTab({ path: null, scope: toolSummary.reviewScope }),
                )
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
              {t("overview.review")}
            </ListRow>
          </SimpleTooltip>

          <ListRow
            density="compact"
            selected={isFilesActive}
            leading={<File className="size-[14px] stroke-[1.7]" />}
            onClick={() =>
              toggleOrOpenSideTab(isFilesActive, () => orbitActions.openFilesExplorerTab())
            }
          >
            {t("overview.files")}
          </ListRow>

          <SimpleTooltip label={t("overview.terminal")} shortcut={`${mod}J`} side="left">
            <ListRow
              density="compact"
              selected={isTerminalActive}
              leading={<SquareTerminal className="size-[14px] stroke-[1.7]" />}
              onClick={() =>
                toggleOrOpenSideTab(isTerminalActive, () => orbitActions.openTerminalTab())
              }
              trailing={
                toolSummary.terminalCount > 0 ? (
                  <Badge variant="secondary" className="h-[18px] rounded-full px-1.5 font-mono tabular-nums">
                    {toolSummary.terminalCount}
                  </Badge>
                ) : undefined
              }
            >
              {t("overview.terminal")}
            </ListRow>
          </SimpleTooltip>

          {toolSummary.showGroupNotes && toolSummary.workspaceId && (
            <ListRow
              density="compact"
              selected={isBoardActive}
              leading={<FileText className="size-[14px] stroke-[1.7]" />}
              onClick={() =>
                toggleOrOpenSideTab(isBoardActive, () =>
                  orbitActions.openWorkspaceBoardTab(
                    hostId,
                    toolSummary.workspaceId!,
                    toolSummary.groupTitle ?? undefined,
                  ),
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
              {t("overview.groupNotes")}
            </ListRow>
          )}
        </div>
      </div>

      {/* Shared Scrollable Body with pinned 1px top divider (Pure Flexbox, zero JS measurement) */}
      {hasSecondarySections && (
        <div className="orbit-native-scroll border-border-subtle min-h-0 shrink overflow-y-auto overscroll-contain border-t select-none">
          {/* === Tier 1: Live Runtime (Subagents -> Goal & Timers -> Background Tasks) === */}

          {/* 1. Subagent crew, first for live work and approvals */}
          {subagentSummary.visible.length > 0 && (
            <OverviewSection
              id="subagents"
              label={t("overview.subagents")}
              count={subagentSummary.visible.length}
              collapsed={collapsedSections.subagents}
              isRunning={subagentSummary.activeCount > 0}
              hasAlert={subagentSummary.awaitingCount > 0}
              headerActionVisibility="hover"
              headerAction={
                settledSubagents.length > 0 ? (
                  <button
                    type="button"
                    onClick={() => archiveSettledSubagentsList(hostId, agentId, settledSubagents)}
                    className="text-ink-tertiary hover:text-ink-primary cursor-pointer rounded-[6px] px-1.5 py-0.5 text-[11px] font-medium transition-colors"
                  >
                    {t("overview.archiveDone")}
                  </button>
                ) : undefined
              }
              footer={
                subagentSummary.visible.length > defaultVisibleSubagentCount ? (
                  <OverviewExpandFooter
                    expanded={showAllSubagents}
                    hiddenCount={hiddenSubagentCount}
                    onToggle={() => setShowAllSubagents((v) => !v)}
                  />
                ) : undefined
              }
            >
              <SubagentCrewList
                hostId={hostId}
                agentId={agentId}
                subagents={displayedSubagents}
              />
            </OverviewSection>
          )}

          {/* 2. Active Goal & Armed Timers */}
          {hasGoalOrTimers && (
            <OverviewSection
              id="goals"
              label={goalSectionLabel}
              count={goalSectionCount}
              collapsed={collapsedSections.goals}
              isRunning={goalIsActive || hasArmedTimers}
              hasAlert={goalIsPaused}
              runningTone={goalIsActive ? "emerald" : "timer"}
            >
              <div className="flex flex-col gap-0.5 pb-1">
                {activeGoal && <OverviewGoalRow hostId={hostId} goal={activeGoal} />}

                {armedTimers.slice(0, 3).map((timer) => (
                  <ArmedTimerRow key={timer.timer_id} hostId={hostId} timer={timer} />
                ))}

                {armedTimers.length > 3 && (
                  <button
                    type="button"
                    onClick={() => orbitActions.openAutomations()}
                    className="text-ink-tertiary hover:text-ink-primary hover:bg-accent/70 flex h-[24px] w-full cursor-pointer items-center justify-between rounded-[6px] px-2 text-[11px] font-medium transition-colors"
                  >
                    <span>{t("overview.moreTimers", { count: armedTimers.length - 3 })}</span>
                    <ArrowUpRight className="size-3 shrink-0" />
                  </button>
                )}
              </div>
            </OverviewSection>
          )}

          {/* 3. Background Tasks */}
          {backgroundTasks.length > 0 && (
            <OverviewSection
              id="tasks"
              label={t("overview.backgroundTasks")}
              count={backgroundTasks.length}
              collapsed={collapsedSections.tasks}
              isRunning={hasRunningBgTask}
              runningTone="emerald"
              headerActionVisibility="always"
              headerAction={
                canStopBackground ? (
                  <SimpleTooltip label={t("overview.stopBackgroundTasksTooltip")} side="top">
                    <button
                      type="button"
                      disabled={stoppingBg}
                      onClick={() => {
                        if (stoppingBg) return;
                        setStoppingBg(true);
                        void orbitIpc.interruptAgent(hostId, agentId).then(
                          () => orbitActions.showToast(t("overview.stoppedBackgroundTasksToast")),
                          (err) => {
                            setStoppingBg(false);
                            orbitActions.showToast(
                              t("overview.stopBackgroundTasksFailedToast", {
                                error: String(err),
                              }),
                            );
                          },
                        );
                      }}
                      className="text-signal-danger/85 hover:text-signal-danger hover:bg-signal-danger-bg inline-flex cursor-pointer items-center gap-1 rounded-[6px] px-1.5 py-0.5 text-[11px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                    >
                      <Square className="size-[8px] fill-current" />
                      <span>{t("overview.stopTasks")}</span>
                    </button>
                  </SimpleTooltip>
                ) : undefined
              }
              footer={
                orderedTasks.length > defaultVisibleTaskCount ? (
                  <OverviewExpandFooter
                    expanded={showAllTasks}
                    hiddenCount={hiddenTaskCount}
                    onToggle={() => setShowAllTasks((v) => !v)}
                  />
                ) : undefined
              }
            >
              <div className="flex flex-col gap-0.5 pb-1">
                {visibleTasks.map((task) => (
                  <OverviewBackgroundTaskRow
                    key={task.id}
                    hostId={hostId}
                    cwd={cwd}
                    task={task}
                    isExpanded={Boolean(expandedTasks[task.id])}
                    onToggleExpand={handleToggleTaskExpand}
                  />
                ))}
              </div>
            </OverviewSection>
          )}

          {/* === Tier 2: Session Deliverables (Pull Requests -> Artifacts -> Links) === */}

          {/* 4. Pull Requests */}
          {pullRequests.length > 0 && (
            <OverviewSection
              id="pull_requests"
              label={t("overview.pullRequests")}
              count={pullRequests.length}
              collapsed={collapsedSections.pull_requests}
              footer={
                pullRequests.length > DEFAULT_VISIBLE_LIST_ITEMS ? (
                  <OverviewExpandFooter
                    expanded={showAllPrs}
                    hiddenCount={hiddenPrCount}
                    onToggle={() => setShowAllPrs((v) => !v)}
                  />
                ) : undefined
              }
            >
              <div className="flex flex-col gap-0.5 pb-1">
                {visiblePrs.map((pr) => (
                  <OverviewPullRequestRow key={pr.url} pr={pr} />
                ))}
              </div>
            </OverviewSection>
          )}

          {/* 5. Artifacts (Direct-access item list for active session) */}
          {workspaceArtifacts.length > 0 && (
            <OverviewSection
              id="artifacts"
              label={t("overview.artifacts")}
              count={workspaceArtifacts.length}
              collapsed={collapsedSections.artifacts}
              footer={
                workspaceArtifacts.length > DEFAULT_VISIBLE_LIST_ITEMS ? (
                  <OverviewExpandFooter
                    expanded={showAllArtifacts}
                    hiddenCount={hiddenArtifactCount}
                    onToggle={() => setShowAllArtifacts((v) => !v)}
                  />
                ) : undefined
              }
            >
              <div className="flex flex-col gap-0.5 pb-1">
                {visibleArtifacts.map((artifact) => (
                  <OverviewArtifactRow
                    key={artifact.artifact_id}
                    hostId={hostId}
                    artifact={artifact}
                    selected={
                      activeSideTab?.kind === "artifacts" &&
                      activeSideTab.artifactId === artifact.artifact_id
                    }
                  />
                ))}
              </div>
            </OverviewSection>
          )}

          {/* 6. Links (Local preview URLs -> built-in Browser, external links -> system browser) */}
          {overviewLinks.length > 0 && (
            <OverviewSection
              id="links"
              label={t("overview.links")}
              count={overviewLinks.length}
              collapsed={collapsedSections.links}
              footer={
                overviewLinks.length > DEFAULT_VISIBLE_LIST_ITEMS ? (
                  <OverviewExpandFooter
                    expanded={showAllLinks}
                    hiddenCount={hiddenLinkCount}
                    onToggle={() => setShowAllLinks((v) => !v)}
                  />
                ) : undefined
              }
            >
              <div className="flex flex-col gap-0.5 pb-1">
                {visibleLinks.map((item) => (
                  <OverviewLinkRow
                    key={`${item.kind}:${item.url}`}
                    item={item}
                    selected={
                      item.kind === "local" &&
                      activeSideTab?.kind === "browser" &&
                      doesUrlMatchOpenBrowserUrl(activeSideTab.url, item.url)
                    }
                  />
                ))}
              </div>
            </OverviewSection>
          )}

          {/* === Tier 3: Secondary Context (Side Questions -> Modified Files) === */}

          {/* 7. Side Questions (/btw) */}
          {orderedSideQuestions.length > 0 && (
            <OverviewSection
              id="side_questions"
              label={t("overview.sideQuestions")}
              count={orderedSideQuestions.length}
              collapsed={collapsedSections.side_questions}
              isRunning={hasStreamingSideQuestion}
              footer={
                orderedSideQuestions.length > DEFAULT_VISIBLE_LIST_ITEMS ? (
                  <OverviewExpandFooter
                    expanded={showAllSideQuestions}
                    hiddenCount={hiddenSideQuestionCount}
                    onToggle={() => setShowAllSideQuestions((v) => !v)}
                  />
                ) : undefined
              }
            >
              <div className="flex flex-col gap-0.5 pb-1">
                {visibleSideQuestions.map((entry) => (
                  <OverviewSideQuestionRow
                    key={entry.id}
                    entry={entry}
                    selected={
                      isSideQuestionActive && activeSideTab?.sideQuestionEntryId === entry.id
                    }
                  />
                ))}
              </div>
            </OverviewSection>
          )}

          {/* 8. Modified Files (or Touched Files for read-only research sessions; collapsed by default) */}
          {formattedTouchedFiles.length > 0 && (
            <OverviewSection
              id="touched_files"
              label={hasModifiedFiles ? t("overview.modifiedFiles") : t("overview.touchedFiles")}
              count={activeFileList.length}
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
              footer={
                activeFileList.length > DEFAULT_VISIBLE_TOUCHED_FILES ? (
                  <OverviewExpandFooter
                    expanded={showAllTouchedFiles}
                    hiddenCount={hiddenTouchedFileCount}
                    onToggle={() => setShowAllTouchedFiles((v) => !v)}
                  />
                ) : undefined
              }
            >
              <div className="flex flex-col gap-0.5 pb-1">
                {visibleTouchedFiles.map(({ file, basename, dirHint }) => {
                  const isModified = file.action === "created" || file.action === "edited";
                  const actionLabel =
                    file.action === "created"
                      ? t("overview.fileActionCreated")
                      : file.action === "edited"
                        ? t("overview.fileActionEdited")
                        : t("overview.fileActionRead");

                  return (
                    <div key={file.path} className="group/tfile relative">
                      <ListRow
                        density="compact"
                        selected={
                          activeSideTab?.kind === "file" &&
                          doesTouchedFileMatchOpenPath(activeSideTab.filePath, file, cwd)
                        }
                        title={`${actionLabel} · ${file.display_path}`}
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
                            <span
                              className={cn(
                                isModified &&
                                  "transition-opacity duration-150 group-focus-within/tfile:opacity-0 group-hover/tfile:opacity-0",
                              )}
                            >
                              <DiffStat
                                size="xs"
                                additions={file.additions}
                                deletions={file.deletions}
                              />
                            </span>
                          ) : undefined
                        }
                      >
                        <span className="flex min-w-0 items-baseline gap-1.5">
                          <span className="min-w-0 truncate font-mono text-[12px]">{basename}</span>
                          {dirHint && (
                            <span className="text-ink-faint max-w-[44%] shrink-0 truncate font-mono text-[10.5px] font-normal">
                              {dirHint}
                            </span>
                          )}
                        </span>
                      </ListRow>

                      {isModified && (
                        <div className="bg-[var(--overview-surface,var(--surface-raised))]/95 border-border-subtle pointer-events-none absolute top-1/2 right-1.5 flex -translate-y-1/2 items-center rounded-[7px] border p-0.5 opacity-0 shadow-xs backdrop-blur-[6px] transition-opacity duration-150 group-focus-within/tfile:pointer-events-auto group-focus-within/tfile:opacity-100 group-hover/tfile:pointer-events-auto group-hover/tfile:opacity-100">
                          <IconButton
                            size="xs"
                            tooltip={t("overview.viewFileDiff")}
                            onClick={() =>
                              orbitActions.openReviewTab({
                                path: file.display_path,
                                scope: toolSummary.reviewScope,
                              })
                            }
                          >
                            <FileDiff className="size-3" />
                          </IconButton>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </OverviewSection>
          )}
        </div>
      )}
    </>
  );
});

/**
 * Isolated Docked Shell so `useWorkspaceMotion()` phase transitions (e.g. Left Sidebar toggling)
 * animate the outer GPU transform without re-reconciling the memoized `StageOverviewBody` child.
 */
const DockedOverviewShell = memo(function DockedOverviewShell({
  ariaLabel,
  dockedTopPx,
  children,
}: {
  ariaLabel: string;
  dockedTopPx: number;
  children: ReactNode;
}) {
  const {
    dockedOverviewMounted,
    dockedOverviewVisualOpen,
    dockedOverviewTransform,
    dockedOverviewTransition,
  } = useWorkspaceMotion();

  if (!dockedOverviewMounted) return null;

  const responsiveMaxHeight = `min(640px, max(160px, calc(100dvh - ${dockedTopPx + 16}px)))`;

  return (
    <aside
      aria-label={ariaLabel}
      inert={!dockedOverviewVisualOpen}
      style={{
        top: `${dockedTopPx}px`,
        maxHeight: responsiveMaxHeight,
        transform: dockedOverviewTransform,
        opacity: dockedOverviewVisualOpen ? 1 : 0,
        transition: dockedOverviewTransition,
      }}
      className={cn(
        "fixed right-4 z-[25] flex w-[288px] origin-top-right flex-col will-change-[transform,opacity]",
        !dockedOverviewVisualOpen && "pointer-events-none",
      )}
    >
      <Card className="overview-card-surface flex min-h-0 max-h-full w-full flex-col gap-0 overflow-hidden rounded-[12px] p-0">
        {children}
      </Card>
    </aside>
  );
});

const PopoverOverviewShell = memo(function PopoverOverviewShell({
  canDock,
  ariaLabel,
  children,
}: {
  canDock: boolean;
  ariaLabel: string;
  children: ReactNode;
}) {
  const popoverOpen = useOrbitSelector((s) => s.overviewPopoverOpen);
  const popoverRef = useRef<HTMLDivElement | null>(null);
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
          popoverRef.current?.focus({ preventScroll: true });
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
        ev.stopPropagation();
        orbitActions.setOverviewPopoverOpen(false);
        const trigger = document.querySelector<HTMLElement>("[data-overview-trigger]");
        trigger?.focus({ preventScroll: true });
      }
    };
    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [isPopoverVisible]);

  useEffect(() => {
    if (canDock && popoverOpen) {
      orbitActions.setOverviewPopoverOpen(false);
    }
  }, [canDock, popoverOpen]);

  if (!popoverMounted) return null;

  return (
    <div
      ref={popoverRef}
      tabIndex={-1}
      role="dialog"
      aria-label={ariaLabel}
      inert={!popoverVisualOpen}
      className={cn(
        "absolute top-3 right-4 z-30 flex max-h-[min(560px,calc(100%-24px))] w-[288px] max-w-[calc(100%-32px)] origin-top-right flex-col outline-none will-change-[transform,opacity]",
        popoverVisualOpen
          ? "translate-x-0 translate-y-0 scale-100 opacity-100 transition-[opacity,transform] duration-[190ms] ease-[var(--ease-out-expo)]"
          : "pointer-events-none translate-x-1 -translate-y-1.5 scale-[0.97] opacity-0 transition-[opacity,transform] duration-[145ms] ease-[var(--ease-exit)]",
      )}
    >
      <Card className="floating-surface flex min-h-0 max-h-full w-full flex-col gap-0 overflow-hidden rounded-[12px] border p-0">
        {children}
      </Card>
    </div>
  );
});

interface StageOverviewPanelProps {
  canDock: boolean;
  hostId: string;
  agentId: string;
  cwd: string;
  insights: SessionInsightsView;
  sessionTurnDiffs: readonly DiffFileView[];
  currentAgent: AgentCardView | null;
  stageContainerRef?: RefObject<HTMLDivElement | null>;
}

function areStageOverviewPanelPropsEqual(
  prev: StageOverviewPanelProps,
  next: StageOverviewPanelProps,
): boolean {
  return (
    prev.canDock === next.canDock &&
    prev.hostId === next.hostId &&
    prev.agentId === next.agentId &&
    prev.cwd === next.cwd &&
    prev.insights === next.insights &&
    prev.sessionTurnDiffs === next.sessionTurnDiffs &&
    prev.stageContainerRef === next.stageContainerRef &&
    (prev.currentAgent?.workspace_id ?? null) === (next.currentAgent?.workspace_id ?? null) &&
    (prev.currentAgent?.parent_agent_id ?? null) === (next.currentAgent?.parent_agent_id ?? null) &&
    isAgentBackgroundWaiting(prev.currentAgent) === isAgentBackgroundWaiting(next.currentAgent) &&
    isAgentForegroundRunning(prev.currentAgent) === isAgentForegroundRunning(next.currentAgent)
  );
}

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
}: StageOverviewPanelProps) {
  const { t } = useI18n();
  const [dockedTopPx, setDockedTopPx] = useState(52);

  useLayoutEffect(() => {
    const el = stageContainerRef?.current;
    if (!el) return;
    let rafId = 0;
    const syncRect = () => {
      const nextTop = Math.round(el.getBoundingClientRect().top) + 12;
      setDockedTopPx((prev) => (prev === nextTop ? prev : nextTop));
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
    if (el.parentElement) ro?.observe(el.parentElement);
    window.addEventListener("resize", scheduleSyncRect);
    return () => {
      ro?.disconnect();
      window.removeEventListener("resize", scheduleSyncRect);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [stageContainerRef, canDock]);

  const overviewAriaLabel = t("overview.ariaLabel");

  const agentWorkspaceId = currentAgent?.workspace_id ?? null;
  const agentParentId = currentAgent?.parent_agent_id ?? null;
  const agentBackgroundWaiting = isAgentBackgroundWaiting(currentAgent);
  const agentForegroundRunning = isAgentForegroundRunning(currentAgent);

  const bodyNode = useMemo(
    () => (
      <StageOverviewBody
        hostId={hostId}
        agentId={agentId}
        cwd={cwd}
        insights={insights}
        sessionTurnDiffs={sessionTurnDiffs}
        agentWorkspaceId={agentWorkspaceId}
        agentParentId={agentParentId}
        agentBackgroundWaiting={agentBackgroundWaiting}
        agentForegroundRunning={agentForegroundRunning}
      />
    ),
    [
      hostId,
      agentId,
      cwd,
      insights,
      sessionTurnDiffs,
      agentWorkspaceId,
      agentParentId,
      agentBackgroundWaiting,
      agentForegroundRunning,
    ],
  );

  return (
    <>
      <DockedOverviewShell
        ariaLabel={overviewAriaLabel}
        dockedTopPx={dockedTopPx}
      >
        {bodyNode}
      </DockedOverviewShell>

      <PopoverOverviewShell
        canDock={canDock}
        ariaLabel={overviewAriaLabel}
      >
        {bodyNode}
      </PopoverOverviewShell>
    </>
  );
}, areStageOverviewPanelPropsEqual);
