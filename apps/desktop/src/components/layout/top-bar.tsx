import { memo } from "react";
import { FileText, PanelRight } from "lucide-react";
import { isMacDesktop } from "@/lib/orbit-ipc";
import { modSymbol } from "@/lib/keybindings";
import { SimpleTooltip } from "@/components/ui/overlays";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export const TopBar = memo(function TopBar() {
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const sidePanelOpen = useOrbitSelector((s) => s.sidePanelOpen);
  const overviewVisible = useOrbitSelector((s) =>
    s.stageCanDockOverview ? s.overviewDesktopOpenPref : s.overviewPopoverOpen,
  );
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const hasActiveSession = workspacePanel === "conversation" && !isDraft && Boolean(selectedAgentId);

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
      s.timelines[`${s.selectedHostId}::${s.selectedAgentId}`]?.agent;
    return ag?.title || "New Conversation";
  });

  const mod = modSymbol();
  const isMac = isMacDesktop();

  const showBottomBorder =
    workspacePanel === "automations" || (!isDraft && Boolean(selectedAgentId));

  return (
    <header
      data-tauri-drag-region
      className={cn(
        "bg-canvas flex h-[40px] shrink-0 items-center justify-between gap-3 px-4 transition-[padding-left] select-none",
        sidebarOpen
          ? "duration-[260ms] ease-[var(--ease-panel)]"
          : "duration-[225ms] ease-[var(--ease-panel-out)]",
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

      {/* Right: Overview & Right Side Panel Toggles */}
      <div className={cn("flex shrink-0 items-center gap-1", isMac && "translate-y-[1px]")}>
        {hasActiveSession && (
          <SimpleTooltip
            label={overviewVisible ? "Hide overview" : "Show overview"}
            shortcut={`${mod}⌥O`}
          >
            <button
              type="button"
              data-overview-trigger
              aria-label={overviewVisible ? "Hide overview" : "Show overview"}
              onClick={() => orbitActions.toggleOverviewPanel()}
              className={cn(
                "inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors",
                overviewVisible
                  ? "bg-black/[0.06] text-foreground dark:bg-white/[0.085]"
                  : "text-foreground/65 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
              )}
            >
              <FileText className="h-[15.5px] w-[15.5px] stroke-[1.75]" />
            </button>
          </SimpleTooltip>
        )}

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
