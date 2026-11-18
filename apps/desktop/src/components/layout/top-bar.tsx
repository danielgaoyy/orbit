import { useMemo, useState } from "react";
import {
  Check,
  ChevronDown,
  ExternalLink,
  GitBranch,
  PanelLeft,
  PanelRight,
  Terminal as TerminalIcon,
} from "lucide-react";
import { isMacDesktop } from "@/lib/orbit-ipc";
import { processTimelineForStage } from "@/lib/timeline-coalesce";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function basenameOfPath(p: string): string {
  if (!p || p === "~") return "~";
  const trimmed = p.replace(/\/+$/, "");
  const idx = trimmed.lastIndexOf("/");
  return idx >= 0 ? trimmed.slice(idx + 1) || "/" : trimmed;
}

export function TopBar() {
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const inspectorOpen = useOrbitSelector((s) => s.inspectorOpen);
  const terminalOpen = useOrbitSelector((s) => s.terminalOpen);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const fleetHostCount = useOrbitSelector((s) => s.fleet.hosts.length);
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const gitReview = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedWorkspaceCwd
      ? (s.gitReviews[`${s.selectedHostId}::${s.selectedWorkspaceCwd}`] ?? null)
      : null,
  );
  const activeTimeline = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedAgentId
      ? (s.timelines[`${s.selectedHostId}::${s.selectedAgentId}`] ?? null)
      : null,
  );

  const [copiedPath, setCopiedPath] = useState(false);

  const currentAgent = useMemo(
    () => hostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ?? null,
    [hostDetail, selectedAgentId],
  );

  const currentWorkspace = useMemo(
    () => hostDetail?.workspaces.find((w) => w.cwd === selectedCwd) ?? null,
    [hostDetail, selectedCwd],
  );

  const branchName =
    gitReview?.status?.branch ??
    currentWorkspace?.git_branch ??
    hostDetail?.discovered_projects.find((p) => p.path === selectedCwd)?.git_branch ??
    null;

  const sessionDiffCount = useMemo(() => {
    if (!activeTimeline) return 0;
    return processTimelineForStage(activeTimeline.items).sessionTurnDiffs.length;
  }, [activeTimeline]);

  const changedBadgeCount = gitReview?.status?.files.length || sessionDiffCount;
  const macTrafficInset = isMacDesktop() && !sidebarOpen;

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
        "bg-canvas/90 flex h-[38px] shrink-0 items-center justify-between gap-3 px-4 backdrop-blur-md select-none",
        !isDraft && selectedAgentId && "border-b border-black/[0.055] dark:border-white/[0.06]",
        macTrafficInset && "pl-[86px]",
      )}
    >
      {/* Left: Sidebar Toggle + Restrained Session / Workspace Context */}
      <div className="flex min-w-0 items-center gap-2">
        <SimpleTooltip label={sidebarOpen ? "Hide sidebar" : "Show sidebar"}>
          <button
            type="button"
            onClick={() => orbitActions.toggleSidebar()}
            className={cn(
              "text-foreground/65 hover:text-foreground inline-flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-lg transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
              !sidebarOpen && "text-foreground",
            )}
          >
            <PanelLeft className="size-[17px] stroke-[1.75]" />
          </button>
        </SimpleTooltip>

        {hostDetail && !isDraft && currentAgent && (
          <div className="flex min-w-0 items-center gap-2 text-[13.5px]">
            <span className="text-foreground/92 max-w-72 truncate font-medium">
              {currentAgent.title}
            </span>

            <button
              type="button"
              onClick={() => void handleCopyCwd()}
              title={selectedCwd}
              className="text-foreground/52 hover:text-foreground hidden cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[12.5px] transition-colors hover:bg-black/[0.045] sm:inline-flex dark:hover:bg-white/[0.06]"
            >
              {fleetHostCount > 1 && <span>{hostDetail.host.display_name} ·</span>}
              <span className="max-w-36 truncate">{basenameOfPath(selectedCwd)}</span>
              {copiedPath && <Check className="size-3.5 text-emerald-500" />}
            </button>

            {branchName && (
              <span className="text-foreground/45 hidden max-w-36 items-center gap-1 truncate font-mono text-[11.5px] md:inline-flex">
                <GitBranch className="size-3 shrink-0 opacity-70" />
                <span className="truncate">{branchName}</span>
              </span>
            )}
          </div>
        )}
      </div>

      {/* Right: Quiet Workspace Handoff + Diff & Terminal Toggles (Zero Shortcut Clutter) */}
      <div className="flex shrink-0 items-center gap-1">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              className="text-foreground/68 hover:text-foreground inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-lg px-2.5 text-[13px] font-normal transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
            >
              <ExternalLink className="size-3.5 stroke-[1.75] opacity-80" />
              <span className="hidden sm:inline">Open</span>
              <ChevronDown className="size-3 opacity-55" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-46">
            <DropdownMenuLabel>Open Workspace In</DropdownMenuLabel>
            {externalEditors.map((ed) => (
              <DropdownMenuItem
                key={ed.kind}
                onSelect={() => void orbitActions.openWorkspaceInEditor(ed.kind)}
              >
                <span className={cn(!ed.installed && "text-muted-foreground")}>{ed.label}</span>
              </DropdownMenuItem>
            ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => void handleCopyCwd()}>
              <span>Copy Path</span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>

        <SimpleTooltip label="Diff Review">
          <button
            type="button"
            onClick={() => orbitActions.toggleInspector()}
            className={cn(
              "inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-lg px-2 text-[13px] font-normal transition-colors",
              inspectorOpen
                ? "bg-black/[0.06] text-foreground dark:bg-white/[0.085]"
                : "text-foreground/65 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
            )}
          >
            <PanelRight className="size-[17px] stroke-[1.75]" />
            {changedBadgeCount > 0 && (
              <span className="tabular-nums bg-foreground/10 text-foreground rounded-full px-1.5 py-0 text-[11px] font-medium">
                {changedBadgeCount}
              </span>
            )}
          </button>
        </SimpleTooltip>

        <SimpleTooltip label="Terminal">
          <button
            type="button"
            onClick={() => orbitActions.toggleTerminalDock()}
            className={cn(
              "inline-flex size-8 cursor-pointer items-center justify-center rounded-lg transition-colors",
              terminalOpen
                ? "bg-black/[0.06] text-foreground dark:bg-white/[0.085]"
                : "text-foreground/65 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
            )}
          >
            <TerminalIcon className="size-[17px] stroke-[1.75]" />
          </button>
        </SimpleTooltip>
      </div>
    </header>
  );
}
