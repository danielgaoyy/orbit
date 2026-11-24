import { memo, useMemo } from "react";
import {
  ArrowLeftRight,
  Code2,
  Copy,
  Download,
  GitBranch,
  GitFork,
  LayoutList,
  MoreHorizontal,
  Sparkles,
} from "lucide-react";
import {
  type HostCardView,
  type HostDetailView,
  type SessionBackgroundTaskView,
  type SessionSubagentEntryView,
  isMacDesktop,
} from "@/lib/orbit-ipc";
import { useI18n } from "@/lib/i18n";
import { isAgentInGitRepo } from "@/lib/projects";
import { useActionShortcutDisplay } from "@/lib/keybindings";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import { SubagentCrewMiniDock } from "@/components/stage/subagent-crew-dock";
import { StandaloneGroupButton } from "@/components/stage/workspace-peer-strip";
import { IconButton } from "@/components/ui/icon-button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { Separator } from "@/components/ui/separator";
import {
  hasCrossHostChildAgents,
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
 *   2. Conversation actions: Collaborate (`Users`) + More (`MoreHorizontal`)
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
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const hasActiveSession =
    workspacePanel === "conversation" &&
    !isDraft &&
    Boolean(selectedHostId) &&
    Boolean(selectedAgentId);

  const activeWorktreeWorkspace = useOrbitSelector((s) => {
    if (
      s.workspacePanel !== "conversation" ||
      s.isDraftNewSession ||
      !s.selectedHostId ||
      !s.selectedAgentId
    ) {
      return null;
    }
    const detail = s.hostDetails[s.selectedHostId];
    const ag = detail?.agents.find((a) => a.agent_id === s.selectedAgentId);
    if (!ag || !detail) return null;
    const ws = detail.workspaces.find((w) => w.workspace_id === ag.workspace_id);
    return ws?.is_worktree ? ws : null;
  });

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

  const insightSubagents = useOrbitSelector((s) =>
    hasActiveSession && s.selectedHostId && s.selectedAgentId
      ? (s.stages[`${s.selectedHostId}::${s.selectedAgentId}`]?.insights?.subagents ??
        EMPTY_INSIGHT_SUBAGENTS)
      : EMPTY_INSIGHT_SUBAGENTS,
  );
  const currentHostDetail = useOrbitSelector((s) =>
    hasActiveSession && s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const crossHostDetails = useOrbitSelector((s) =>
    hasActiveSession &&
    hasCrossHostChildAgents(s.hostDetails, s.selectedHostId, s.selectedAgentId, insightSubagents)
      ? s.hostDetails
      : EMPTY_HOST_DETAILS,
  );
  const crossHostFleetHosts = useOrbitSelector((s) =>
    hasActiveSession &&
    hasCrossHostChildAgents(s.hostDetails, s.selectedHostId, s.selectedAgentId, insightSubagents)
      ? s.fleet.hosts
      : EMPTY_FLEET_HOSTS,
  );
  const archivedSubagents = useOrbitSelector((s) => s.archivedSubagents);
  const insightBackgroundTasks = useOrbitSelector((s) =>
    hasActiveSession && s.selectedHostId && s.selectedAgentId
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
    }),
    [scopedHostDetails, crossHostFleetHosts, archivedSubagents],
  );

  const subagentSummary = useMemo(
    () =>
      hasActiveSession
        ? selectSessionSubagents(selectorState, selectedHostId, selectedAgentId, insightSubagents)
        : null,
    [hasActiveSession, selectorState, selectedHostId, selectedAgentId, insightSubagents],
  );

  const runtimeActivity = useMemo(
    () =>
      hasActiveSession
        ? selectSessionRuntimeActivity(selectorState, selectedHostId, selectedAgentId, {
            subagents: subagentSummary ?? undefined,
            backgroundTasks: insightBackgroundTasks,
          })
        : null,
    [
      hasActiveSession,
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
    return ag?.title || "__NEW_CONVERSATION__";
  });

  const headerTitle =
    rawHeaderTitle === "__AUTOMATIONS__"
      ? t("common.automations")
      : rawHeaderTitle === "__OVERVIEW__"
        ? t("overview.title")
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

  const indicatorDotTone = overviewVisible
    ? "none"
    : overviewIndicatorTone === "approval"
      ? "warning"
      : overviewIndicatorTone === "running"
        ? "success"
        : overviewIndicatorTone === "goal"
          ? "info"
          : "none";

  const overviewTooltip = overviewVisible
    ? t("topbar.hideOverview")
    : overviewIndicatorReason === "approval"
      ? t("topbar.showOverviewApproval")
      : overviewIndicatorReason === "subagent"
        ? t("topbar.showOverviewSubagent")
        : overviewIndicatorReason === "background_task"
          ? t("topbar.showOverviewBackground")
          : overviewIndicatorReason === "goal"
            ? t("topbar.showOverviewGoal")
            : overviewIndicatorReason === "timer"
              ? t("topbar.showOverviewTimer")
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
        "@container bg-surface-base flex h-[40px] min-w-0 shrink-0 items-center justify-between gap-2 overflow-hidden select-none @[460px]:gap-3",
        showBottomBorder && "border-border-subtle border-b",
        topbarUseOpenLeftPad ? "pl-[16px]" : isMac ? "pl-[114px]" : "pl-[48px]",
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

            <StandaloneGroupButton />

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <IconButton
                  size="md"
                  aria-label={t("topbar.conversationActions")}
                >
                  <MoreHorizontal className="size-[15px] stroke-[1.75]" />
                </IconButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" sideOffset={6} className="min-w-[188px]">
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
                    onSelect={() => orbitActions.openExportDialog(selectedHostId, selectedAgentId)}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <Download className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("topbar.export")}</span>
                    </span>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() => {
                      void orbitActions.openCopyCodeBlocksModal(selectedHostId, selectedAgentId);
                    }}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <Code2 className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("topbar.copyCodeBlock")}</span>
                    </span>
                  </DropdownMenuItem>
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
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuItem
                    onSelect={() => {
                      void orbitActions.summarizeAgent(selectedHostId, selectedAgentId);
                    }}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <Sparkles className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("topbar.summarize")}</span>
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
              aria-label={overviewVisible ? t("topbar.hideOverview") : t("topbar.showOverview")}
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
