import { memo, useEffect, useRef, useState } from "react";
import {
  FileCode2,
  FolderTree,
  GitCompareArrows,
  Globe,
  Layers,
  MessageCircleQuestion,
  PanelRightClose,
  Plus,
  Search,
  SquareTerminal,
  X,
} from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { BrowserTab } from "@/components/side-panel/browser-tab";
import { FileViewerTab } from "@/components/side-panel/file-viewer-tab";
import { FilesExplorerTab } from "@/components/side-panel/files-explorer-tab";
import { ReviewTab } from "@/components/side-panel/review-tab";
import { SideQuestionTab } from "@/components/side-panel/side-question-tab";
import { SubagentTab } from "@/components/side-panel/subagent-tab";
import { TerminalTab } from "@/components/side-panel/terminal-tab";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
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
      return <GitCompareArrows className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "files":
      return <FolderTree className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
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
  }
}

export function WorkspaceSidePanel() {
  const sidePanelOpen = useOrbitSelector((s) => s.sidePanelOpen);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const hasTerminalTab = useOrbitSelector((s) =>
    s.sidePanelTabs.some((t) => t.kind === "terminal") || s.terminalTabs.length > 0,
  );

  const shouldOpen = sidePanelOpen && Boolean(selectedHostId);
  const [transitionsReady, setTransitionsReady] = useState(false);
  const [isClosing, setIsClosing] = useState(false);
  const [bodyMounted, setBodyMounted] = useState(shouldOpen || hasTerminalTab);
  const prevShouldOpenRef = useRef(shouldOpen);

  useEffect(() => {
    const raf = window.requestAnimationFrame(() => {
      setTransitionsReady(true);
    });
    return () => window.cancelAnimationFrame(raf);
  }, []);

  useEffect(() => {
    const wasOpen = prevShouldOpenRef.current;
    prevShouldOpenRef.current = shouldOpen;

    if (shouldOpen) {
      setIsClosing(false);
      return;
    }
    if (!wasOpen) return;

    setIsClosing(true);
    const id = window.setTimeout(() => {
      setIsClosing(false);
    }, 270);
    return () => window.clearTimeout(id);
  }, [shouldOpen]);

  useEffect(() => {
    if (shouldOpen || isClosing || hasTerminalTab) {
      setBodyMounted(true);
    } else {
      setBodyMounted(false);
    }
  }, [shouldOpen, isClosing, hasTerminalTab]);

  if (!selectedHostId) return null;

  return (
    <WorkspaceSidePanelInner
      selectedHostId={selectedHostId}
      shouldOpen={shouldOpen}
      transitionsReady={transitionsReady}
      bodyMounted={bodyMounted}
      isClosing={isClosing}
    />
  );
}

const WorkspaceSidePanelInner = memo(function WorkspaceSidePanelInner({
  selectedHostId,
  shouldOpen,
  transitionsReady,
  bodyMounted,
  isClosing,
}: {
  selectedHostId: string;
  shouldOpen: boolean;
  transitionsReady: boolean;
  bodyMounted: boolean;
  isClosing: boolean;
}) {
  const sidePanelWidth = useOrbitSelector((s) => s.sidePanelWidth);
  const tabs = useOrbitSelector((s) => s.sidePanelTabs);
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

  const [isResizing, setIsResizing] = useState(false);
  const resizeCleanupRef = useRef<(() => void) | null>(null);
  const mod = modSymbol();

  useEffect(() => {
    return () => {
      resizeCleanupRef.current?.();
    };
  }, []);

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? tabs[0] ?? null;
  const hasTerminalTabInBar = tabs.some((t) => t.kind === "terminal");
  const hasAnyPtySession = totalTerminalCount > 0;
  const isTerminalVisible = (shouldOpen || isClosing) && activeTab?.kind === "terminal";

  const startResize = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    resizeCleanupRef.current?.();
    const startX = e.clientX;
    const startW = sidePanelWidth;
    setIsResizing(true);
    document.body.style.cursor = "col-resize";

    const onMove = (ev: MouseEvent) => {
      const delta = startX - ev.clientX;
      orbitActions.setSidePanelWidth(startW + delta);
    };
    const cleanup = () => {
      setIsResizing(false);
      document.body.style.cursor = "";
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

  const effectiveWidthExpr = `min(${sidePanelWidth}px, 58vw)`;

  return (
    <aside
      inert={!shouldOpen}
      data-open={shouldOpen ? "true" : "false"}
      data-ready={transitionsReady ? "true" : "false"}
      data-resizing={isResizing ? "true" : "false"}
      style={{
        width: effectiveWidthExpr,
        marginRight: shouldOpen ? "0px" : `calc(-1 * ${effectiveWidthExpr})`,
        opacity: shouldOpen ? 1 : 0,
        visibility: shouldOpen || isClosing ? "visible" : "hidden",
      }}
      className={cn(
        "orbit-panel-right border-border bg-sidebar relative flex h-full shrink-0 flex-col overflow-hidden border-l select-none",
        !shouldOpen && "pointer-events-none",
      )}
    >
      {/* Left Resize Drag Handle */}
      {shouldOpen && (
        <div
          onMouseDown={startResize}
          className="hover:bg-primary/40 absolute top-0 left-0 z-20 h-full w-1 cursor-col-resize transition-colors duration-150"
        />
      )}

      {/* Transparent Drag Shield so embedded iframes / WebGL canvases never swallow resize events */}
      {isResizing && <div className="fixed inset-0 z-50 cursor-col-resize select-none" />}

      <div
        data-open={shouldOpen ? "true" : "false"}
        data-ready={transitionsReady ? "true" : "false"}
        data-resizing={isResizing ? "true" : "false"}
        className="orbit-panel-right-inner flex min-h-0 flex-1 flex-col overflow-hidden"
      >
        {bodyMounted && (
          <>
            {/* Flush 40px Multi-Tab Header Bar */}
      <div
        data-tauri-drag-region
        className="border-border flex h-[40px] shrink-0 items-center justify-between gap-1.5 border-b px-2"
      >
        <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
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
                  "group/stab relative inline-flex h-[28px] max-w-[148px] shrink-0 cursor-pointer items-center gap-1.5 rounded-[7px] px-2 text-[12px] transition-colors",
                  isActive
                    ? "bg-canvas text-foreground font-medium shadow-[0_1px_2px_rgba(0,0,0,0.04)] ring-1 ring-black/[0.06] dark:shadow-none dark:ring-white/[0.08]"
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
                  onClick={(e) => {
                    e.stopPropagation();
                    orbitActions.closeSidePanelTab(tab.id);
                  }}
                  className={cn(
                    "ml-0.5 inline-flex h-[16px] w-[16px] shrink-0 cursor-pointer items-center justify-center rounded transition-opacity hover:bg-black/[0.08] dark:hover:bg-white/[0.12]",
                    isActive
                      ? "opacity-55 hover:opacity-100"
                      : "opacity-0 group-hover/stab:opacity-65 hover:!opacity-100",
                  )}
                >
                  <X className="h-[11px] w-[11px]" />
                </button>
              </div>
            );
          })}
        </div>

        {/* Add Tab Menu & Collapse Panel */}
        <div className="flex shrink-0 items-center gap-0.5">
          <DropdownMenu>
            <SimpleTooltip label="New tab">
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  aria-label="New side panel tab"
                  className="text-muted-foreground hover:text-foreground inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-[7px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                >
                  <Plus className="h-[15px] w-[15px] stroke-[1.8]" />
                </button>
              </DropdownMenuTrigger>
            </SimpleTooltip>

            <DropdownMenuContent align="end" className="w-52">
              <DropdownMenuItem onSelect={() => orbitActions.setQuickOpenFilesOpen(true)}>
                <span className="flex w-full items-center justify-between gap-2">
                  <span className="flex items-center gap-2">
                    <Search className="text-muted-foreground h-[14px] w-[14px]" />
                    <span>Open File…</span>
                  </span>
                  <span className="text-muted-foreground font-mono text-[11px]">{mod}P</span>
                </span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => orbitActions.openFilesExplorerTab()}>
                <span className="flex items-center gap-2">
                  <FolderTree className="text-muted-foreground h-[14px] w-[14px]" />
                  <span>Files Explorer</span>
                </span>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={() => orbitActions.openReviewTab()}>
                <span className="flex w-full items-center justify-between gap-2">
                  <span className="flex items-center gap-2">
                    <GitCompareArrows className="text-muted-foreground h-[14px] w-[14px]" />
                    <span>Review (Diff)</span>
                  </span>
                  <span className="text-muted-foreground font-mono text-[11px]">{mod}D</span>
                </span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => orbitActions.openTerminalTab()}>
                <span className="flex w-full items-center justify-between gap-2">
                  <span className="flex items-center gap-2">
                    <SquareTerminal className="text-muted-foreground h-[14px] w-[14px]" />
                    <span>Terminal</span>
                  </span>
                  <span className="text-muted-foreground font-mono text-[11px]">{mod}J</span>
                </span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => orbitActions.openBrowserTab()}>
                <span className="flex items-center gap-2">
                  <Globe className="text-muted-foreground h-[14px] w-[14px]" />
                  <span>Browser</span>
                </span>
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => orbitActions.openSideQuestionTab()}>
                <span className="flex w-full items-center justify-between gap-2">
                  <span className="flex items-center gap-2">
                    <MessageCircleQuestion className="text-muted-foreground h-[14px] w-[14px]" />
                    <span>Side Question (/btw)</span>
                  </span>
                  <span className="text-muted-foreground font-mono text-[11px]">{mod};</span>
                </span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          <SimpleTooltip label="Hide side panel" shortcut={`${mod}\\`}>
            <button
              type="button"
              aria-label="Hide side panel"
              onClick={() => orbitActions.toggleSidePanel(false)}
              className="text-muted-foreground hover:text-foreground inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-[7px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
            >
              <PanelRightClose className="h-[15px] w-[15px] stroke-[1.75]" />
            </button>
          </SimpleTooltip>
        </div>
      </div>

      {/* Tab Body Area */}
      <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
        {/* Persistent WebGL Terminal Pool (mounted whenever terminal tab or live PTY exists) */}
        {(hasTerminalTabInBar || hasAnyPtySession) && (
          <TerminalTab selectedHostId={selectedHostId} visible={isTerminalVisible} />
        )}

        {activeTab?.kind === "review" && <ReviewTab selectedHostId={selectedHostId} />}
        {activeTab?.kind === "files" && <FilesExplorerTab />}
        {activeTab?.kind === "file" && <FileViewerTab key={activeTab.id} tab={activeTab} />}
        {activeTab?.kind === "browser" && <BrowserTab key={activeTab.id} tab={activeTab} />}
        {activeTab?.kind === "side_question" && <SideQuestionTab />}
        {activeTab?.kind === "subagent" && <SubagentTab key={activeTab.id} tab={activeTab} />}

            {!activeTab && (
              <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2 text-[12.5px]">
                <span>No open tabs.</span>
                <button
                  type="button"
                  onClick={() => orbitActions.openReviewTab()}
                  className="border-border hover:bg-accent text-foreground cursor-pointer rounded-lg border px-3 py-1 text-[12px] transition-colors"
                >
                  Open Diff Review
                </button>
              </div>
            )}
          </div>
          </>
        )}
      </div>
    </aside>
  );
});
