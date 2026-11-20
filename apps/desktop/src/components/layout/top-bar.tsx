import { memo } from "react";
import { FileText } from "lucide-react";
import { isAgentBackgroundWaiting, isMacDesktop } from "@/lib/orbit-ipc";
import { modSymbol } from "@/lib/keybindings";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import { SimpleTooltip } from "@/components/ui/overlays";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export const TopBar = memo(function TopBar() {
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const overviewVisible = useOrbitSelector((s) =>
    s.stageCanDockOverview ? s.overviewDesktopOpenPref : s.overviewPopoverOpen,
  );
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const hasActiveSession = workspacePanel === "conversation" && !isDraft && Boolean(selectedAgentId);
  const hasRunningBackgroundTasks = useOrbitSelector((s) => {
    if (
      s.workspacePanel !== "conversation" ||
      s.isDraftNewSession ||
      !s.selectedHostId ||
      !s.selectedAgentId
    ) {
      return false;
    }
    const ag = s.hostDetails[s.selectedHostId]?.agents.find(
      (a) => a.agent_id === s.selectedAgentId,
    );
    if (isAgentBackgroundWaiting(ag)) return true;
    const tasks =
      s.stages[`${s.selectedHostId}::${s.selectedAgentId}`]?.insights?.background_tasks;
    return Boolean(tasks && tasks.some((t) => t.status === "running"));
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
        {hasActiveSession && (
          <SimpleTooltip
            label={
              overviewVisible
                ? "Hide overview"
                : hasRunningBackgroundTasks
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
              {!overviewVisible && hasRunningBackgroundTasks && (
                <span
                  aria-hidden="true"
                  className="ring-canvas absolute top-[5px] right-[5px] h-[5px] w-[5px] rounded-full bg-emerald-500/90 ring-[1.5px]"
                />
              )}
            </button>
          </SimpleTooltip>
        )}
      </div>
    </header>
  );
});
