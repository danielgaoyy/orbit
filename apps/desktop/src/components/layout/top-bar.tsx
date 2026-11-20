import { memo, useState } from "react";
import {
  Check,
  CodeXml,
  Copy,
  FolderOpen,
  GitBranch,
  GitCompareArrows,
  LayoutList,
  PanelRight,
  SquareTerminal,
} from "lucide-react";
import { type ExternalEditorKind, type TimelineItemView, isMacDesktop } from "@/lib/orbit-ipc";
import { modSymbol } from "@/lib/keybindings";
import { countSessionTurnDiffs } from "@/lib/timeline-coalesce";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const timelineDiffCountCache = new WeakMap<TimelineItemView[], number>();

function getCachedSessionDiffCount(items: TimelineItemView[] | undefined): number {
  if (!items || items.length === 0) return 0;
  const cached = timelineDiffCountCache.get(items);
  if (cached !== undefined) return cached;
  const count = countSessionTurnDiffs(items);
  timelineDiffCountCache.set(items, count);
  return count;
}

function basenameOfPath(p: string): string {
  if (!p || p === "~") return "~";
  const trimmed = p.replace(/\/+$/, "");
  const idx = trimmed.lastIndexOf("/");
  return idx >= 0 ? trimmed.slice(idx + 1) || "/" : trimmed;
}

function EditorItemIcon({ kind }: { kind: ExternalEditorKind }) {
  switch (kind) {
    case "terminal":
      return <SquareTerminal className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />;
    case "file_manager":
      return <FolderOpen className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />;
    case "cursor":
    case "zed":
    case "vs_code":
      return <CodeXml className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />;
  }
}

export const TopBar = memo(function TopBar() {
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const sidePanelOpen = useOrbitSelector((s) => s.sidePanelOpen);
  const activeSidePanelTabKind = useOrbitSelector((s) => {
    if (!s.sidePanelOpen || !s.activeSidePanelTabId) return null;
    return s.sidePanelTabs.find((t) => t.id === s.activeSidePanelTabId)?.kind ?? null;
  });
  const overviewVisible = useOrbitSelector((s) =>
    s.stageCanDockOverview ? s.overviewDesktopOpenPref : s.overviewPopoverOpen,
  );
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const hasActiveSession = workspacePanel === "conversation" && !isDraft && Boolean(selectedAgentId);
  const hasRunningBackgroundActivity = useOrbitSelector((s) => {
    if (!s.selectedHostId || !s.selectedAgentId || s.isDraftNewSession) return false;
    const tl = s.timelines[`${s.selectedHostId}::${s.selectedAgentId}`];
    if (!tl) return false;
    for (let i = tl.items.length - 1; i >= 0; i -= 1) {
      const it = tl.items[i];
      if (
        it.item_type === "tool_call" &&
        it.status === "running" &&
        (it.detail.kind === "shell" || it.detail.kind === "subagent")
      ) {
        return true;
      }
    }
    return false;
  });

  // Primitive selectors prevent `TopBar` from re-rendering on streaming tokens or background host ticks
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
      s.timelines[`${s.selectedHostId}::${s.selectedAgentId}`]?.agent;
    return ag?.title || "New Conversation";
  });

  const branchName = useOrbitSelector((s) => {
    if (!s.selectedHostId || !s.selectedWorkspaceCwd) return null;
    const gitBranch =
      s.gitReviews[`${s.selectedHostId}::${s.selectedWorkspaceCwd}`]?.status?.branch;
    if (gitBranch) return gitBranch;
    const detail = s.hostDetails[s.selectedHostId];
    return (
      detail?.workspaces.find((w) => w.cwd === s.selectedWorkspaceCwd)?.git_branch ??
      detail?.discovered_projects.find((p) => p.path === s.selectedWorkspaceCwd)
        ?.git_branch ??
      null
    );
  });

  const changedBadgeCount = useOrbitSelector((s) => {
    const gitCount =
      s.selectedHostId && s.selectedWorkspaceCwd
        ? (s.gitReviews[`${s.selectedHostId}::${s.selectedWorkspaceCwd}`]?.status?.files
            .length ?? 0)
        : 0;
    if (gitCount > 0) return gitCount;
    if (!s.selectedHostId || !s.selectedAgentId || s.isDraftNewSession) return 0;
    const tl = s.timelines[`${s.selectedHostId}::${s.selectedAgentId}`];
    return getCachedSessionDiffCount(tl?.items);
  });

  const [copiedPath, setCopiedPath] = useState(false);
  const mod = modSymbol();
  const isMac = isMacDesktop();

  const showBottomBorder =
    workspacePanel === "automations" || (!isDraft && Boolean(selectedAgentId));

  const handleCopyCwd = async () => {
    if (!selectedCwd) return;
    await navigator.clipboard.writeText(selectedCwd);
    setCopiedPath(true);
    setTimeout(() => setCopiedPath(false), 1400);
  };

  return (
    <header
      data-tauri-drag-region
      className={cn(
        "bg-canvas flex h-[40px] shrink-0 items-center justify-between gap-3 px-4 transition-[padding-left] duration-[190ms] ease-[cubic-bezier(0.22,1,0.36,1)] select-none",
        showBottomBorder &&
          "shadow-[inset_0_-1px_0_0_rgba(0,0,0,0.055)] dark:shadow-[inset_0_-1px_0_0_rgba(255,255,255,0.06)]",
        !sidebarOpen && (isMac ? "pl-[114px]" : "pl-[48px]"),
      )}
    >
      {/* Left: Conversation Title (glides smoothly without mounting/unmounting sibling buttons) */}
      <div className="flex min-w-0 items-center" data-tauri-drag-region>
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

      {/* Right: Semantic Workspace Tool Icons (Open Workspace, Diff Review, Terminal) */}
      <div className={cn("flex shrink-0 items-center gap-1", isMac && "translate-y-[1px]")}>
        <DropdownMenu>
          <SimpleTooltip label="Open workspace" shortcut={`${mod}O`}>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                aria-label="Open workspace"
                className="text-foreground/65 hover:text-foreground inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
              >
                <FolderOpen className="h-[16px] w-[16px] stroke-[1.75]" />
              </button>
            </DropdownMenuTrigger>
          </SimpleTooltip>

          <DropdownMenuContent align="end" className="w-56">
            {selectedCwd && (
              <>
                <div className="space-y-0.5 px-2.5 py-1.5">
                  <div className="text-foreground truncate text-[13px] font-medium">
                    {basenameOfPath(selectedCwd)}
                  </div>
                  <div className="text-muted-foreground flex items-center gap-1.5 truncate font-mono text-[12px]">
                    {branchName ? (
                      <>
                        <GitBranch className="h-[13px] w-[13px] shrink-0 opacity-75" />
                        <span className="truncate">{branchName}</span>
                      </>
                    ) : (
                      <span className="truncate">{selectedCwd}</span>
                    )}
                  </div>
                </div>
                <DropdownMenuSeparator />
              </>
            )}

            <DropdownMenuLabel>Open In</DropdownMenuLabel>
            {externalEditors.map((ed) => (
              <DropdownMenuItem
                key={ed.kind}
                onSelect={() => void orbitActions.openWorkspaceInEditor(ed.kind)}
              >
                <span className="flex items-center gap-2.5">
                  <EditorItemIcon kind={ed.kind} />
                  <span className={cn(!ed.installed && "text-muted-foreground")}>{ed.label}</span>
                </span>
              </DropdownMenuItem>
            ))}

            {selectedCwd && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onSelect={() => void handleCopyCwd()}>
                  <span className="flex items-center gap-2.5">
                    {copiedPath ? (
                      <Check className="h-[15px] w-[15px] shrink-0 text-emerald-500" />
                    ) : (
                      <Copy className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />
                    )}
                    <span>Copy Path</span>
                  </span>
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>

        {hasActiveSession && (
          <SimpleTooltip label={overviewVisible ? "Hide overview" : "Show overview"} shortcut={`${mod}⌥O`}>
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
              <LayoutList className="h-[15.5px] w-[15.5px] stroke-[1.75]" />
              {!overviewVisible && hasRunningBackgroundActivity && (
                <span className="bg-primary absolute top-1.5 right-1.5 h-1.5 w-1.5 animate-pulse rounded-full" />
              )}
            </button>
          </SimpleTooltip>
        )}

        <SimpleTooltip label="Diff Review" shortcut={`${mod}D`}>
          <button
            type="button"
            aria-label="Diff Review"
            onClick={() => orbitActions.toggleReviewTab()}
            className={cn(
              "inline-flex h-[28px] cursor-pointer items-center justify-center gap-1.5 rounded-[7px] transition-colors",
              changedBadgeCount > 0 ? "px-2" : "w-[28px]",
              activeSidePanelTabKind === "review"
                ? "bg-black/[0.06] text-foreground dark:bg-white/[0.085]"
                : "text-foreground/65 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
            )}
          >
            <GitCompareArrows className="h-[16px] w-[16px] stroke-[1.75]" />
            {changedBadgeCount > 0 && (
              <span className="tabular-nums bg-foreground/10 text-foreground rounded-full px-1.5 py-0 text-[11.5px] font-medium">
                {changedBadgeCount}
              </span>
            )}
          </button>
        </SimpleTooltip>

        <SimpleTooltip label="Terminal" shortcut={`${mod}J`}>
          <button
            type="button"
            aria-label="Terminal"
            onClick={() => orbitActions.toggleTerminalTab()}
            className={cn(
              "inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors",
              activeSidePanelTabKind === "terminal"
                ? "bg-black/[0.06] text-foreground dark:bg-white/[0.085]"
                : "text-foreground/65 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
            )}
          >
            <SquareTerminal className="h-[16px] w-[16px] stroke-[1.75]" />
          </button>
        </SimpleTooltip>

        <SimpleTooltip
          label={sidePanelOpen ? "Hide side panel" : "Show side panel"}
          shortcut={`${mod}\\`}
        >
          <button
            type="button"
            aria-label={sidePanelOpen ? "Hide side panel" : "Show side panel"}
            onClick={() => orbitActions.toggleSidePanel()}
            className={cn(
              "inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors",
              sidePanelOpen
                ? "bg-black/[0.06] text-foreground dark:bg-white/[0.085]"
                : "text-foreground/65 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
            )}
          >
            <PanelRight className="h-[16px] w-[16px] stroke-[1.75]" />
          </button>
        </SimpleTooltip>
      </div>
    </header>
  );
});
