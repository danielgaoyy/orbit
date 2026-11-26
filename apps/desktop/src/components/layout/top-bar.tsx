import { memo, useMemo } from "react";
import {
  Archive,
  ArrowLeftRight,
  Copy,
  Download,
  GitBranch,
  GitFork,
  LayoutList,
  MoreHorizontal,
  Pin,
  PinOff,
  Search,
} from "lucide-react";
import {
  type HostCardView,
  type HostDetailView,
  type SessionBackgroundTaskView,
  type SessionSubagentEntryView,
  isAgentBackgroundWaiting,
  isMacDesktop,
} from "@/lib/orbit-ipc";
import { useI18n } from "@/lib/i18n";
import { isAgentInGitRepo } from "@/lib/projects";
import { useActionShortcutDisplay } from "@/lib/keybindings";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import { usePersonalAgentBriefing } from "@/components/personal-agent/use-personal-agent-briefing";
import { SubagentCrewMiniDock } from "@/components/stage/subagent-crew-dock";
import {
  isDefaultPersonalAgentTitle,
  selectIsSelectedAgentPersonalAgent,
} from "@/state/selectors/personal-agent-selectors";
import { IconButton } from "@/components/ui/icon-button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { Separator } from "@/components/ui/separator";
import {
  hasArmedTimersForAgent,
  hasCrossHostChildAgents,
  hasLocalSubagentRecords,
  selectActiveGoalForAgent,
  selectSessionRuntimeActivity,
  selectSessionSubagents,
} from "@/state/selectors/session-selectors";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_INSIGHT_SUBAGENTS: readonly SessionSubagentEntryView[] = Object.freeze([]);
const EMPTY_INSIGHT_BG_TASKS: readonly SessionBackgroundTaskView[] = Object.freeze([]);
const EMPTY_HOST_DETAILS: Record<string, HostDetailView> = Object.freeze({});
const EMPTY_FLEET_HOSTS: readonly HostCardView[] = Object.freeze([]);

/**
 * Precision 40px Stage Header:
 * - Left: Conversation title + quiet worktree branch chip (when applicable).
 * - Right (strict icon budget, zero duplication with Overview):
 *   1. Conditional SubagentCrewMiniDock (ONLY when Overview is hidden or narrow)
 *   2. Conversation actions: More (`MoreHorizontal`)
 *   3. 1px hairline divider
 *   4. View toggle: Overview (`⌘⌥O`) with subtle semantic indicator dot when hidden
 *      (Side Panel toggle stays stationary at top-right of App shell)
 */
export const TopBar = memo(function TopBar() {
  const { t } = useI18n();
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const overviewVisible = useOrbitSelector((s) =>
    s.stageCanDockOverview ? s.overviewDesktopOpenPref : s.overviewPopoverOpen,
  );
  const isPersonalAgentSession = useOrbitSelector(selectIsSelectedAgentPersonalAgent);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const hasActiveSession =
    workspacePanel === "conversation" &&
    !isDraft &&
    Boolean(selectedHostId) &&
    Boolean(selectedAgentId);

  const activeWorktreeKey = useOrbitSelector((s) => {
    if (
      s.workspacePanel !== "conversation" ||
      s.isDraftNewSession ||
      !s.selectedHostId ||
      !s.selectedAgentId
    ) {
      return "";
    }
    const detail = s.hostDetails[s.selectedHostId];
    const ag = detail?.agents.find((a) => a.agent_id === s.selectedAgentId);
    if (!ag || !detail) return "";
    const ws = detail.workspaces.find((w) => w.workspace_id === ag.workspace_id);
    if (!ws?.is_worktree) return "";
    return `${ws.workspace_id}\0${ws.git_branch ?? ""}\0${ws.cwd}`;
  });

  const activeWorktreeWorkspace = useMemo(() => {
    if (!activeWorktreeKey) return null;
    const [workspace_id, git_branch, cwd] = activeWorktreeKey.split("\0");
    return { workspace_id, git_branch: git_branch || null, cwd };
  }, [activeWorktreeKey]);

  const canForkWorktree = useOrbitSelector((s) => {
    if (
      s.workspacePanel !== "conversation" ||
      s.isDraftNewSession ||
      !s.selectedHostId ||
      !s.selectedAgentId
    ) {
      return false;
    }
    const detail = s.hostDetails[s.selectedHostId];
    const ag = detail?.agents.find((a) => a.agent_id === s.selectedAgentId);
    if (!ag) return false;
    return isAgentInGitRepo(s.selectedHostId, ag, detail, s.sidebarView);
  });

  const isPinned = useOrbitSelector((s) =>
    hasActiveSession && s.selectedHostId && s.selectedAgentId
      ? Boolean(
          s.sidebarView?.pinned_conversations?.some(
            (c) => c.host_id === s.selectedHostId && c.agent.agent_id === s.selectedAgentId,
          ),
        )
      : false,
  );

  const needsTopBarActivity = hasActiveSession && !overviewVisible;
  const { briefing: personalAgentBriefing } = usePersonalAgentBriefing({
    enabled: isPersonalAgentSession && needsTopBarActivity,
    includeActivity: false,
  });

  const insightSubagents = useOrbitSelector((s) =>
    needsTopBarActivity && s.selectedHostId && s.selectedAgentId
      ? (s.stages[`${s.selectedHostId}::${s.selectedAgentId}`]?.insights?.subagents ??
        EMPTY_INSIGHT_SUBAGENTS)
      : EMPTY_INSIGHT_SUBAGENTS,
  );
  const currentHostDetail = useOrbitSelector((s) => {
    if (!needsTopBarActivity || !s.selectedHostId || !s.selectedAgentId) return null;
    const detail = s.hostDetails[s.selectedHostId];
    if (!detail) return null;
    const ag = detail.agents.find((a) => a.agent_id === s.selectedAgentId);
    if (
      hasLocalSubagentRecords(detail, s.selectedHostId, s.selectedAgentId, insightSubagents) ||
      selectActiveGoalForAgent(detail, s.selectedAgentId) !== null ||
      hasArmedTimersForAgent(detail, s.selectedAgentId) ||
      isAgentBackgroundWaiting(ag)
    ) {
      return detail;
    }
    return null;
  });
  const crossHostDetails = useOrbitSelector((s) =>
    needsTopBarActivity &&
    hasCrossHostChildAgents(s.hostDetails, s.selectedHostId, s.selectedAgentId, insightSubagents)
      ? s.hostDetails
      : EMPTY_HOST_DETAILS,
  );
  const crossHostFleetHosts = useOrbitSelector((s) =>
    needsTopBarActivity &&
    hasCrossHostChildAgents(s.hostDetails, s.selectedHostId, s.selectedAgentId, insightSubagents)
      ? s.fleet.hosts
      : EMPTY_FLEET_HOSTS,
  );
  const archivedSubagents = useOrbitSelector((s) => s.archivedSubagents);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const insightBackgroundTasks = useOrbitSelector((s) =>
    needsTopBarActivity && s.selectedHostId && s.selectedAgentId
      ? (s.stages[`${s.selectedHostId}::${s.selectedAgentId}`]?.insights?.background_tasks ??
        EMPTY_INSIGHT_BG_TASKS)
      : EMPTY_INSIGHT_BG_TASKS,
  );

  const scopedHostDetails = useMemo(
    () =>
      crossHostDetails !== EMPTY_HOST_DETAILS
        ? crossHostDetails
        : selectedHostId && currentHostDetail
          ? { [selectedHostId]: currentHostDetail }
          : EMPTY_HOST_DETAILS,
    [crossHostDetails, selectedHostId, currentHostDetail],
  );

  const selectorState = useMemo(
    () => ({
      hostDetails: scopedHostDetails,
      fleet: {
        revision: 0,
        total_pending_approvals: 0,
        hosts: crossHostFleetHosts as HostCardView[],
        projects: [],
      },
      archivedSubagents,
      archivedThreads,
    }),
    [scopedHostDetails, crossHostFleetHosts, archivedSubagents, archivedThreads],
  );

  const subagentSummary = useMemo(
    () =>
      needsTopBarActivity
        ? selectSessionSubagents(selectorState, selectedHostId, selectedAgentId, insightSubagents)
        : null,
    [needsTopBarActivity, selectorState, selectedHostId, selectedAgentId, insightSubagents],
  );

  const runtimeActivity = useMemo(
    () =>
      needsTopBarActivity
        ? selectSessionRuntimeActivity(selectorState, selectedHostId, selectedAgentId, {
            subagents: subagentSummary ?? undefined,
            backgroundTasks: insightBackgroundTasks,
          })
        : null,
    [
      needsTopBarActivity,
      selectorState,
      selectedHostId,
      selectedAgentId,
      subagentSummary,
      insightBackgroundTasks,
    ],
  );

  const overviewIndicatorTone = runtimeActivity?.overviewIndicatorTone ?? "none";
  const overviewIndicatorReason = runtimeActivity?.overviewIndicatorReason ?? "none";

  const {
    topbarUseOpenLeftPad,
    topbarReserveRightToggle,
    topbarPaddingTransition,
    topbarTitleOffsetPx,
    topbarTitleTransition,
    topbarRightOffsetPx,
    topbarRightTransition,
    isAnimating,
  } = useWorkspaceMotion();

  const rawHeaderTitle = useOrbitSelector((s) => {
    if (s.workspacePanel === "automations") return "__AUTOMATIONS__";
    if (s.workspacePanel === "overview") return "__OVERVIEW__";
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) {
      return "__NEW_CONVERSATION__";
    }
    const custom =
      s.threadCustomTitles[threadScopeKey(s.selectedHostId, s.selectedAgentId)];
    if (custom) return custom;
    const detail = s.hostDetails[s.selectedHostId];
    const ag =
      detail?.agents.find((a) => a.agent_id === s.selectedAgentId) ??
      s.stages[`${s.selectedHostId}::${s.selectedAgentId}`]?.agent;
    if (
      selectIsSelectedAgentPersonalAgent(s) &&
      isDefaultPersonalAgentTitle(ag?.title)
    ) {
      return "__PERSONAL_AGENT__";
    }
    return ag?.title || "__NEW_CONVERSATION__";
  });

  const headerTitle =
    rawHeaderTitle === "__AUTOMATIONS__"
      ? t("common.automations")
      : rawHeaderTitle === "__OVERVIEW__" || rawHeaderTitle === "__PERSONAL_AGENT__"
        ? t("sidebar.personalRouter")
        : rawHeaderTitle === "__NEW_CONVERSATION__"
          ? t("common.newConversation")
          : rawHeaderTitle;

  const overviewShortcut = useActionShortcutDisplay("toggle_overview_hud");
  const isMac = isMacDesktop();
  const showBottomBorder =
    workspacePanel === "automations" ||
    workspacePanel === "overview" ||
    (!isDraft && Boolean(selectedAgentId));

  const showTopBarCrewRelay =
    hasActiveSession &&
    !overviewVisible &&
    Boolean(selectedAgentId) &&
    (subagentSummary?.visible.length ?? 0) > 0;

  const briefingNeedsYouCount = isPersonalAgentSession
    ? (personalAgentBriefing?.counts.needs_you ?? 0)
    : 0;
  const briefingRunningCount = isPersonalAgentSession
    ? (personalAgentBriefing?.counts.running ?? 0)
    : 0;

  const indicatorDotTone = overviewVisible
    ? "none"
    : briefingNeedsYouCount > 0 || overviewIndicatorTone === "approval"
      ? "warning"
      : overviewIndicatorTone === "running" || briefingRunningCount > 0
        ? "success"
        : overviewIndicatorTone === "goal"
          ? "info"
          : "none";

  const overviewTooltip = overviewVisible
    ? isPersonalAgentSession
      ? t("briefing.hideBriefing")
      : t("topbar.hideOverview")
    : briefingNeedsYouCount > 0 || overviewIndicatorReason === "approval"
      ? t("topbar.showOverviewApproval")
      : overviewIndicatorReason === "subagent" ||
          (isPersonalAgentSession && briefingRunningCount > 0)
        ? t("topbar.showOverviewSubagent")
        : overviewIndicatorReason === "background_task"
          ? t("topbar.showOverviewBackground")
          : overviewIndicatorReason === "goal"
            ? t("topbar.showOverviewGoal")
            : overviewIndicatorReason === "timer"
              ? t("topbar.showOverviewTimer")
              : isPersonalAgentSession
                ? t("briefing.showBriefing")
                : t("topbar.showOverview");

  return (
    <header
      data-tauri-drag-region
      style={
        topbarPaddingTransition !== "none"
          ? { transition: topbarPaddingTransition }
          : undefined
      }
      className={cn(
        "@container bg-surface-base flex h-(--orbit-titlebar-h) min-w-0 shrink-0 items-center justify-between gap-2 overflow-hidden select-none @[460px]:gap-3",
        showBottomBorder && "border-border-subtle border-b",
        topbarUseOpenLeftPad ? "pl-[16px]" : isMac ? "pl-(--orbit-traffic-pad)" : "pl-[48px]",
        topbarReserveRightToggle ? "pr-[42px]" : "pr-[12px]",
      )}
    >
      {/* Left: Conversation Title + quiet Worktree Branch chip */}
      <div
        className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden"
        data-tauri-drag-region
        style={{
          transform:
            topbarTitleOffsetPx !== 0 ? `translate3d(${topbarTitleOffsetPx}px, 0, 0)` : undefined,
          transition: topbarTitleTransition,
          willChange: isAnimating ? "transform" : undefined,
        }}
      >
        <span
          data-tauri-drag-region
          title={headerTitle}
          className={cn(
            "block min-w-0 truncate text-[13px] leading-[20px] font-medium",
            workspacePanel === "conversation" && isDraft
              ? "text-ink-tertiary"
              : "text-ink-primary",
          )}
        >
          {headerTitle}
        </span>

        {activeWorktreeWorkspace?.git_branch && (
          <span
            data-tauri-drag-region
            title={`Worktree branch: ${activeWorktreeWorkspace.git_branch}`}
            className="text-ink-muted hidden max-w-[160px] shrink-0 items-center gap-1 font-mono text-[11.5px] leading-none @[520px]:inline-flex"
          >
            <GitBranch className="size-3 shrink-0 opacity-70" />
            <span className="truncate">{activeWorktreeWorkspace.git_branch}</span>
          </span>
        )}
      </div>

      {/* Right: Zero-duplication Crew Relay + Conversation Actions | Hairline | Overview Toggle */}
      <div
        style={{
          transform:
            topbarRightOffsetPx !== 0
              ? `translate3d(${topbarRightOffsetPx}px, ${isMac ? "1px" : "0px"}, 0)`
              : isMac
                ? "translate3d(0, 1px, 0)"
                : undefined,
          transition: topbarRightTransition,
          willChange: isAnimating ? "transform" : undefined,
        }}
        className="flex shrink-0 items-center gap-1"
      >
        {hasActiveSession && selectedHostId && selectedAgentId && (
          <>
            {showTopBarCrewRelay && subagentSummary && (
              <>
                <SubagentCrewMiniDock
                  agentId={selectedAgentId}
                  subagents={subagentSummary.visible}
                />
                <span aria-hidden="true" className="bg-border-subtle mx-1 h-[14px] w-px shrink-0" />
              </>
            )}

            <DropdownMenu>
              <SimpleTooltip label={t("topbar.conversationActions")}>
                <DropdownMenuTrigger asChild>
                  <IconButton
                    size="md"
                    aria-label={t("topbar.conversationActions")}
                  >
                    <MoreHorizontal className="size-[15px] stroke-[1.75]" />
                  </IconButton>
                </DropdownMenuTrigger>
              </SimpleTooltip>
              <DropdownMenuContent align="end" sideOffset={6} className="min-w-[204px]">
                <DropdownMenuGroup>
                  <DropdownMenuItem
                    onSelect={() => {
                      window.requestAnimationFrame(() => orbitActions.openStageFind());
                    }}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <Search className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("stage.findInConversation")}</span>
                    </span>
                    <DropdownMenuShortcut>{isMac ? "⌘F" : "Ctrl+F"}</DropdownMenuShortcut>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuItem
                    onSelect={() => {
                      void orbitActions.forkAgent({
                        hostId: selectedHostId,
                        sourceAgentId: selectedAgentId,
                      });
                    }}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <GitFork className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("topbar.forkConversation")}</span>
                    </span>
                  </DropdownMenuItem>
                  {canForkWorktree && (
                    <DropdownMenuItem
                      onSelect={() => {
                        void orbitActions.forkAgent({
                          hostId: selectedHostId,
                          sourceAgentId: selectedAgentId,
                          worktree: true,
                        });
                      }}
                    >
                      <span className="flex items-center gap-2 whitespace-nowrap">
                        <GitBranch className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                        <span>{t("topbar.forkInWorktree")}</span>
                      </span>
                    </DropdownMenuItem>
                  )}
                  {activeWorktreeWorkspace && (
                    <DropdownMenuItem
                      onSelect={() =>
                        orbitActions.openWorktreeHandoffDialog({
                          hostId: selectedHostId,
                          workspaceId: activeWorktreeWorkspace.workspace_id,
                          direction: "worktree_to_local",
                          defaultMode: "apply",
                          sourceCwd: activeWorktreeWorkspace.cwd,
                        })
                      }
                    >
                      <span className="flex items-center gap-2 whitespace-nowrap">
                        <ArrowLeftRight className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                        <span>{t("topbar.applyToLocal")}</span>
                      </span>
                    </DropdownMenuItem>
                  )}
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuItem
                    onSelect={() => {
                      void orbitActions.copyThread(selectedHostId, selectedAgentId);
                    }}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <Copy className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("topbar.copyTranscript")}</span>
                    </span>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() => orbitActions.openExportDialog(selectedHostId, selectedAgentId)}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <Download className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("topbar.export")}</span>
                    </span>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuItem
                    onSelect={() => orbitActions.togglePinThread(selectedHostId, selectedAgentId)}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      {isPinned ? (
                        <PinOff className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      ) : (
                        <Pin className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      )}
                      <span>{isPinned ? t("sidebar.unpin") : t("sidebar.pin")}</span>
                    </span>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() => orbitActions.archiveThread(selectedHostId, selectedAgentId)}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <Archive className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("common.archive")}</span>
                    </span>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
              </DropdownMenuContent>
            </DropdownMenu>

            <Separator orientation="vertical" className="mx-0.5 h-[14px]" />

            <IconButton
              size="md"
              data-overview-trigger
              active={overviewVisible}
              indicatorTone={indicatorDotTone}
              tooltip={overviewTooltip}
              shortcut={overviewShortcut}
              aria-label={
                isPersonalAgentSession
                  ? overviewVisible
                    ? t("briefing.hideBriefing")
                    : t("briefing.showBriefing")
                  : overviewVisible
                    ? t("topbar.hideOverview")
                    : t("topbar.showOverview")
              }
              onClick={() => orbitActions.toggleOverviewPanel()}
            >
              <LayoutList className="size-[15px] stroke-[1.75]" />
            </IconButton>
          </>
        )}
      </div>
    </header>
  );
});
