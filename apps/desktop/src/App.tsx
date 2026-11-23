import { memo, useEffect, useMemo, useRef, useState } from "react";
import { ExternalLink, PanelLeft, PanelRight } from "lucide-react";
import { resyncAllAmbientAnimations } from "@/lib/animation-sync";
import { AutomationsView } from "@/components/automations/automations-view";
import { CommandPalette } from "@/components/command/command-palette";
import {
  DetachedTerminalWindow,
  parseDetachedTerminalParams,
} from "@/components/detached-terminal-window";
import { MarkdownThemeProvider } from "@/components/markdown-blocks";
import { CopyCodeBlocksModal } from "@/components/modals/copy-code-blocks-modal";
import { ExportTranscriptModal } from "@/components/modals/export-transcript-modal";
import { WorktreeHandoffModal } from "@/components/modals/worktree-handoff-modal";
import { OnboardingGuide } from "@/components/onboarding/onboarding-guide";
import { MediaPreviewModal } from "@/components/viewers/media-preview-modal";
import { ConnectionStatusBanner } from "@/components/layout/connection-status-banner";
import { TopBar } from "@/components/layout/top-bar";
import { SettingsView } from "@/components/settings/settings-view";
import { QuickOpenFileDialog } from "@/components/side-panel/quick-open-file-dialog";
import {
  WorkspaceMotionProvider,
  useWorkspaceMotion,
} from "@/components/layout/workspace-motion";
import { WorkspaceSidePanel } from "@/components/side-panel/workspace-side-panel";
import { FleetSidebar } from "@/components/sidebar/fleet-sidebar";
import { RemoteFolderPickerDialog } from "@/components/sidebar/remote-folder-picker-dialog";
import { StreamStage } from "@/components/stage/stream-stage";
import { WorkspacePeerStrip } from "@/components/stage/workspace-peer-strip";
import { SimpleTooltip, TooltipProvider } from "@/components/ui/overlays";
import { PanelErrorBoundary } from "@/components/ui/panel-error-boundary";
import {
  isConfirmDialogOpen,
  modSymbol,
  useGlobalErgonomicKeybindings,
} from "@/lib/keybindings";
import { orbitIpc, isMacDesktop } from "@/lib/orbit-ipc";
import { type ToastAction, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export default function App() {
  const detachedTerminal = useMemo(
    () =>
      typeof window !== "undefined" ? parseDetachedTerminalParams(window.location.search) : null,
    [],
  );

  if (detachedTerminal) {
    return (
      <TooltipProvider>
        <DetachedTerminalWindow {...detachedTerminal} />
      </TooltipProvider>
    );
  }

  return <MainDesktopApp />;
}

function MainDesktopApp() {
  const isDarkTheme = useOrbitSelector((s) => s.resolvedTheme === "dark");
  const activeView = useOrbitSelector((s) => s.activeView);
  const settingsSection = useOrbitSelector((s) => s.settingsSection);
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const toastNotice = useOrbitSelector((s) => s.toastNotice);
  const toastAction = useOrbitSelector((s) => s.toastAction);
  const lastArchivedUndo = useOrbitSelector((s) => s.lastArchivedUndo);

  // Presence state for smooth Settings modal enter & exit
  const isSettingsOpen = activeView === "settings";
  const [settingsMounted, setSettingsMounted] = useState(isSettingsOpen);
  const [settingsClosing, setSettingsClosing] = useState(false);

  // Presence state for smooth Toast enter & exit
  const [renderedToast, setRenderedToast] = useState<string | null>(toastNotice);
  const [renderedToastAction, setRenderedToastAction] = useState<ToastAction | null>(toastAction);
  const [toastExiting, setToastExiting] = useState(false);

  useGlobalErgonomicKeybindings();

  useEffect(() => {
    void orbitActions.init();
  }, []);

  // Re-attach terminal tab into Side Panel when requested from a floating detached terminal window
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    void orbitIpc
      .onReattachTerminal(({ host_id, terminal_id, title, cwd, slot }) => {
        if (!host_id || !terminal_id) return;
        const snap = orbitActions.getSnapshot();
        const detail = snap.hostDetails[host_id];
        const termMeta = detail?.terminals.find((t) => t.terminal_id === terminal_id);
        const resolvedCwd = cwd || termMeta?.cwd || snap.selectedWorkspaceCwd || "~";
        const resolvedTitle = title || termMeta?.title || "shell";
        orbitActions.closeSettings();
        void orbitActions.attachExistingTerminal(
          host_id,
          terminal_id,
          resolvedCwd,
          resolvedTitle,
          slot ?? null,
        );
      })
      .then((fn) => {
        if (!active) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  // Listen to native menu bar / system tray "New Conversation" action
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    void orbitIpc
      .onTrayNewConversation(() => {
        orbitActions.closeSettings();
        orbitActions.closeOnboardingGuide(false);
        orbitActions.startNewThreadDraft();
      })
      .then((fn) => {
        if (!active) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  // Track OS/WebView internet connectivity & window foreground/background lifecycle
  useEffect(() => {
    if (typeof window === "undefined") return;
    const syncOnline = () => {
      orbitActions.setBrowserOffline(!navigator.onLine);
      if (navigator.onLine) {
        void orbitIpc.onNetworkChanged();
      }
    };
    const onFocus = () => void orbitIpc.onAppForeground();
    const syncVisibilityClass = () => {
      document.documentElement.classList.toggle(
        "orbit-window-hidden",
        document.visibilityState !== "visible",
      );
    };
    const onVisibility = () => {
      syncVisibilityClass();
      if (document.visibilityState === "visible") {
        window.requestAnimationFrame(() => resyncAllAmbientAnimations());
        void orbitIpc.onAppForeground();
      } else {
        orbitActions.flushPendingSeenMarks();
        void orbitIpc.onAppBackground();
      }
    };
    const onPageHide = () => {
      orbitActions.flushPendingSeenMarks();
    };
    syncOnline();
    syncVisibilityClass();
    window.addEventListener("offline", syncOnline);
    window.addEventListener("online", syncOnline);
    window.addEventListener("focus", onFocus);
    window.addEventListener("pagehide", onPageHide);
    window.addEventListener("beforeunload", onPageHide);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("offline", syncOnline);
      window.removeEventListener("online", syncOnline);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("pagehide", onPageHide);
      window.removeEventListener("beforeunload", onPageHide);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, []);

  // Deterministic responsive viewport sync (`< 800px` auto-collapses, `>= 824px` restores desktop preference)
  useEffect(() => {
    orbitActions.updateViewportWidth(window.innerWidth);

    let rafId = 0;
    const onResize = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        orbitActions.updateViewportWidth(window.innerWidth);
      });
    };

    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, []);

  useEffect(() => {
    if (isSettingsOpen) {
      setSettingsMounted(true);
      setSettingsClosing(false);
      return;
    }
    if (settingsMounted) {
      setSettingsClosing(true);
      const id = window.setTimeout(() => {
        setSettingsMounted(false);
        setSettingsClosing(false);
      }, 165);
      return () => window.clearTimeout(id);
    }
  }, [isSettingsOpen, settingsMounted]);

  useEffect(() => {
    if (!lastArchivedUndo) return;
    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.isContentEditable)
      ) {
        return;
      }
      if (isConfirmDialogOpen()) {
        return;
      }
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key.toLowerCase() === "z") {
        e.preventDefault();
        orbitActions.undoLastArchive();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [lastArchivedUndo]);

  useEffect(() => {
    if (toastNotice) {
      setRenderedToast(toastNotice);
      setRenderedToastAction(toastAction);
      setToastExiting(false);
      return;
    }
    if (renderedToast) {
      setToastExiting(true);
      const id = window.setTimeout(() => {
        setRenderedToast(null);
        setRenderedToastAction(null);
        setToastExiting(false);
      }, 170);
      return () => window.clearTimeout(id);
    }
  }, [toastNotice, toastAction, renderedToast]);

  return (
    <TooltipProvider>
      <MarkdownThemeProvider isDark={isDarkTheme}>
        {/* Dedicated Full-Window Settings Center (`⌘,`) with Smooth Enter/Exit */}
        {settingsMounted && (
          <PanelErrorBoundary
            panelName="Settings"
            resetKey={`${activeView}:${settingsSection}`}
            className="bg-canvas fixed inset-0 z-40"
          >
            <SettingsView closing={settingsClosing} />
          </PanelErrorBoundary>
        )}

        {/* Main Workspace Shell coordinated by 100% GPU WorkspaceMotionProvider */}
        <WorkspaceMotionProvider>
          <WorkspaceShellLayout isSettingsOpen={isSettingsOpen} workspacePanel={workspacePanel} />
        </WorkspaceMotionProvider>

        {/* Global Command Palette (`⌘K`), Quick Open File (`⌘P`), Export & Code Picker */}
        <CommandPalette />
        <QuickOpenFileDialog />
        <RemoteFolderPickerDialog />
        <ExportTranscriptModal />
        <CopyCodeBlocksModal />
        <WorktreeHandoffModal />
        <OnboardingGuide />
        <MediaPreviewModal />

        {/* Restrained Ephemeral Status Toast with Smooth Enter/Exit, Actions & Undo Archive */}
        {renderedToast && (
          <div
            className={cn(
              "bg-elevated text-foreground floating-surface fixed right-4 bottom-4 z-50 flex items-center gap-2.5 rounded-xl border border-black/[0.08] px-3.5 py-2 text-[13px] dark:border-white/[0.10]",
              lastArchivedUndo || renderedToastAction ? "pointer-events-auto" : "pointer-events-none",
              toastExiting ? "orbit-toast-exit" : "orbit-toast-enter",
            )}
          >
            <span>{renderedToast}</span>
            {renderedToastAction && (
              <button
                type="button"
                onClick={() => {
                  if (renderedToastAction.url) {
                    void orbitIpc.openExternalBrowserUrl(renderedToastAction.url);
                  }
                  renderedToastAction.onClick?.();
                }}
                className="text-primary hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[12px] font-medium transition-colors"
              >
                <span>{renderedToastAction.label}</span>
                {renderedToastAction.url && (
                  <ExternalLink className="h-[11.5px] w-[11.5px] shrink-0" />
                )}
              </button>
            )}
            {lastArchivedUndo && (
              <button
                type="button"
                onClick={() => orbitActions.undoLastArchive()}
                className="text-primary hover:bg-accent cursor-pointer rounded-md px-2 py-0.5 text-[12px] font-medium transition-colors"
              >
                Undo
              </button>
            )}
          </div>
        )}
      </MarkdownThemeProvider>
    </TooltipProvider>
  );
}

const StableFleetSidebar = memo(function StableFleetSidebar() {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  return (
    <PanelErrorBoundary panelName="Fleet Sidebar" compact resetKey={selectedHostId}>
      <FleetSidebar />
    </PanelErrorBoundary>
  );
});

const WorkspaceShellLayout = memo(function WorkspaceShellLayout({
  isSettingsOpen,
  workspacePanel,
}: {
  isSettingsOpen: boolean;
  workspacePanel: "conversation" | "automations";
}) {
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const sidePanelOpen = useOrbitSelector((s) => s.sidePanelOpen);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const [isResizingSidebar, setIsResizingSidebar] = useState(false);
  const sidebarElRef = useRef<HTMLDivElement | null>(null);

  const {
    isAnimating,
    hasSelectedHost,
    leftWidthCss,
    leftLayoutReserveCss,
    rightLayoutReserveCss,
    shellReserveTransition,
    leftVisible,
    leftTransform,
    leftTransition,
    setIsResizingLeft,
    onLeftTransitionEnd,
  } = useWorkspaceMotion();

  const isMac = isMacDesktop();
  const mod = modSymbol();

  const startSidebarResize = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if (e.detail >= 2) {
      e.preventDefault();
      return;
    }
    e.preventDefault();
    const startX = e.clientX;
    const startW = Math.round(sidebarElRef.current?.getBoundingClientRect().width ?? 256);
    let latestW = startW;
    let hasMoved = false;
    let rafId: number | null = null;

    const onMove = (ev: MouseEvent) => {
      const delta = ev.clientX - startX;
      if (!hasMoved) {
        if (Math.abs(delta) < 2) return;
        hasMoved = true;
        setIsResizingSidebar(true);
        setIsResizingLeft(true);
        document.body.style.cursor = "col-resize";
      }
      latestW = startW + delta;
      if (rafId === null) {
        rafId = window.requestAnimationFrame(() => {
          rafId = null;
          orbitActions.setSidebarWidth(latestW, false);
        });
      }
    };
    const cleanup = () => {
      if (rafId !== null) {
        window.cancelAnimationFrame(rafId);
        rafId = null;
      }
      if (hasMoved) {
        orbitActions.setSidebarWidth(latestW, true);
        setIsResizingSidebar(false);
        setIsResizingLeft(false);
        document.body.style.cursor = "";
      }
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", cleanup);
      window.removeEventListener("blur", cleanup);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", cleanup);
    window.addEventListener("blur", cleanup);
  };

  const resetSidebarWidth = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    orbitActions.resetSidebarWidth();
  };

  return (
    <>
      {/* Transparent Drag Shield so embedded iframes / WebGL canvases never swallow pointer events during resize */}
      {isResizingSidebar && <div className="fixed inset-0 z-50 cursor-col-resize select-none" />}

      <div className="bg-canvas text-foreground relative flex h-screen w-screen overflow-clip">
        {/* Single Stationary Titlebar Sidebar Toggle (`⌘B`) — optically centered with macOS traffic lights */}
        {!isSettingsOpen && (
          <div
            className={cn(
              "pointer-events-none fixed top-0 z-30 flex h-[40px] items-center",
              isMac ? "left-[80px] translate-y-[1px]" : "left-[12px]",
            )}
          >
            <SimpleTooltip
              label={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
              shortcut={`${mod}B`}
            >
              <button
                type="button"
                aria-label={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
                onClick={() => orbitActions.toggleSidebar()}
                className="text-foreground/64 hover:text-foreground pointer-events-auto inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
              >
                <PanelLeft className="h-[16px] w-[16px] stroke-[1.75]" />
              </button>
            </SimpleTooltip>
          </div>
        )}

        {/* Single Stationary Top-Right Side Panel Toggle (`⌘\`) */}
        {!isSettingsOpen && hasSelectedHost && (
          <div
            className={cn(
              "pointer-events-none fixed top-0 right-[10px] z-30 flex h-[40px] items-center",
              isMac && "translate-y-[1px]",
            )}
          >
            <SimpleTooltip
              label={sidePanelOpen ? "Hide side panel" : "Show side panel"}
              shortcut={`${mod}\\`}
            >
              <button
                type="button"
                aria-label={sidePanelOpen ? "Hide side panel" : "Show side panel"}
                onClick={() => orbitActions.toggleSidePanel()}
                className={cn(
                  "pointer-events-auto inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors",
                  sidePanelOpen
                    ? "bg-black/[0.06] text-foreground dark:bg-white/[0.085]"
                    : "text-foreground/64 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
                )}
              >
                <PanelRight className="h-[16px] w-[16px] stroke-[1.75]" />
              </button>
            </SimpleTooltip>
          </div>
        )}

        {/* 1. Left Unified Fleet Sidebar (`⌘B`) — Pure GPU translate3d Layer */}
        <div
          ref={sidebarElRef}
          inert={!sidebarOpen}
          data-open={sidebarOpen ? "true" : "false"}
          data-animating={isAnimating ? "true" : "false"}
          data-resizing={isResizingSidebar ? "true" : "false"}
          onTransitionEnd={onLeftTransitionEnd}
          style={{
            width: leftWidthCss,
            transform: leftTransform,
            transition: leftTransition,
            visibility: leftVisible ? "visible" : "hidden",
          }}
          className={cn(
            "orbit-panel-left bg-sidebar absolute inset-y-0 left-0 z-20 h-full shrink-0 overflow-hidden [contain:layout_style] [backface-visibility:hidden]",
            !sidebarOpen && "pointer-events-none",
          )}
        >
          <StableFleetSidebar />

          {sidebarOpen && (
            <div
              role="separator"
              aria-orientation="vertical"
              aria-label="Resize sidebar (double-click to reset)"
              onMouseDown={startSidebarResize}
              onDoubleClick={resetSidebarWidth}
              className="group/sash absolute top-0 right-0 z-20 flex h-full w-[6px] cursor-col-resize justify-end"
            >
              <div className="group-hover/sash:bg-primary/45 group-active/sash:bg-primary/65 h-full w-[2px] transition-colors duration-150" />
            </div>
          )}
        </div>

        {/* 2. Center Narrative Stage (pure CSS variables when idle; coordinated motion on panel toggle) */}
        <div
          className="flex min-h-0 min-w-0 flex-1 flex-col overflow-clip [contain:layout_style]"
          style={{
            paddingLeft: leftLayoutReserveCss,
            paddingRight: rightLayoutReserveCss,
            transition: shellReserveTransition,
          }}
        >
          <TopBar />
          <ConnectionStatusBanner />
          <WorkspacePeerStrip />
          <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
            {workspacePanel === "automations" ? (
              <PanelErrorBoundary
                panelName="Automations"
                resetKey={`${selectedHostId ?? ""}:${workspacePanel}`}
              >
                <AutomationsView />
              </PanelErrorBoundary>
            ) : (
              <PanelErrorBoundary
                panelName="Conversation Stage"
                resetKey={`${selectedHostId ?? ""}:${selectedAgentId ?? ""}:${isDraftNewSession ? "draft" : "thread"}`}
              >
                <StreamStage />
              </PanelErrorBoundary>
            )}
          </main>
        </div>

        {/* 3. Unified Multi-Tab Right Side Panel — Pure GPU translate3d Layer */}
        <WorkspaceSidePanel />
      </div>
    </>
  );
});
