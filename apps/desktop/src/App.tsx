import { memo, useEffect, useState } from "react";
import { PanelLeft, PanelRight } from "lucide-react";
import { AutomationsView } from "@/components/automations/automations-view";
import { CommandPalette } from "@/components/command/command-palette";
import { MarkdownThemeProvider } from "@/components/markdown-blocks";
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
import { SimpleTooltip, TooltipProvider } from "@/components/ui/overlays";
import { modSymbol, useGlobalErgonomicKeybindings } from "@/lib/keybindings";
import { orbitIpc, isMacDesktop } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export default function App() {
  const isDarkTheme = useOrbitSelector((s) => s.resolvedTheme === "dark");
  const activeView = useOrbitSelector((s) => s.activeView);
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const toastNotice = useOrbitSelector((s) => s.toastNotice);
  const lastArchivedUndo = useOrbitSelector((s) => s.lastArchivedUndo);

  // Presence state for smooth Settings modal enter & exit
  const isSettingsOpen = activeView === "settings";
  const [settingsMounted, setSettingsMounted] = useState(isSettingsOpen);
  const [settingsClosing, setSettingsClosing] = useState(false);

  // Presence state for smooth Toast enter & exit
  const [renderedToast, setRenderedToast] = useState<string | null>(toastNotice);
  const [toastExiting, setToastExiting] = useState(false);

  useGlobalErgonomicKeybindings();

  useEffect(() => {
    void orbitActions.init();
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
    const onVisibility = () => {
      if (document.visibilityState === "visible") {
        void orbitIpc.onAppForeground();
      } else {
        void orbitIpc.onAppBackground();
      }
    };
    syncOnline();
    window.addEventListener("offline", syncOnline);
    window.addEventListener("online", syncOnline);
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.removeEventListener("offline", syncOnline);
      window.removeEventListener("online", syncOnline);
      window.removeEventListener("focus", onFocus);
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
      setToastExiting(false);
      return;
    }
    if (renderedToast) {
      setToastExiting(true);
      const id = window.setTimeout(() => {
        setRenderedToast(null);
        setToastExiting(false);
      }, 170);
      return () => window.clearTimeout(id);
    }
  }, [toastNotice, renderedToast]);

  return (
    <TooltipProvider>
      <MarkdownThemeProvider isDark={isDarkTheme}>
        {/* Dedicated Full-Window Settings Center (`⌘,`) with Smooth Enter/Exit */}
        {settingsMounted && <SettingsView closing={settingsClosing} />}

        {/* Main Workspace Shell coordinated by 100% GPU WorkspaceMotionProvider */}
        <WorkspaceMotionProvider>
          <WorkspaceShellLayout isSettingsOpen={isSettingsOpen} workspacePanel={workspacePanel} />
        </WorkspaceMotionProvider>

        {/* Global Command Palette (`⌘K`), Quick Open File (`⌘P`) & Remote Folder Browser */}
        <CommandPalette />
        <QuickOpenFileDialog />
        <RemoteFolderPickerDialog />

        {/* Restrained Ephemeral Status Toast with Smooth Enter/Exit & Undo Archive */}
        {renderedToast && (
          <div
            className={cn(
              "bg-elevated text-foreground floating-surface fixed right-4 bottom-4 z-50 flex items-center gap-2.5 rounded-xl border border-black/[0.08] px-3.5 py-2 text-[13px] dark:border-white/[0.10]",
              lastArchivedUndo ? "pointer-events-auto" : "pointer-events-none",
              toastExiting ? "orbit-toast-exit" : "orbit-toast-enter",
            )}
          >
            <span>{renderedToast}</span>
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
  return <FleetSidebar />;
});

const WorkspaceShellLayout = memo(function WorkspaceShellLayout({
  isSettingsOpen,
  workspacePanel,
}: {
  isSettingsOpen: boolean;
  workspacePanel: "conversation" | "automations";
}) {
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const sidebarWidth = useOrbitSelector((s) => s.sidebarWidth);
  const sidePanelOpen = useOrbitSelector((s) => s.sidePanelOpen);
  const [isResizingSidebar, setIsResizingSidebar] = useState(false);

  const {
    isAnimating,
    hasSelectedHost,
    leftLayoutReservePx,
    rightLayoutReservePx,
    leftVisible,
    leftTransform,
    leftTransition,
    setIsResizingLeft,
    onLeftTransitionEnd,
  } = useWorkspaceMotion();

  const isMac = isMacDesktop();
  const mod = modSymbol();

  const startSidebarResize = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = sidebarWidth;
    setIsResizingSidebar(true);
    setIsResizingLeft(true);
    document.body.style.cursor = "col-resize";

    const onMove = (ev: MouseEvent) => {
      orbitActions.setSidebarWidth(startW + (ev.clientX - startX));
    };
    const cleanup = () => {
      setIsResizingSidebar(false);
      setIsResizingLeft(false);
      document.body.style.cursor = "";
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", cleanup);
      window.removeEventListener("blur", cleanup);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", cleanup);
    window.addEventListener("blur", cleanup);
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
          inert={!sidebarOpen}
          data-open={sidebarOpen ? "true" : "false"}
          data-animating={isAnimating ? "true" : "false"}
          data-resizing={isResizingSidebar ? "true" : "false"}
          onTransitionEnd={onLeftTransitionEnd}
          style={{
            width: `${sidebarWidth}px`,
            transform: leftTransform,
            transition: leftTransition,
            visibility: leftVisible ? "visible" : "hidden",
          }}
          className={cn(
            "orbit-panel-left absolute inset-y-0 left-0 z-20 h-full shrink-0 overflow-hidden [contain:layout_style]",
            !sidebarOpen && "pointer-events-none",
          )}
        >
          <StableFleetSidebar />

          {sidebarOpen && (
            <div
              onMouseDown={startSidebarResize}
              className="hover:bg-primary/40 absolute top-0 right-0 z-20 h-full w-1 cursor-col-resize transition-colors"
            />
          )}
        </div>

        {/* 2. Center Narrative Stage (statically reserved at rest, GPU-slid during panel motion) */}
        <div
          className="flex min-h-0 min-w-0 flex-1 flex-col overflow-clip"
          style={{
            paddingLeft: `${leftLayoutReservePx}px`,
            paddingRight: `${rightLayoutReservePx}px`,
          }}
        >
          <TopBar />
          <ConnectionStatusBanner />
          <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
            {workspacePanel === "automations" ? <AutomationsView /> : <StreamStage />}
          </main>
        </div>

        {/* 3. Unified Multi-Tab Right Side Panel — Pure GPU translate3d Layer */}
        <WorkspaceSidePanel />
      </div>
    </>
  );
});
