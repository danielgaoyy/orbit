import { memo, useEffect, useRef } from "react";
import {
  Columns2,
  File,
  FileCode2,
  FileDiff,
  FileText,
  Globe,
  Layers,
  MessageCircleQuestion,
  Plus,
  SquareTerminal,
  X,
} from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { BrowserTab } from "@/components/side-panel/browser-tab";
import { FileViewerTab } from "@/components/side-panel/file-viewer-tab";
import { FilesExplorerTab } from "@/components/side-panel/files-explorer-tab";
import { PeerSplitTab } from "@/components/side-panel/peer-split-tab";
import { ReviewTab } from "@/components/side-panel/review-tab";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import { SideQuestionTab } from "@/components/side-panel/side-question-tab";
import { SubagentTab } from "@/components/side-panel/subagent-tab";
import { TerminalTab } from "@/components/side-panel/terminal-tab";
import { WorkspaceBoardTab } from "@/components/side-panel/workspace-board-tab";
import { SidePanelLaunchpad } from "@/components/side-panel/side-panel-launchpad";
import { SimpleTooltip } from "@/components/ui/overlays";
import { modSymbol } from "@/lib/keybindings";
import {
  type SidePanelTab,
  orbitActions,
  sideQuestionScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function SidePanelTabIcon({ tab }: { tab: SidePanelTab }) {
  switch (tab.kind) {
    case "review":
      return <FileDiff className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "files":
      return <File className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "file": {
      const Icon = tab.filePath ? resolveSemanticFileIcon(tab.filePath) : FileCode2;
      return <Icon className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    }
    case "terminal":
      return <SquareTerminal className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "browser":
      return <Globe className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "side_question":
      return <MessageCircleQuestion className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "subagent":
      return <Layers className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "board":
      return <FileText className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "peer":
      return <Columns2 className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
  }
}

export function WorkspaceSidePanel() {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const hasTerminalTab = useOrbitSelector((s) =>
    s.sidePanelTabs.some((t) => t.kind === "terminal") || s.terminalTabs.length > 0,
  );
  const { rightVisible } = useWorkspaceMotion();

  if (!selectedHostId) return null;

  return (
    <WorkspaceSidePanelInner
      selectedHostId={selectedHostId}
      shouldRenderBody={rightVisible || hasTerminalTab}
    />
  );
}

const WorkspaceSidePanelInner = memo(function WorkspaceSidePanelInner({
  selectedHostId,
  shouldRenderBody,
}: {
  selectedHostId: string;
  shouldRenderBody: boolean;
}) {
  const {
    shouldRightOpen: shouldOpen,
    panelWidthCss,
    rightVisible: panelVisible,
    isRightSettledOpen: isSettledOpen,
    isAnimating,
    rightTransform: panelTransform,
    rightTransition: panelTransition,
    isResizing: isResizingRight,
    setIsResizingRight,
    onRightTransitionEnd,
  } = useWorkspaceMotion();

  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const tabs = useOrbitSelector((s) => s.sidePanelTabs);
  const panelElRef = useRef<HTMLElement | null>(null);
  const activeTabId = useOrbitSelector((s) => s.activeSidePanelTabId);
  const hasStreamingSideQuestion = useOrbitSelector((s) => {
    const scope = sideQuestionScopeKey(s);
    const list = s.sideQuestionsByScope[scope];
    return Boolean(list?.some((e) => e.status === "streaming"));
  });
  const activeTerminalCount = useOrbitSelector(
    (s) => s.terminalTabs.filter((t) => t.hostId === selectedHostId).length,
  );
  const totalTerminalCount = useOrbitSelector((s) => s.terminalTabs.length);

  const resizeCleanupRef = useRef<(() => void) | null>(null);
  const mod = modSymbol();

  useEffect(() => {
    return () => {
      resizeCleanupRef.current?.();
    };
  }, []);

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;
  const hasTerminalTabInBar = tabs.some((t) => t.kind === "terminal");
  const hasAnyPtySession = totalTerminalCount > 0;
  const isTerminalVisible = panelVisible && activeTab?.kind === "terminal";

  const startResize = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if (e.detail >= 2) {
      e.preventDefault();
      return;
    }
    e.preventDefault();
    resizeCleanupRef.current?.();
    const startX = e.clientX;
    const startW = Math.round(panelElRef.current?.getBoundingClientRect().width ?? 480);
    let latestW = startW;
    let hasMoved = false;
    let rafId: number | null = null;

    const onMove = (ev: MouseEvent) => {
      const delta = startX - ev.clientX;
      if (!hasMoved) {
        if (Math.abs(delta) < 2) return;
        hasMoved = true;
        setIsResizingRight(true);
        document.body.style.cursor = "col-resize";
      }
      latestW = startW + delta;
      if (rafId === null) {
        rafId = window.requestAnimationFrame(() => {
          rafId = null;
          orbitActions.setSidePanelWidth(latestW, false);
        });
      }
    };
    const cleanup = () => {
      if (rafId !== null) {
        window.cancelAnimationFrame(rafId);
        rafId = null;
      }
      if (hasMoved) {
        orbitActions.setSidePanelWidth(latestW, true);
        setIsResizingRight(false);
        document.body.style.cursor = "";
      }
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", cleanup);
      window.removeEventListener("blur", cleanup);
      if (resizeCleanupRef.current === cleanup) {
        resizeCleanupRef.current = null;
      }
    };
    resizeCleanupRef.current = cleanup;
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", cleanup);
    window.addEventListener("blur", cleanup);
  };

  const handleEqualizeSidePanel = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    const leftEl = sidebarOpen ? document.querySelector<HTMLElement>(".orbit-panel-left") : null;
    const activeLeft = leftEl ? Math.round(leftEl.getBoundingClientRect().width) : 0;
    orbitActions.equalizeSidePanel(window.innerWidth, activeLeft);
  };

  return (
    <aside
      ref={panelElRef}
      inert={!shouldOpen}
      data-open={shouldOpen ? "true" : "false"}
      data-animating={isAnimating ? "true" : "false"}
      data-resizing={isResizingRight ? "true" : "false"}
      onTransitionEnd={onRightTransitionEnd}
      style={{
        width: panelWidthCss,
        transform: panelTransform,
        transition: panelTransition,
        visibility: panelVisible ? "visible" : "hidden",
      }}
      className={cn(
        "orbit-panel-right border-border bg-canvas absolute inset-y-0 right-0 z-20 flex h-full shrink-0 flex-col overflow-hidden border-l [contain:layout_style] select-none",
        !shouldOpen && "pointer-events-none",
      )}
    >
      {/* Left Resize Drag Handle (double-click to split 50/50 with main stage) */}
      {shouldOpen && (
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize side panel (double-click to split evenly)"
          onMouseDown={startResize}
          onDoubleClick={handleEqualizeSidePanel}
          className="group/sash absolute top-0 left-0 z-20 flex h-full w-[6px] cursor-col-resize justify-start"
        >
          <div className="group-hover/sash:bg-primary/45 group-active/sash:bg-primary/65 h-full w-[2px] transition-colors duration-150" />
        </div>
      )}

      {/* Transparent Drag Shield so embedded iframes / WebGL canvases never swallow resize events */}
      {isResizingRight && <div className="fixed inset-0 z-50 cursor-col-resize select-none" />}

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {shouldRenderBody && (
          <>
            {/* Flush 40px Two-State Side Panel Header Bar (No-Tab vs With-Tab) */}
      <div
        className="flex h-[40px] shrink-0 items-center justify-between gap-1.5 pl-2 pr-[44px] shadow-[inset_0_-1px_0_0_rgba(0,0,0,0.055)] dark:shadow-[inset_0_-1px_0_0_rgba(255,255,255,0.06)]"
      >
        {tabs.length > 0 ? (
          <div
            className="flex h-full min-w-0 flex-1 items-center gap-0.5 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
          >
            {tabs.map((tab) => {
              const isActive = tab.id === activeTab?.id;
              const showPulseDot =
                (tab.kind === "side_question" && hasStreamingSideQuestion) ||
                (tab.kind === "terminal" && activeTerminalCount > 0 && !isActive);

              return (
                <div
                  key={tab.id}
                  role="tab"
                  aria-selected={isActive}
                  title={tab.filePath || tab.url || tab.title}
                  onClick={() => orbitActions.focusSidePanelTab(tab.id)}
                  onMouseDown={(e) => {
                    if (e.button === 1) {
                      e.preventDefault();
                      orbitActions.closeSidePanelTab(tab.id);
                    }
                  }}
                  className={cn(
                    "group/stab relative inline-flex h-[28px] max-w-[160px] shrink-0 cursor-pointer items-center gap-1.5 rounded-[7px] px-2 text-[12px] transition-colors",
                    isActive
                      ? "bg-black/[0.055] text-foreground font-medium dark:bg-white/[0.08]"
                      : "text-muted-foreground hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.05]",
                  )}
                >
                  <SidePanelTabIcon tab={tab} />
                  <span className="truncate">{tab.title}</span>
                  {showPulseDot && (
                    <span
                      className={cn(
                        "h-1.5 w-1.5 shrink-0 rounded-full",
                        tab.kind === "side_question"
                          ? "bg-primary animate-pulse"
                          : "bg-emerald-500/80",
                      )}
                    />
                  )}
                  <button
                    type="button"
                    aria-label={`Close ${tab.title}`}
                    onMouseDown={(e) => e.stopPropagation()}
                    onClick={(e) => {
                      e.stopPropagation();
                      orbitActions.closeSidePanelTab(tab.id);
                    }}
                    className={cn(
                      "ml-0.5 inline-flex h-[16px] w-[16px] shrink-0 cursor-pointer items-center justify-center rounded transition-opacity hover:bg-black/[0.08] dark:hover:bg-white/[0.12]",
                      isActive || tab.kind === "subagent"
                        ? "opacity-65 hover:opacity-100"
                        : "opacity-0 group-hover/stab:opacity-65 hover:!opacity-100",
                    )}
                  >
                    <X className="h-[11px] w-[11px]" />
                  </button>
                </div>
              );
            })}

            <SimpleTooltip label="New Tab" shortcut={`${mod}T`}>
              <button
                type="button"
                aria-label="New Tab"
                onClick={() => orbitActions.openSidePanelNewTab()}
                className={cn(
                  "ml-0.5 inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors",
                  !activeTab
                    ? "bg-black/[0.06] text-foreground dark:bg-white/[0.09]"
                    : "text-muted-foreground hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
                )}
              >
                <Plus className="h-[15px] w-[15px] stroke-[1.8]" />
              </button>
            </SimpleTooltip>

            <div data-tauri-drag-region className="h-full min-w-4 flex-1" />
          </div>
        ) : (
          <div data-tauri-drag-region className="h-full min-w-0 flex-1" />
        )}
      </div>

      {/* Tab Body Area */}
      <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
        {/* Persistent WebGL Terminal Pool (mounted whenever terminal tab or live PTY exists) */}
        {(hasTerminalTabInBar || hasAnyPtySession) && (
          <TerminalTab selectedHostId={selectedHostId} visible={isTerminalVisible} />
        )}

        {/* Persistent Browser Preview Pool (kept alive across Side Panel tab switches) */}
        {tabs
          .filter((t) => t.kind === "browser")
          .map((browserTab) => (
            <BrowserTab
              key={browserTab.id}
              tab={browserTab}
              visible={panelVisible && activeTab?.id === browserTab.id}
            />
          ))}

        {activeTab?.kind === "review" && <ReviewTab selectedHostId={selectedHostId} />}
        {activeTab?.kind === "files" && <FilesExplorerTab />}
        {activeTab?.kind === "file" && <FileViewerTab key={activeTab.id} tab={activeTab} />}
        {activeTab?.kind === "side_question" && <SideQuestionTab />}
        {activeTab?.kind === "subagent" && <SubagentTab key={activeTab.id} tab={activeTab} />}
        {activeTab?.kind === "board" && (
          <WorkspaceBoardTab key={activeTab.id} tab={activeTab} />
        )}
        {activeTab?.kind === "peer" && <PeerSplitTab key={activeTab.id} tab={activeTab} />}

        {!activeTab && (
          <SidePanelLaunchpad
            selectedHostId={selectedHostId}
            isPanelOpen={shouldOpen}
            isPanelSettled={isSettledOpen}
          />
        )}
      </div>
          </>
        )}
      </div>
    </aside>
  );
});
