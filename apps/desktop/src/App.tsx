import { useEffect, useState } from "react";
import { PanelLeft } from "lucide-react";
import { AutomationsView } from "@/components/automations/automations-view";
import { CommandPalette } from "@/components/command/command-palette";
import { ReviewInspector } from "@/components/inspector/review-inspector";
import { TopBar } from "@/components/layout/top-bar";
import { SettingsView } from "@/components/settings/settings-view";
import { FleetSidebar } from "@/components/sidebar/fleet-sidebar";
import { RemoteFolderPickerDialog } from "@/components/sidebar/remote-folder-picker-dialog";
import { StreamStage } from "@/components/stage/stream-stage";
import { TerminalDock } from "@/components/terminal/terminal-dock";
import { SimpleTooltip, TooltipProvider } from "@/components/ui/overlays";
import { modSymbol, useGlobalErgonomicKeybindings } from "@/lib/keybindings";
import { isMacDesktop } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export default function App() {
  const activeView = useOrbitSelector((s) => s.activeView);
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const sidebarWidth = useOrbitSelector((s) => s.sidebarWidth);
  const toastNotice = useOrbitSelector((s) => s.toastNotice);

  const [isResizingSidebar, setIsResizingSidebar] = useState(false);
  const [transitionsReady, setTransitionsReady] = useState(false);

  // Presence state for smooth Settings modal enter & exit
  const isSettingsOpen = activeView === "settings";
  const [settingsMounted, setSettingsMounted] = useState(isSettingsOpen);
  const [settingsClosing, setSettingsClosing] = useState(false);

  // Presence state for smooth Toast enter & exit
  const [renderedToast, setRenderedToast] = useState<string | null>(toastNotice);
  const [toastExiting, setToastExiting] = useState(false);

  const isMac = isMacDesktop();
  const mod = modSymbol();

  useGlobalErgonomicKeybindings();

  useEffect(() => {
    void orbitActions.init();
    const raf = window.requestAnimationFrame(() => {
      setTransitionsReady(true);
    });
    return () => window.cancelAnimationFrame(raf);
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
      }, 120);
      return () => window.clearTimeout(id);
    }
  }, [isSettingsOpen, settingsMounted]);

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
      }, 145);
      return () => window.clearTimeout(id);
    }
  }, [toastNotice, renderedToast]);

  const startSidebarResize = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = sidebarWidth;
    setIsResizingSidebar(true);
    document.body.style.cursor = "col-resize";

    const onMove = (ev: MouseEvent) => {
      orbitActions.setSidebarWidth(startW + (ev.clientX - startX));
    };
    const cleanup = () => {
      setIsResizingSidebar(false);
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
    <TooltipProvider>
      {/* Dedicated Full-Window Settings Center (`⌘,`) with Smooth Enter/Exit */}
      {settingsMounted && <SettingsView closing={settingsClosing} />}

      {/* Main Workspace Shell */}
      <div className="bg-canvas text-foreground relative flex h-screen w-screen overflow-hidden">
        {/* Single Stationary Titlebar Sidebar Toggle (`[◫]`, `⌘B`) — optically centered with macOS traffic lights & title */}
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

        {/* 1. Collapsible & Resizable Single Unified Fleet Sidebar (`⌘B` — Single Source of Truth) */}
        <div
          inert={!sidebarOpen}
          style={{
            width: `${sidebarWidth}px`,
            marginLeft: sidebarOpen ? "0px" : `-${sidebarWidth}px`,
          }}
          className={cn(
            "relative h-full shrink-0 overflow-hidden",
            !sidebarOpen && "pointer-events-none",
            transitionsReady &&
              !isResizingSidebar &&
              "transition-[margin-left] duration-[190ms] ease-[cubic-bezier(0.22,1,0.36,1)]",
          )}
        >
          <FleetSidebar />

          {sidebarOpen && (
            <div
              onMouseDown={startSidebarResize}
              className="hover:bg-primary/40 absolute top-0 right-0 z-20 h-full w-1 cursor-col-resize transition-colors"
            />
          )}
        </div>

        {/* 2. Center Narrative Stage + Omni-Composer + Persistent Bottom PTY Dock (`⌘J`) */}
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <TopBar />

          <div className="flex min-h-0 flex-1 overflow-hidden">
            <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
              {workspacePanel === "automations" ? <AutomationsView /> : <StreamStage />}
              <TerminalDock />
            </main>

            {/* 3. Contextual Right-Hand Diff Review Inspector (`⌘D`) */}
            <ReviewInspector />
          </div>
        </div>
      </div>

      {/* Global Command Palette (`⌘K`) & Remote Folder Browser */}
      <CommandPalette />
      <RemoteFolderPickerDialog />

      {/* Restrained Ephemeral Status Toast with Smooth Enter/Exit */}
      {renderedToast && (
        <div
          className={cn(
            "bg-elevated text-foreground floating-surface pointer-events-none fixed right-4 bottom-4 z-50 rounded-xl border border-black/[0.08] px-3.5 py-2 text-[13px] dark:border-white/[0.10]",
            toastExiting ? "orbit-toast-exit" : "orbit-toast-enter",
          )}
        >
          {renderedToast}
        </div>
      )}
    </TooltipProvider>
  );
}
