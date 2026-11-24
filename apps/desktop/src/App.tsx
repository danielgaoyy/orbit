import { memo, useEffect, useMemo, useRef, useState } from "react";
import { PanelLeft, PanelRight } from "lucide-react";
import { resyncAllAmbientAnimations } from "@/lib/animation-sync";
import { AutomationsView } from "@/components/automations/automations-view";
import { PersonalAgentControlView } from "@/components/overview/personal-agent-control-view";
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
import { IconButton } from "@/components/ui/icon-button";
import { TooltipProvider } from "@/components/ui/overlays";
import { PanelErrorBoundary } from "@/components/ui/panel-error-boundary";
import { ToastViewport } from "@/components/ui/toast-viewport";
import { SIDEBAR_DEFAULT_WIDTH_PX } from "@/lib/workspace-layout";
import {
  isConfirmDialogOpen,
  useActionShortcutDisplay,
  useGlobalErgonomicKeybindings,
} from "@/lib/keybindings";
import { orbitIpc, isMacDesktop } from "@/lib/orbit-ipc";
import {
  type ToastAction,
  type WorkspacePanelMode,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    Boolean(window.matchMedia?.("(prefers-reduced-motion: reduce)").matches)
  );
}

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

  // Presence state for smooth Toast enter & exit (monotonic revision key re-triggers enter on replacement)
  const [renderedToast, setRenderedToast] = useState<string | null>(toastNotice);
  const [renderedToastAction, setRenderedToastAction] = useState<ToastAction | null>(toastAction);
  const [toastExiting, setToastExiting] = useState(false);
  const [toastRevision, setToastRevision] = useState(0);
  const renderedToastRef = useRef<string | null>(renderedToast);
  renderedToastRef.current = renderedToast;

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

  // Listen to native OS notification open / resolved action events
  useEffect(() => {
    let active = true;
    const unlisteners: Array<() => void> = [];
    void Promise.all([
      orbitIpc.onNativeNotificationOpen(({ host_id, agent_id }) => {
        if (!host_id) return;
        orbitActions.closeSettings();
        orbitActions.closeOnboardingGuide(false);
        if (agent_id) {
          orbitActions.selectAgent(host_id, agent_id);
        } else {
          orbitActions.selectHost(host_id);
        }
      }),
      orbitIpc.onNativeNotificationResolved(({ host_id }) => {
        if (host_id) {
          void orbitActions.reloadHostDetail(host_id, true);
        }
      }),
    ])
      .then((fns) => {
        if (!active) {
          for (const fn of fns) fn();
        } else {
          unlisteners.push(...fns);
        }
      })
      .catch(() => undefined);
    return () => {
      active = false;
      for (const fn of unlisteners) fn();
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
      if (prefersReducedMotion()) {
        setSettingsMounted(false);
        setSettingsClosing(false);
        return;
      }
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
      setToastRevision((r) => r + 1);
      return;
    }
    if (renderedToastRef.current) {
      if (prefersReducedMotion()) {
        setRenderedToast(null);
        setRenderedToastAction(null);
        setToastExiting(false);
        return;
      }
      setToastExiting(true);
      const id = window.setTimeout(() => {
        setRenderedToast(null);
        setRenderedToastAction(null);
        setToastExiting(false);
      }, 170);
      return () => window.clearTimeout(id);
    }
  }, [toastNotice, toastAction]);

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
        <ToastViewport
          key={toastRevision}
          message={renderedToast}
          action={renderedToastAction}
          undoEntry={lastArchivedUndo}
          exiting={toastExiting}
          onUndoArchive={() => orbitActions.undoLastArchive()}
        />
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
  workspacePanel: WorkspacePanelMode;
}) {
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const sidePanelOpen = useOrbitSelector((s) => s.sidePanelOpen);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const toggleSidebarShortcut = useActionShortcutDisplay("toggle_sidebar");
  const toggleSidePanelShortcut = useActionShortcutDisplay("toggle_side_panel");
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

  const startSidebarResize = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if (e.detail >= 2) {
      e.preventDefault();
      return;
    }
    e.preventDefault();
    const startX = e.clientX;
    const startW = Math.round(
      sidebarElRef.current?.getBoundingClientRect().width ?? SIDEBAR_DEFAULT_WIDTH_PX,
    );
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

      <div className="bg-surface-base text-ink-primary relative flex h-screen w-screen overflow-clip">
        {/* Single Stationary Titlebar Sidebar Toggle (`⌘B`) — optically centered with macOS traffic lights */}
        {!isSettingsOpen && (
          <div
            className={cn(
              "pointer-events-none fixed top-0 z-30 flex h-[40px] items-center",
              isMac ? "left-[80px] translate-y-[1px]" : "left-[12px]",
            )}
          >
            <IconButton
              size="md"
              tooltip={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
              shortcut={toggleSidebarShortcut}
              onClick={() => orbitActions.toggleSidebar()}
              className="pointer-events-auto"
            >
              <PanelLeft className="size-[16px] stroke-[1.75]" />
            </IconButton>
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
            <IconButton
              size="md"
              active={sidePanelOpen}
              tooltip={sidePanelOpen ? "Hide side panel" : "Show side panel"}
              shortcut={toggleSidePanelShortcut}
              onClick={() => orbitActions.toggleSidePanel()}
              className="pointer-events-auto"
            >
              <PanelRight className="size-[16px] stroke-[1.75]" />
            </IconButton>
          </div>
        )}

        {/* 1. Left Unified Fleet Sidebar (`⌘B`) — Same-Hue Canvas + 1px Hairline Border */}
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
            "orbit-panel-left bg-surface-base border-border-subtle absolute inset-y-0 left-0 z-20 h-full shrink-0 overflow-hidden border-r [contain:layout_style] [backface-visibility:hidden]",
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
              <div className="group-hover/sash:bg-border-strong group-active/sash:bg-ink-muted h-full w-[1.5px] transition-colors duration-150" />
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
            ) : workspacePanel === "overview" ? (
              <PanelErrorBoundary
                panelName="Global Overview"
                resetKey={`${selectedHostId ?? ""}:${workspacePanel}`}
              >
                <PersonalAgentControlView />
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
