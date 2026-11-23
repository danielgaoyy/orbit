import { memo } from "react";
import {
  ArrowLeftRight,
  Blocks,
  Code2,
  Copy,
  Download,
  FileText,
  GitBranch,
  GitFork,
  MoreHorizontal,
  Sparkles,
} from "lucide-react";
import { isAgentBackgroundWaiting, isMacDesktop } from "@/lib/orbit-ipc";
import { isAgentInGitRepo } from "@/lib/projects";
import { modSymbol } from "@/lib/keybindings";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import { StandaloneGroupButton } from "@/components/stage/workspace-peer-strip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export const TopBar = memo(function TopBar() {
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
  // Primitive return keeps TopBar free from re-renders on unrelated store updates
  const overviewBadgeTone = useOrbitSelector<
    "none" | "approval" | "subagent_running" | "bg_running"
  >((s) => {
    if (
      s.workspacePanel !== "conversation" ||
      s.isDraftNewSession ||
      !s.selectedHostId ||
      !s.selectedAgentId
    ) {
      return "none";
    }
    const parentHostId = s.selectedHostId;
    const parentAgentId = s.selectedAgentId;
    const ag = s.hostDetails[parentHostId]?.agents.find((a) => a.agent_id === parentAgentId);
    const stageInsights = s.stages[`${parentHostId}::${parentAgentId}`]?.insights;

    let hasRunningSubagent = Boolean(
      stageInsights?.subagents?.some((sa) => sa.status === "running"),
    );

    const providerSubs = s.hostDetails[parentHostId]?.provider_subagents?.[parentAgentId];
    if (providerSubs?.some((ps) => ps.status === "running")) {
      hasRunningSubagent = true;
    }

    for (const detail of Object.values(s.hostDetails)) {
      for (const child of detail.agents) {
        if (child.archived_at_rfc3339) continue;
        if (
          child.parent_agent_id === parentAgentId &&
          (!child.parent_host_id || child.parent_host_id === parentHostId)
        ) {
          if (child.is_awaiting_permission) return "approval";
          if (child.is_running || child.has_background_tasks) hasRunningSubagent = true;
        }
      }
    }

    if (hasRunningSubagent) return "subagent_running";

    const hasRunningBgTask =
      isAgentBackgroundWaiting(ag) ||
      Boolean(stageInsights?.background_tasks?.some((t) => t.status === "running"));
    return hasRunningBgTask ? "bg_running" : "none";
  });

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

  // Primitive selector prevents `TopBar` from re-rendering on streaming tokens or background host ticks
  const headerTitle = useOrbitSelector((s) => {
    if (s.workspacePanel === "automations") return "Automations";
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) {
      return "New Conversation";
    }
    const custom =
      s.threadCustomTitles[threadScopeKey(s.selectedHostId, s.selectedAgentId)];
    if (custom) return custom;
    const detail = s.hostDetails[s.selectedHostId];
    const ag =
      detail?.agents.find((a) => a.agent_id === s.selectedAgentId) ??
      s.stages[`${s.selectedHostId}::${s.selectedAgentId}`]?.agent;
    return ag?.title || "New Conversation";
  });

  const workspaceArtifactCount = useOrbitSelector((s) => {
    if (!s.selectedHostId) return 0;
    const detail = s.hostDetails[s.selectedHostId];
    const all = detail?.artifacts ?? [];
    if (all.length === 0) return 0;
    const ag =
      !s.isDraftNewSession && s.selectedAgentId
        ? detail?.agents.find((a) => a.agent_id === s.selectedAgentId)
        : null;
    if (ag?.workspace_id) {
      return all.filter((a) => a.workspace_id === ag.workspace_id).length;
    }
    return all.length;
  });

  const mod = modSymbol();
  const isMac = isMacDesktop();

  const showBottomBorder =
    workspacePanel === "automations" || (!isDraft && Boolean(selectedAgentId));

  return (
    <header
      data-tauri-drag-region
      style={
        topbarPaddingTransition !== "none"
          ? { transition: topbarPaddingTransition }
          : undefined
      }
      className={cn(
        "bg-canvas flex h-[40px] shrink-0 items-center justify-between gap-3 select-none",
        showBottomBorder &&
          "shadow-[inset_0_-1px_0_0_rgba(0,0,0,0.055)] dark:shadow-[inset_0_-1px_0_0_rgba(255,255,255,0.06)]",
        topbarUseOpenLeftPad ? "pl-[16px]" : isMac ? "pl-[114px]" : "pl-[48px]",
        topbarReserveRightToggle ? "pr-[42px]" : "pr-[12px]",
      )}
    >
      {/* Left: Conversation Title (glides smoothly via pure GPU translate3d) */}
      <div
        className="flex min-w-0 items-center"
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
          className={cn(
            "translate-y-[0.5px] truncate text-[13.5px] leading-[20px] font-medium",
            workspacePanel === "conversation" && isDraft
              ? "text-foreground/72"
              : "text-foreground/92",
          )}
        >
          {headerTitle}
        </span>
      </div>

      {/* Right: Stage Overview Toggle (Side Panel toggle is stationary in App shell; GPU-slid on panel open/close) */}
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
            {workspaceArtifactCount > 0 && (
              <SimpleTooltip label="Artifacts">
                <button
                  type="button"
                  aria-label={`Artifacts (${workspaceArtifactCount})`}
                  onClick={() => orbitActions.openArtifactsTab({ hostId: selectedHostId })}
                  className="text-foreground/70 hover:text-foreground inline-flex h-[28px] cursor-pointer items-center gap-1 rounded-[7px] px-2 text-[12px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                >
                  <Blocks className="h-[14px] w-[14px] stroke-[1.75]" />
                  <span className="tabular-nums text-[11px] opacity-80">
                    {workspaceArtifactCount}
                  </span>
                </button>
              </SimpleTooltip>
            )}
            <StandaloneGroupButton />
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  aria-label="Conversation actions"
                  className="text-foreground/65 hover:text-foreground inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                >
                  <MoreHorizontal className="h-[15.5px] w-[15.5px] stroke-[1.75]" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" sideOffset={6} className="min-w-[196px]">
                <DropdownMenuItem
                  onSelect={() => {
                    void orbitActions.forkAgent({
                      hostId: selectedHostId,
                      sourceAgentId: selectedAgentId,
                    });
                  }}
                >
                  <span className="flex items-center gap-2">
                    <GitFork className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                    <span>Fork conversation</span>
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
                    <span className="flex items-center gap-2">
                      <GitBranch className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                      <span>Fork in worktree</span>
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
                    <span className="flex items-center gap-2">
                      <ArrowLeftRight className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                      <span>Apply to Local…</span>
                    </span>
                  </DropdownMenuItem>
                )}
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  onSelect={() => orbitActions.openExportDialog(selectedHostId, selectedAgentId)}
                >
                  <span className="flex items-center gap-2">
                    <Download className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                    <span>Export…</span>
                  </span>
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() => {
                    void orbitActions.openCopyCodeBlocksModal(selectedHostId, selectedAgentId);
                  }}
                >
                  <span className="flex items-center gap-2">
                    <Code2 className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                    <span>Copy code block…</span>
                  </span>
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() => {
                    void orbitActions.copyThread(selectedHostId, selectedAgentId);
                  }}
                >
                  <span className="flex items-center gap-2">
                    <Copy className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                    <span>Copy transcript</span>
                  </span>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  onSelect={() => {
                    void orbitActions.summarizeAgent(selectedHostId, selectedAgentId);
                  }}
                >
                  <span className="flex items-center gap-2">
                    <Sparkles className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
                    <span>Summarize</span>
                  </span>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <SimpleTooltip
            label={
              overviewVisible
                ? "Hide overview"
                : overviewBadgeTone === "approval"
                  ? "Show overview (subagent needs approval)"
                  : overviewBadgeTone === "subagent_running"
                    ? "Show overview (subagent running)"
                    : overviewBadgeTone === "bg_running"
                      ? "Show overview (background task running)"
                      : "Show overview"
            }
            shortcut={`${mod}⌥O`}
          >
            <button
              type="button"
              data-overview-trigger
              aria-label={overviewVisible ? "Hide overview" : "Show overview"}
              onClick={() => orbitActions.toggleOverviewPanel()}
              className={cn(
                "relative inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors",
                overviewVisible
                  ? "bg-black/[0.06] text-foreground dark:bg-white/[0.085]"
                  : "text-foreground/65 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
              )}
            >
              <FileText className="h-[15.5px] w-[15.5px] stroke-[1.75]" />
              {!overviewVisible && overviewBadgeTone !== "none" && (
                <span
                  aria-hidden="true"
                  className={cn(
                    "ring-canvas absolute top-[5px] right-[5px] h-[5px] w-[5px] rounded-full ring-[1.5px]",
                    overviewBadgeTone === "approval" ? "bg-amber-500" : "bg-emerald-500/90",
                  )}
                />
              )}
            </button>
          </SimpleTooltip>
          </>
        )}
      </div>
    </header>
  );
});
