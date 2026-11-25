import { memo, useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import {
  Blocks,
  Columns2,
  File,
  FileCode2,
  FileDiff,
  FileText,
  Globe,
  MessageCircleQuestion,
  Plus,
  SquareTerminal,
  X,
} from "lucide-react";
import { ArtifactsTab } from "@/components/artifacts/artifacts-tab";
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
import { WorkspacePluginsPopover } from "@/components/side-panel/workspace-plugins-popover";
import { WorkspaceScriptsPopover } from "@/components/side-panel/scripts-panel";
import { SimpleTooltip } from "@/components/ui/overlays";
import { PanelErrorBoundary } from "@/components/ui/panel-error-boundary";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
import type {
  HostCardView,
  HostDetailView,
  SessionSubagentEntryView,
} from "@/lib/orbit-ipc";
import { getSubagentCharacterById } from "@/lib/subagent-avatar";
import type { ResolvedSubagentStatus } from "@/lib/subagents";
import {
  hasCrossHostChildAgents,
  selectSessionSubagents,
} from "@/state/selectors/session-selectors";
import { type MessageKey, useI18n } from "@/lib/i18n";
import { useActionShortcutDisplay } from "@/lib/keybindings";
import {
  type SidePanelTab,
  fileCacheKey,
  orbitActions,
  sideQuestionScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface TabDropIndicator {
  targetTabId: string;
  position: "before" | "after";
}

const EMPTY_INSIGHT_SUBAGENTS: readonly SessionSubagentEntryView[] = [];
const EMPTY_HOST_DETAILS: Record<string, HostDetailView> = {};
const EMPTY_FLEET_HOSTS: readonly HostCardView[] = [];

const FileTabDirtyIndicator = memo(function FileTabDirtyIndicator({
  hostId,
  cwd,
  filePath,
  title,
}: {
  hostId: string;
  cwd: string;
  filePath: string;
  title: string;
}) {
  const key = fileCacheKey(hostId, cwd, filePath);
  const isDirty = useOrbitSelector((s) => Boolean(s.fileEditorDrafts[key]?.dirty));
  if (!isDirty) return null;
  return (
    <span
      title={title}
      className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-500/90"
    />
  );
});

const SubagentSidePanelTabAvatar = memo(function SubagentSidePanelTabAvatar({
  tab,
}: {
  tab: SidePanelTab;
}) {
  const hostId = useOrbitSelector((s) => s.selectedHostId);
  const parentAgentId = useOrbitSelector((s) => tab.parentAgentId ?? s.selectedAgentId);
  const insightSubagents = useOrbitSelector((s) =>
    hostId && parentAgentId
      ? (s.stages[`${hostId}::${parentAgentId}`]?.insights?.subagents ?? EMPTY_INSIGHT_SUBAGENTS)
      : EMPTY_INSIGHT_SUBAGENTS,
  );
  const avatarKey = useOrbitSelector((s) => {
    if (!hostId || !parentAgentId) return "";
    const hasCrossHost = hasCrossHostChildAgents(
      s.hostDetails,
      hostId,
      parentAgentId,
      insightSubagents,
    );
    const localDetail = s.hostDetails[hostId];
    const scopedHostDetails = hasCrossHost
      ? s.hostDetails
      : localDetail
        ? { [hostId]: localDetail }
        : EMPTY_HOST_DETAILS;
    const summary = selectSessionSubagents(
      {
        hostDetails: scopedHostDetails,
        fleet: {
          revision: 0,
          total_pending_approvals: 0,
          hosts: (hasCrossHost ? s.fleet.hosts : EMPTY_FLEET_HOSTS) as HostCardView[],
          projects: [],
        },
        archivedSubagents: s.archivedSubagents,
      },
      hostId,
      parentAgentId,
      insightSubagents,
    );
    const matched = summary.all.find(
      (sa) =>
        (tab.childAgentId && sa.childAgentId === tab.childAgentId) ||
        (tab.providerSubagentId && sa.providerSubagentId === tab.providerSubagentId) ||
        (tab.subagentCallId && sa.callId === tab.subagentCallId),
    );
    if (!matched) return "";
    return `${matched.character.id}\0${matched.status}`;
  });

  const sepIdx = avatarKey.indexOf("\0");
  const character =
    sepIdx > 0 ? (getSubagentCharacterById(avatarKey.slice(0, sepIdx)) ?? undefined) : undefined;
  const status = (sepIdx >= 0 ? avatarKey.slice(sepIdx + 1) : "completed") as ResolvedSubagentStatus;

  const fallbackSeed =
    tab.childAgentId || tab.providerSubagentId || tab.subagentCallId || tab.title;

  return (
    <SubagentAvatar
      character={character}
      seedKey={fallbackSeed}
      status={status}
      size="xs"
    />
  );
});

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
      return <SubagentSidePanelTabAvatar tab={tab} />;
    case "board":
      return <FileText className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "peer":
      return <Columns2 className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
    case "artifacts":
      return <Blocks className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />;
  }
}

function resolveSidePanelTabTitle(
  tab: SidePanelTab,
  t: (key: MessageKey) => string,
): string {
  switch (tab.kind) {
    case "review":
      return tab.title === "Review" ? t("sidePanel.actionReview") : tab.title;
    case "files":
      return tab.title === "Files" ? t("composer.files") : tab.title;
    case "artifacts":
      return tab.title === "Artifacts" ? t("sidePanel.actionArtifacts") : tab.title;
    case "terminal":
      return tab.title === "Terminal" ? t("sidePanel.actionTerminal") : tab.title;
    case "browser":
      return tab.title === "Browser" ? t("sidePanel.actionBrowser") : tab.title;
    case "side_question":
      return tab.title === "Side Question" ? t("sidePanel.actionSideQuestion") : tab.title;
    default:
      return tab.title;
  }
}

function renderTransientTabBody(
  tab: SidePanelTab,
  selectedHostId: string,
  selectedWorkspaceCwd: string,
  paneStyle: CSSProperties | undefined,
  paneClassName: string,
) {
  switch (tab.kind) {
    case "files":
      return (
        <div key={tab.id} style={paneStyle} className={paneClassName}>
          <PanelErrorBoundary
            panelName="Files"
            resetKey={`${selectedHostId}:${selectedWorkspaceCwd ?? ""}`}
          >
            <FilesExplorerTab />
          </PanelErrorBoundary>
        </div>
      );
    case "file":
      return (
        <div
          key={`${tab.id}:${tab.filePath ?? ""}`}
          style={paneStyle}
          className={paneClassName}
        >
          <PanelErrorBoundary
            key={`${tab.id}:${tab.filePath ?? ""}`}
            panelName="File Viewer"
          >
            <FileViewerTab tab={tab} />
          </PanelErrorBoundary>
        </div>
      );
    case "side_question":
      return (
        <div key={tab.id} style={paneStyle} className={paneClassName}>
          <PanelErrorBoundary
            panelName="Side Question"
            resetKey={`${selectedHostId}:${tab.id}`}
          >
            <SideQuestionTab />
          </PanelErrorBoundary>
        </div>
      );
    case "subagent":
      return (
        <div key={tab.id} style={paneStyle} className={paneClassName}>
          <PanelErrorBoundary key={tab.id} panelName="Subagent">
            <SubagentTab tab={tab} />
          </PanelErrorBoundary>
        </div>
      );
    case "peer":
      return (
        <div key={tab.id} style={paneStyle} className={paneClassName}>
          <PanelErrorBoundary key={tab.id} panelName="Peer Split">
            <PeerSplitTab tab={tab} />
          </PanelErrorBoundary>
        </div>
      );
    case "artifacts":
      return (
        <div key={tab.id} style={paneStyle} className={paneClassName}>
          <PanelErrorBoundary key={tab.id} panelName="Artifacts">
            <ArtifactsTab tab={tab} />
          </PanelErrorBoundary>
        </div>
      );
    default:
      return null;
  }
}

export function WorkspaceSidePanel() {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const hasPersistentTab = useOrbitSelector(
    (s) =>
      s.terminalTabs.length > 0 ||
      s.sidePanelTabs.some(
        (t) => t.kind === "terminal" || t.kind === "review" || t.kind === "board",
      ),
  );
  const { rightVisible } = useWorkspaceMotion();

  if (!selectedHostId) return null;

  return (
    <WorkspaceSidePanelInner
      selectedHostId={selectedHostId}
      shouldRenderBody={rightVisible || hasPersistentTab}
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
  const { t } = useI18n();
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
  const selectedWorkspaceCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const tabs = useOrbitSelector((s) => s.sidePanelTabs);
  const sidePanelSplit = useOrbitSelector((s) => s.sidePanelSplit);
  const activeTabId = useOrbitSelector((s) => s.activeSidePanelTabId);
  const newTabShortcut = useActionShortcutDisplay("new_side_tab");

  const panelElRef = useRef<HTMLElement | null>(null);
  const bodyElRef = useRef<HTMLDivElement | null>(null);
  const tabStripRef = useRef<HTMLDivElement | null>(null);
  const tabElRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const suppressNextTabClickRef = useRef(false);
  const tabDragCleanupRef = useRef<(() => void) | null>(null);
  const splitResizeCleanupRef = useRef<(() => void) | null>(null);

  const [draggingTabId, setDraggingTabId] = useState<string | null>(null);
  const [tabDropIndicator, setTabDropIndicator] = useState<TabDropIndicator | null>(null);
  const [isResizingSplit, setIsResizingSplit] = useState(false);

  const hasStreamingSideQuestion = useOrbitSelector((s) => {
    const scope = sideQuestionScopeKey(s);
    const list = s.sideQuestionsByScope[scope];
    return Boolean(list?.some((e) => e.status === "streaming"));
  });
  const activeTerminalCount = useOrbitSelector(
    (s) => s.terminalTabs.filter((t) => t.hostId === selectedHostId).length,
  );
  const totalTerminalCount = useOrbitSelector((s) => s.terminalTabs.length);
  const artifactCount = useOrbitSelector((s) => {
    const detail = s.hostDetails[selectedHostId];
    const all = detail?.artifacts ?? [];
    if (all.length === 0) return 0;
    const activeAg =
      !s.isDraftNewSession && s.selectedAgentId
        ? detail?.agents.find((a) => a.agent_id === s.selectedAgentId)
        : null;
    if (activeAg?.workspace_id) {
      const wsCount = all.filter((a) => a.workspace_id === activeAg.workspace_id).length;
      if (wsCount > 0) return wsCount;
    }
    return all.length;
  });

  const resizeCleanupRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    return () => {
      resizeCleanupRef.current?.();
      tabDragCleanupRef.current?.();
      splitResizeCleanupRef.current?.();
    };
  }, []);

  // Keep the active side-panel tab scrolled into view when multiple tabs overflow horizontally
  useEffect(() => {
    if (!activeTabId || !tabStripRef.current) return;
    const activeEl = tabStripRef.current.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
    const prefersReducedMotion =
      typeof window !== "undefined" &&
      Boolean(window.matchMedia?.("(prefers-reduced-motion: reduce)").matches);
    activeEl?.scrollIntoView({
      block: "nearest",
      inline: "nearest",
      behavior: prefersReducedMotion ? "auto" : "smooth",
    });
  }, [activeTabId, tabs.length]);

  const activeTab = tabs.find((t) => t.id === activeTabId) ?? null;
  const secondaryTab =
    sidePanelSplit && activeTab && sidePanelSplit.secondaryTabId !== activeTab.id
      ? (tabs.find((t) => t.id === sidePanelSplit.secondaryTabId) ?? null)
      : null;
  const isSplitActive = Boolean(activeTab && secondaryTab);
  const splitRatio = sidePanelSplit?.ratio ?? 0.5;

  const primaryPaneStyle: CSSProperties | undefined = isSplitActive
    ? { order: 1, flex: `0 0 ${splitRatio * 100}%`, minWidth: 0 }
    : undefined;
  const secondaryPaneStyle: CSSProperties | undefined = isSplitActive
    ? { order: 3, flex: `1 1 ${(1 - splitRatio) * 100}%`, minWidth: 0 }
    : undefined;

  const paneSlotForTab = (tabId: string): "primary" | "secondary" | null => {
    if (!panelVisible) return null;
    if (activeTab?.id === tabId) return "primary";
    if (secondaryTab?.id === tabId) return "secondary";
    return null;
  };

  const hasTerminalTabInBar = tabs.some((t) => t.kind === "terminal");
  const hasAnyPtySession = totalTerminalCount > 0;
  const terminalTab = tabs.find((t) => t.kind === "terminal") ?? null;
  const terminalSlot = terminalTab ? paneSlotForTab(terminalTab.id) : null;
  const isTerminalVisible = terminalSlot !== null;

  const hasReviewTab = tabs.some((t) => t.kind === "review");
  const reviewTab = tabs.find((t) => t.kind === "review") ?? null;
  const reviewSlot = reviewTab ? paneSlotForTab(reviewTab.id) : null;
  const isReviewVisible = reviewSlot !== null;

  const boardTabs = tabs.filter((t) => t.kind === "board");

  // Pointer-driven zero-rerender horizontal tab drag-reordering (works cleanly in macOS WKWebView)
  const startTabDrag = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, sourceTabId: string) => {
      if (e.button !== 0 || tabs.length < 2) return;
      if ((e.target as HTMLElement | null)?.closest("button")) return;

      tabDragCleanupRef.current?.();
      const startX = e.clientX;
      let hasMoved = false;
      let latestIndicator: TabDropIndicator | null = null;

      const resolveDropTarget = (clientX: number): TabDropIndicator | null => {
        let best: { id: string; pos: "before" | "after"; dist: number } | null = null;
        for (const [id, el] of tabElRefs.current.entries()) {
          if (id === sourceTabId || !el.isConnected) continue;
          const rect = el.getBoundingClientRect();
          const mid = rect.left + rect.width * 0.5;
          const dist = Math.abs(clientX - mid);
          if (best === null || dist < best.dist) {
            best = { id, pos: clientX < mid ? "before" : "after", dist };
          }
        }
        return best ? { targetTabId: best.id, position: best.pos } : null;
      };

      const onMove = (ev: PointerEvent) => {
        if (!hasMoved) {
          if (Math.abs(ev.clientX - startX) < 5) return;
          hasMoved = true;
          setDraggingTabId(sourceTabId);
          document.body.style.cursor = "grabbing";
        }
        const next = resolveDropTarget(ev.clientX);
        if (
          next?.targetTabId !== latestIndicator?.targetTabId ||
          next?.position !== latestIndicator?.position
        ) {
          latestIndicator = next;
          setTabDropIndicator(next);
        }
      };

      const cleanup = () => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onUp);
        window.removeEventListener("pointercancel", onCancel);
        window.removeEventListener("blur", onCancel);
        document.body.style.cursor = "";
        setDraggingTabId(null);
        setTabDropIndicator(null);
        if (tabDragCleanupRef.current === cleanup) {
          tabDragCleanupRef.current = null;
        }
      };

      const onUp = () => {
        if (hasMoved) {
          suppressNextTabClickRef.current = true;
          if (latestIndicator) {
            orbitActions.reorderSidePanelTabs(
              sourceTabId,
              latestIndicator.targetTabId,
              latestIndicator.position,
            );
          }
          window.requestAnimationFrame(() => {
            suppressNextTabClickRef.current = false;
          });
        }
        cleanup();
      };

      const onCancel = () => {
        cleanup();
      };

      tabDragCleanupRef.current = cleanup;
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onUp);
      window.addEventListener("pointercancel", onCancel);
      window.addEventListener("blur", onCancel);
    },
    [tabs.length],
  );

  const startSplitResize = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.button !== 0 || !bodyElRef.current) return;
    e.preventDefault();
    splitResizeCleanupRef.current?.();
    const rect = bodyElRef.current.getBoundingClientRect();
    if (rect.width <= 0) return;

    setIsResizingSplit(true);
    document.body.style.cursor = "col-resize";

    let rafId: number | null = null;
    let latestRatio = splitRatio;

    const onMove = (ev: MouseEvent) => {
      latestRatio = (ev.clientX - rect.left) / rect.width;
      if (rafId === null) {
        rafId = window.requestAnimationFrame(() => {
          rafId = null;
          orbitActions.setSidePanelSplitRatio(latestRatio);
        });
      }
    };

    const cleanup = () => {
      if (rafId !== null) {
        window.cancelAnimationFrame(rafId);
        rafId = null;
      }
      setIsResizingSplit(false);
      document.body.style.cursor = "";
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", cleanup);
      window.removeEventListener("blur", cleanup);
      if (splitResizeCleanupRef.current === cleanup) {
        splitResizeCleanupRef.current = null;
      }
    };

    splitResizeCleanupRef.current = cleanup;
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", cleanup);
    window.addEventListener("blur", cleanup);
  };

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

  const styleForSlot = (slot: "primary" | "secondary" | null): CSSProperties | undefined => {
    if (!isSplitActive || !slot) return undefined;
    return slot === "primary" ? primaryPaneStyle : secondaryPaneStyle;
  };

  const transientClass = "orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden";

  return (
    <aside
      ref={panelElRef}
      tabIndex={-1}
      data-orbit-side-panel="true"
      inert={!shouldOpen}
      data-open={shouldOpen ? "true" : "false"}
      data-animating={isAnimating ? "true" : "false"}
      data-resizing={isResizingRight || isResizingSplit ? "true" : "false"}
      onTransitionEnd={onRightTransitionEnd}
      style={{
        width: panelWidthCss,
        transform: panelTransform,
        transition: panelTransition,
        visibility: panelVisible ? "visible" : "hidden",
      }}
      className={cn(
        "orbit-panel-right border-border-subtle bg-surface-base absolute inset-y-0 right-0 z-20 flex h-full shrink-0 flex-col overflow-hidden border-l outline-none [contain:layout_style] select-none",
        !shouldOpen && "pointer-events-none",
      )}
    >
      {/* Left Resize Drag Handle (double-click to split 50/50 with main stage) */}
      {shouldOpen && (
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label={t("sidePanel.resizeAria")}
          onMouseDown={startResize}
          onDoubleClick={handleEqualizeSidePanel}
          className="group/sash absolute top-0 left-0 z-20 flex h-full w-[6px] cursor-col-resize justify-start"
        >
          <div className="group-hover/sash:bg-border-strong group-active/sash:bg-ink-muted h-full w-[1.5px] transition-colors duration-150" />
        </div>
      )}

      {/* Transparent Drag Shield so embedded iframes / WebGL canvases never swallow resize events */}
      {(isResizingRight || isResizingSplit) && (
        <div className="fixed inset-0 z-50 cursor-col-resize select-none" />
      )}

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {shouldRenderBody && (
          <>
            {/* Flush 40px Two-State Side Panel Header Bar (No-Tab vs With-Tab) */}
            <div className="@container border-border-subtle flex h-[40px] min-w-0 shrink-0 items-center gap-1 border-b pr-[44px] pl-2">
              {tabs.length > 0 ? (
                <>
                  <div
                    ref={tabStripRef}
                    role="tablist"
                    onWheel={(e) => {
                      const el = tabStripRef.current;
                      if (!el || el.scrollWidth <= el.clientWidth) return;
                      if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
                        el.scrollLeft += e.deltaY;
                      }
                    }}
                    className="flex h-full min-w-0 items-center gap-0.5 overflow-x-auto pr-2.5 [mask-image:linear-gradient(to_right,black_calc(100%-10px),transparent)] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
                  >
                    {tabs.map((tab) => {
                      const isPrimary = tab.id === activeTab?.id;
                      const isSecondary = tab.id === secondaryTab?.id;
                      const isActive = isPrimary || isSecondary;
                      const showPulseDot =
                        (tab.kind === "side_question" && hasStreamingSideQuestion) ||
                        (tab.kind === "terminal" && activeTerminalCount > 0 && !isActive);

                      const displayTitle = resolveSidePanelTabTitle(tab, t);
                      const dropSide =
                        tabDropIndicator?.targetTabId === tab.id ? tabDropIndicator.position : null;

                      return (
                        <div
                          key={tab.id}
                          ref={(el) => {
                            if (el) tabElRefs.current.set(tab.id, el);
                            else tabElRefs.current.delete(tab.id);
                          }}
                          role="tab"
                          aria-selected={isPrimary}
                          title={tab.filePath || tab.url || displayTitle}
                          onPointerDown={(e) => startTabDrag(e, tab.id)}
                          onClick={(e) => {
                            if (suppressNextTabClickRef.current) return;
                            if (e.altKey && tabs.length >= 2) {
                              orbitActions.setSidePanelSecondaryTab(tab.id);
                            } else {
                              orbitActions.focusSidePanelTab(tab.id);
                            }
                          }}
                          onMouseDown={(e) => {
                            if (e.button === 1) {
                              e.preventDefault();
                              orbitActions.closeSidePanelTab(tab.id);
                            }
                          }}
                          className={cn(
                            "group/stab relative inline-flex h-[28px] max-w-[116px] shrink-0 cursor-pointer items-center gap-1.5 rounded-[7px] px-2 text-[12px] transition-colors @[380px]:max-w-[144px] @[480px]:max-w-[168px]",
                            draggingTabId === tab.id && "opacity-45",
                            isPrimary
                              ? "bg-black/[0.055] text-foreground font-medium dark:bg-white/[0.08]"
                              : isSecondary
                                ? "border-border-default bg-surface-recessed text-ink-primary border font-medium"
                                : "text-muted-foreground hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.05]",
                          )}
                        >
                          {dropSide === "before" && (
                            <span className="bg-signal-info pointer-events-none absolute top-1 bottom-1 -left-[1px] z-20 w-[2px] rounded-full" />
                          )}
                          {dropSide === "after" && (
                            <span className="bg-signal-info pointer-events-none absolute top-1 -right-[1px] bottom-1 z-20 w-[2px] rounded-full" />
                          )}
                          <SidePanelTabIcon tab={tab} />
                          <span className="min-w-0 truncate">{displayTitle}</span>
                          {tab.kind === "file" && tab.filePath && (
                            <FileTabDirtyIndicator
                              hostId={tab.hostId ?? selectedHostId}
                              cwd={tab.cwd ?? selectedWorkspaceCwd ?? "~"}
                              filePath={tab.filePath}
                              title={t("sidePanel.unsavedChanges")}
                            />
                          )}
                          {tab.kind === "artifacts" && artifactCount > 0 && (
                            <span className="text-muted-foreground/75 hidden shrink-0 text-[10.5px] tabular-nums @[380px]:inline">
                              {artifactCount}
                            </span>
                          )}
                          {showPulseDot && (
                            <span
                              className={cn(
                                "h-1.5 w-1.5 shrink-0 rounded-full",
                                tab.kind === "side_question"
                                  ? "bg-signal-info animate-pulse"
                                  : "bg-emerald-500/80",
                              )}
                            />
                          )}
                          <button
                            type="button"
                            aria-label={t("sidePanel.closeTab", { title: displayTitle })}
                            onMouseDown={(e) => e.stopPropagation()}
                            onPointerDown={(e) => e.stopPropagation()}
                            onClick={(e) => {
                              e.stopPropagation();
                              orbitActions.closeSidePanelTab(tab.id);
                            }}
                            className={cn(
                              "ml-0.5 inline-flex h-[16px] w-[16px] shrink-0 cursor-pointer items-center justify-center rounded transition-opacity hover:bg-black/[0.08] dark:hover:bg-white/[0.12]",
                              isActive
                                ? "opacity-65 hover:opacity-100"
                                : "opacity-0 group-hover/stab:opacity-65 hover:!opacity-100",
                            )}
                          >
                            <X className="h-[11px] w-[11px]" />
                          </button>
                        </div>
                      );
                    })}
                  </div>

                  <SimpleTooltip label={t("sidePanel.newTab")} shortcut={newTabShortcut}>
                    <button
                      type="button"
                      aria-label={t("sidePanel.newTab")}
                      onClick={() => orbitActions.openSidePanelNewTab()}
                      className={cn(
                        "inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-[7px] transition-colors",
                        !activeTab
                          ? "bg-black/[0.06] text-foreground dark:bg-white/[0.09]"
                          : "text-muted-foreground hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
                      )}
                    >
                      <Plus className="h-[15px] w-[15px] stroke-[1.8]" />
                    </button>
                  </SimpleTooltip>

                  {tabs.length >= 2 && (
                    <SimpleTooltip
                      label={
                        isSplitActive ? t("sidePanel.unsplitPane") : t("sidePanel.splitPane")
                      }
                    >
                      <button
                        type="button"
                        aria-label={
                          isSplitActive ? t("sidePanel.unsplitPane") : t("sidePanel.splitPane")
                        }
                        aria-pressed={isSplitActive}
                        onClick={() =>
                          isSplitActive
                            ? orbitActions.closeSidePanelSplit()
                            : orbitActions.splitSidePanelTab()
                        }
                        className={cn(
                          "inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-[7px] transition-colors",
                          isSplitActive
                            ? "bg-black/[0.065] text-foreground dark:bg-white/[0.1]"
                            : "text-muted-foreground hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
                        )}
                      >
                        <Columns2 className="h-[14px] w-[14px] stroke-[1.75]" />
                      </button>
                    </SimpleTooltip>
                  )}

                  <div data-tauri-drag-region className="h-full min-w-2 flex-1" />
                </>
              ) : (
                <div data-tauri-drag-region className="h-full min-w-0 flex-1" />
              )}
              {tabs.length > 0 && <WorkspacePluginsPopover hostId={selectedHostId} />}
              {tabs.length > 0 && <WorkspaceScriptsPopover hostId={selectedHostId} />}
            </div>

            {/* Tab Body Area (Single-pane flex-col OR Dual-split flex-row with zero teardown!) */}
            <div
              ref={bodyElRef}
              className={cn(
                "relative flex min-h-0 flex-1 overflow-hidden",
                isSplitActive ? "flex-row" : "flex-col",
              )}
            >
              {/* Persistent WebGL Terminal Pool (never unmounts on tab switch or split toggle!) */}
              {(hasTerminalTabInBar || hasAnyPtySession) && (
                <div
                  style={styleForSlot(terminalSlot)}
                  className={cn(
                    "min-h-0 min-w-0 flex-col overflow-hidden",
                    isTerminalVisible ? "flex flex-1" : "hidden",
                  )}
                >
                  <PanelErrorBoundary panelName="Terminal" resetKey={selectedHostId}>
                    <TerminalTab selectedHostId={selectedHostId} visible={isTerminalVisible} />
                  </PanelErrorBoundary>
                </div>
              )}

              {/* Persistent Browser Preview Pool */}
              {tabs
                .filter((t) => t.kind === "browser")
                .map((browserTab) => {
                  const slot = paneSlotForTab(browserTab.id);
                  const browserVisible = slot !== null;
                  return (
                    <div
                      key={browserTab.id}
                      style={styleForSlot(slot)}
                      className={cn(
                        "min-h-0 min-w-0 flex-col overflow-hidden",
                        browserVisible ? "flex flex-1" : "hidden",
                      )}
                    >
                      <PanelErrorBoundary
                        panelName="Browser"
                        resetKey={browserTab.url ?? ""}
                      >
                        <BrowserTab tab={browserTab} visible={browserVisible} />
                      </PanelErrorBoundary>
                    </div>
                  );
                })}

              {/* Persistent Review Pool */}
              {hasReviewTab && (
                <div
                  style={styleForSlot(reviewSlot)}
                  className={cn(
                    "min-h-0 min-w-0 flex-col overflow-hidden",
                    isReviewVisible ? "flex flex-1" : "hidden",
                  )}
                >
                  <PanelErrorBoundary
                    panelName="Review"
                    resetKey={`${selectedHostId}:${selectedWorkspaceCwd ?? ""}`}
                  >
                    <ReviewTab
                      key={`${selectedHostId}:${selectedWorkspaceCwd ?? ""}`}
                      selectedHostId={selectedHostId}
                      visible={isReviewVisible}
                    />
                  </PanelErrorBoundary>
                </div>
              )}

              {/* Persistent Workspace Board Pool */}
              {boardTabs.map((boardTab) => {
                const slot = paneSlotForTab(boardTab.id);
                const isBoardVisible = slot !== null;
                return (
                  <div
                    key={boardTab.id}
                    style={styleForSlot(slot)}
                    className={cn(
                      "min-h-0 min-w-0 flex-col overflow-hidden",
                      isBoardVisible ? "flex flex-1" : "hidden",
                    )}
                  >
                    <PanelErrorBoundary panelName="Workspace Notes">
                      <WorkspaceBoardTab tab={boardTab} visible={isBoardVisible} />
                    </PanelErrorBoundary>
                  </div>
                );
              })}

              {/* Resizable Dual-Split Center Divider (order: 2 between Primary=1 and Secondary=3) */}
              {isSplitActive && (
                <div
                  role="separator"
                  aria-orientation="vertical"
                  aria-label={t("sidePanel.resizeSplitAria")}
                  onMouseDown={startSplitResize}
                  onDoubleClick={() => orbitActions.setSidePanelSplitRatio(0.5)}
                  style={{ order: 2 }}
                  className="group/split-sash border-border-subtle relative flex w-[5px] shrink-0 cursor-col-resize items-center justify-center border-x bg-transparent transition-colors hover:bg-black/[0.04] dark:hover:bg-white/[0.05]"
                >
                  <div className="bg-border-default group-hover/split-sash:bg-border-strong h-8 w-[1.5px] rounded-full transition-colors" />
                </div>
              )}

              {/* Transient Primary Tab Body */}
              {activeTab &&
                renderTransientTabBody(
                  activeTab,
                  selectedHostId,
                  selectedWorkspaceCwd,
                  primaryPaneStyle,
                  transientClass,
                )}

              {/* Transient Secondary Tab Body (when dual split active) */}
              {secondaryTab &&
                renderTransientTabBody(
                  secondaryTab,
                  selectedHostId,
                  selectedWorkspaceCwd,
                  secondaryPaneStyle,
                  transientClass,
                )}

              {!activeTab && (
                <PanelErrorBoundary panelName="Side Panel" resetKey={selectedHostId}>
                  <SidePanelLaunchpad
                    selectedHostId={selectedHostId}
                    isPanelOpen={shouldOpen}
                    isPanelSettled={isSettledOpen}
                  />
                </PanelErrorBoundary>
              )}
            </div>
          </>
        )}
      </div>
    </aside>
  );
});
