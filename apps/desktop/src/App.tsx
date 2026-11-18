import { useEffect } from "react";
import { CommandPalette } from "@/components/command/command-palette";
import { ReviewInspector } from "@/components/inspector/review-inspector";
import { TopBar } from "@/components/layout/top-bar";
import { SettingsView } from "@/components/settings/settings-view";
import { FleetSidebar } from "@/components/sidebar/fleet-sidebar";
import { StreamStage } from "@/components/stage/stream-stage";
import { TerminalDock } from "@/components/terminal/terminal-dock";
import { TooltipProvider } from "@/components/ui/overlays";
import { useGlobalErgonomicKeybindings } from "@/lib/keybindings";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

export default function App() {
  const activeView = useOrbitSelector((s) => s.activeView);
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const sidebarWidth = useOrbitSelector((s) => s.sidebarWidth);
  const toastNotice = useOrbitSelector((s) => s.toastNotice);

  useGlobalErgonomicKeybindings();

  useEffect(() => {
    void orbitActions.init();
  }, []);

  const startSidebarResize = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = sidebarWidth;

    const onMove = (ev: MouseEvent) => {
      orbitActions.setSidebarWidth(startW + (ev.clientX - startX));
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  return (
    <TooltipProvider>
      {/* Dedicated Full-Window Settings Center (`⌘,`) */}
      {activeView === "settings" && <SettingsView />}

      {/* Main Workspace Shell */}
      <div className="bg-canvas text-foreground flex h-screen w-screen overflow-hidden">
        {/* 1. Collapsible & Resizable Single Unified Fleet Sidebar (`⌘B`) */}
        {sidebarOpen && (
          <div style={{ width: `${sidebarWidth}px` }} className="relative h-full shrink-0">
            <FleetSidebar />
            <div
              onMouseDown={startSidebarResize}
              className="hover:bg-primary/40 absolute top-0 right-0 z-20 h-full w-1 cursor-col-resize transition-colors"
            />
          </div>
        )}

        {/* 2. Center Narrative Stage + Omni-Composer + Persistent Bottom PTY Dock (`⌘J`) */}
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <TopBar />

          <div className="flex min-h-0 flex-1 overflow-hidden">
            <main className="flex min-w-0 flex-1 flex-col overflow-hidden">
              <StreamStage />
              <TerminalDock />
            </main>

            {/* 3. Contextual Right-Hand Diff Review Inspector (`⌘D`) */}
            <ReviewInspector />
          </div>
        </div>
      </div>

      {/* Global Command Palette (`⌘K`) */}
      <CommandPalette />

      {/* Restrained Ephemeral Status Toast */}
      {toastNotice && (
        <div className="bg-elevated border-border text-foreground pointer-events-none fixed right-4 bottom-4 z-50 rounded-lg border px-3 py-1.5 text-xs shadow-lg">
          {toastNotice}
        </div>
      )}
    </TooltipProvider>
  );
}
