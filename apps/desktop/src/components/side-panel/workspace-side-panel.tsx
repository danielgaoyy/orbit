import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
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
import { DragShield } from "@/components/ui/drag-feedback";
import { SimpleTooltip } from "@/components/ui/overlays";
import { PanelErrorBoundary } from "@/components/ui/panel-error-boundary";
import { ResizeSash } from "@/components/ui/resize-sash";
import { SubagentAvatar, type SubagentAvatarStatus } from "@/components/ui/subagent-avatar";
import { mapAgentCardToItemStatus } from "@/lib/agent-list-item";
import { useInlineSortable } from "@/lib/drag/use-inline-sortable";
import type {
  HostCardView,
  HostDetailView,
  SessionSubagentEntryView,
} from "@/lib/orbit-ipc";
import { getSubagentCharacterById } from "@/lib/subagent-avatar";
import { mapManagedStatus } from "@/lib/subagents";
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
  sidePanelTabScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

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
  const hostId = tab.parentHostId ?? tab.hostId ?? null;
  const parentAgentId = tab.parentAgentId ?? tab.agentId ?? null;
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
    if (matched) {
      return `${matched.character.id}\0${matched.status}`;
    }
    if (tab.hostId && tab.childAgentId) {
      const directAgent = s.hostDetails[tab.hostId]?.agents.find(
        (a) => a.agent_id === tab.childAgentId,
      );
      if (directAgent) {
        const directStatus = directAgent.parent_agent_id
          ? mapManagedStatus(directAgent)
          : mapAgentCardToItemStatus(directAgent);
        return `\0${directStatus}`;
      }
    }
    return "";
  });

  const sepIdx = avatarKey.indexOf("\0");
  const character =
    sepIdx > 0 ? (getSubagentCharacterById(avatarKey.slice(0, sepIdx)) ?? undefined) : undefined;
  const status = (sepIdx >= 0 ? avatarKey.slice(sepIdx + 1) : "completed") as SubagentAvatarStatus;

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
  scopeResetPrefix: string,
  paneStyle: CSSProperties | undefined,
  paneClassName: string,
) {
  switch (tab.kind) {
    case "files":
      return (
        <div key={`${scopeResetPrefix}:${tab.id}`} style={paneStyle} className={paneClassName}>
          <PanelErrorBoundary
            panelName="Files"
            resetKey={`${scopeResetPrefix}:${tab.hostId ?? ""}:${tab.cwd ?? ""}`}
          >
            <FilesExplorerTab tab={tab} />
          </PanelErrorBoundary>
        </div>
      );
    case "file":
      return (
        <div
          key={`${scopeResetPrefix}:${tab.id}:${tab.filePath ?? ""}`}
          style={paneStyle}
          className={paneClassName}
        >
          <PanelErrorBoundary
            key={`${scopeResetPrefix}:${tab.id}:${tab.filePath ?? ""}`}
            panelName="File Viewer"
            resetKey={`${scopeResetPrefix}:${tab.id}:${tab.filePath ?? ""}`}
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
            resetKey={`${scopeResetPrefix}:${tab.id}`}
          >
            <SideQuestionTab tab={tab} />
          </PanelErrorBoundary>
        </div>
      );
    case "subagent":
      return (
        <div key={tab.id} style={paneStyle} className={paneClassName}>
          <PanelErrorBoundary
            key={`${scopeResetPrefix}:${tab.id}`}
            panelName="Subagent"
            resetKey={`${scopeResetPrefix}:${tab.id}`}
          >
            <SubagentTab tab={tab} />
          </PanelErrorBoundary>
        </div>
      );
    case "peer":
      return (
        <div key={tab.id} style={paneStyle} className={paneClassName}>
          <PanelErrorBoundary
            key={`${scopeResetPrefix}:${tab.id}`}
            panelName="Peer Split"
            resetKey={`${scopeResetPrefix}:${tab.id}`}
          >
            <PeerSplitTab tab={tab} />
          </PanelErrorBoundary>
        </div>
      );
    case "artifacts":
      return (
        <div key={tab.id} style={paneStyle} className={paneClassName}>
          <PanelErrorBoundary
            key={`${scopeResetPrefix}:${tab.id}`}
            panelName="Artifacts"
            resetKey={`${scopeResetPrefix}:${tab.id}`}
          >
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
  const sidePanelScopeKey = useOrbitSelector((s) => s.sidePanelScopeKey);
  const scopeResetPrefix = sidePanelScopeKey ?? "";
  const tabs = useOrbitSelector((s) => s.sidePanelTabs);
  const sidePanelSplit = useOrbitSelector((s) => s.sidePanelSplit);
  const activeTabId = useOrbitSelector((s) => s.activeSidePanelTabId);
  const newTabShortcut = useActionShortcutDisplay("new_side_tab");

  const panelElRef = useRef<HTMLElement | null>(null);
  const bodyElRef = useRef<HTMLDivElement | null>(null);
  const tabStripRef = useRef<HTMLDivElement | null>(null);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;

  const [isResizingSplit, setIsResizingSplit] = useState(false);

  const tabIds = useMemo(() => tabs.map((tab) => tab.id), [tabs]);
  const tabSortable = useInlineSortable({
    axis: "x",
    itemIds: tabIds,
    scrollViewportRef: tabStripRef,
    ignoreSelector: "button",
    onCommit: (fromIndex, toIndex) => {
      const currentTabs = tabsRef.current;
      const src = currentTabs[fromIndex];
      const dst = currentTabs[toIndex];
      if (src && dst) {
        orbitActions.reorderSidePanelTabs(
          src.id,
          dst.id,
          toIndex > fromIndex ? "after" : "before",
        );
      }
    },
  });

  const sideQuestionTab = tabs.find((t) => t.kind === "side_question") ?? null;
  const sideQuestionTabScope = sideQuestionTab ? sidePanelTabScopeKey(sideQuestionTab) : null;
  const terminalTab = tabs.find((t) => t.kind === "terminal") ?? null;
  const terminalHostId = terminalTab?.hostId ?? selectedHostId;
  const artifactsTab = tabs.find((t) => t.kind === "artifacts") ?? null;
  const artifactsHostId = artifactsTab?.hostId ?? selectedHostId;
  const artifactsAgentId = artifactsTab?.agentId ?? null;
  const artifactsWorkspaceId = artifactsTab?.workspaceId ?? "";

  const hasStreamingSideQuestion = useOrbitSelector((s) => {
    if (!sideQuestionTabScope) return false;
    const list = s.sideQuestionsByScope[sideQuestionTabScope];
    return Boolean(list?.some((e) => e.status === "streaming"));
  });
  const activeTerminalCount = useOrbitSelector(
    (s) => s.terminalTabs.filter((t) => t.hostId === terminalHostId).length,
  );
  const totalTerminalCount = useOrbitSelector((s) => s.terminalTabs.length);
  const artifactCount = useOrbitSelector((s) => {
    const detail = s.hostDetails[artifactsHostId];
    const all = detail?.artifacts ?? [];
    if (all.length === 0) return 0;
    const boundAg = artifactsAgentId
      ? detail?.agents.find((a) => a.agent_id === artifactsAgentId)
      : null;
    const wsId = artifactsWorkspaceId || boundAg?.workspace_id || "";
    if (wsId) {
      const wsCount = all.filter((a) => a.workspace_id === wsId).length;
      if (wsCount > 0) return wsCount;
    }
    return all.length;
  });

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

  const hasTerminalTabInBar = terminalTab !== null;
  const hasAnyPtySession = totalTerminalCount > 0;
  const terminalSlot = terminalTab ? paneSlotForTab(terminalTab.id) : null;
  const isTerminalVisible = terminalSlot !== null;

  const hasReviewTab = tabs.some((t) => t.kind === "review");
  const reviewTab = tabs.find((t) => t.kind === "review") ?? null;
  const reviewSlot = reviewTab ? paneSlotForTab(reviewTab.id) : null;
  const isReviewVisible = reviewSlot !== null;

  const boardTabs = tabs.filter((t) => t.kind === "board");

  const splitRatioRef = useRef(splitRatio);
  splitRatioRef.current = splitRatio;

  const handlePanelDragStart = useCallback(
    () => Math.round(panelElRef.current?.getBoundingClientRect().width ?? 480),
    [],
  );

  const handlePanelDragMove = useCallback((delta: number, startW: number) => {
    orbitActions.setSidePanelWidth(startW - delta, false);
  }, []);

  const handlePanelDragCommit = useCallback((delta: number, startW: number) => {
    orbitActions.setSidePanelWidth(startW - delta, true);
  }, []);

  const handlePanelDragCancel = useCallback((startW: number) => {
    orbitActions.setSidePanelWidth(startW, false);
  }, []);

  const handlePanelKeyboardStep = useCallback((mult: number) => {
    const cur = Math.round(panelElRef.current?.getBoundingClientRect().width ?? 480);
    orbitActions.setSidePanelWidth(cur - mult * 16, true);
  }, []);

  const handleEqualizeSidePanel = useCallback(() => {
    const leftEl = sidebarOpen ? document.querySelector<HTMLElement>(".orbit-panel-left") : null;
    const activeLeft = leftEl ? Math.round(leftEl.getBoundingClientRect().width) : 0;
    orbitActions.equalizeSidePanel(window.innerWidth, activeLeft);
  }, [sidebarOpen]);

  const handleSplitDragStart = useCallback(() => {
    const rect = bodyElRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0) return null;
    return { width: rect.width, startRatio: splitRatioRef.current };
  }, []);

  const handleSplitDragMove = useCallback(
    (delta: number, snap: { width: number; startRatio: number }) => {
      orbitActions.setSidePanelSplitRatio(snap.startRatio + delta / snap.width);
    },
    [],
  );

  const handleSplitDragCancel = useCallback((snap: { width: number; startRatio: number }) => {
    orbitActions.setSidePanelSplitRatio(snap.startRatio);
  }, []);

  const handleSplitReset = useCallback(() => {
    orbitActions.setSidePanelSplitRatio(0.5);
  }, []);

  const handleSplitKeyboardStep = useCallback((mult: number) => {
    orbitActions.setSidePanelSplitRatio(splitRatioRef.current + mult * 0.04);
  }, []);

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
        <ResizeSash
          orientation="vertical"
          variant="edge-start"
          ariaLabel={t("sidePanel.resizeAria")}
          title={t("sidePanel.resizeHint")}
          onDragStart={handlePanelDragStart}
          onDragMove={handlePanelDragMove}
          onDragCommit={handlePanelDragCommit}
          onDragCancel={handlePanelDragCancel}
          onDraggingChange={setIsResizingRight}
          onReset={handleEqualizeSidePanel}
          onKeyboardStep={handlePanelKeyboardStep}
        />
      )}

      <DragShield active={tabSortable.isDragging} cursor="grabbing" />

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {shouldRenderBody && (
          <>
            {/* Flush 40px Two-State Side Panel Header Bar (No-Tab vs With-Tab) */}
            <div className="@container border-border-subtle flex h-(--orbit-titlebar-h) min-w-0 shrink-0 items-center gap-1 border-b pr-[44px] pl-2">
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
                    {tabs.map((tab, idx) => {
                      const isPrimary = tab.id === activeTab?.id;
                      const isSecondary = tab.id === secondaryTab?.id;
                      const isActive = isPrimary || isSecondary;
                      const showPulseDot =
                        (tab.kind === "side_question" && hasStreamingSideQuestion) ||
                        (tab.kind === "terminal" && activeTerminalCount > 0 && !isActive);

                      const displayTitle = resolveSidePanelTabTitle(tab, t);
                      const isDragged = tabSortable.sortState?.fromIndex === idx;
                      const isSettling = isDragged && tabSortable.sortState?.phase === "settling";

                      return (
                        <div
                          key={tab.id}
                          ref={(el) => tabSortable.registerItem(tab.id, el)}
                          role="tab"
                          aria-selected={isPrimary}
                          title={tab.filePath || tab.url || displayTitle}
                          onPointerDown={
                            tabs.length > 1
                              ? (e) => tabSortable.handlePointerDown(e, idx)
                              : undefined
                          }
                          onDragStart={(e) => e.preventDefault()}
                          onClick={(e) => {
                            if (tabSortable.shouldSuppressClick()) return;
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
                          style={tabSortable.getNeighborStyle(idx)}
                          className={cn(
                            "group/stab relative inline-flex h-[28px] max-w-[116px] shrink-0 touch-none cursor-pointer items-center gap-1.5 rounded-[7px] border px-2 text-[12px] [-webkit-user-drag:none] @[380px]:max-w-[144px] @[480px]:max-w-[168px]",
                            isDragged
                              ? cn(
                                  "drag-lifted-surface border-transparent text-ink-primary z-10",
                                  (isPrimary || isSecondary) && "font-medium",
                                  !isSettling && "cursor-grabbing",
                                )
                              : cn(
                                  tabSortable.isActive
                                    ? "transition-transform duration-150 ease-[var(--ease-out-expo)] motion-reduce:transition-none"
                                    : "transition-colors",
                                  isPrimary
                                    ? "border-transparent bg-black/[0.055] text-foreground font-medium dark:bg-white/[0.08]"
                                    : isSecondary
                                      ? "border-border-default bg-surface-recessed text-ink-primary font-medium"
                                      : "text-muted-foreground border-transparent hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.05]",
                                ),
                          )}
                        >
                          <SidePanelTabIcon tab={tab} />
                          <span className="min-w-0 truncate">{displayTitle}</span>
                          {tab.kind === "file" && tab.filePath && (
                            <FileTabDirtyIndicator
                              hostId={tab.hostId ?? ""}
                              cwd={tab.cwd ?? "~"}
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
                              tabSortable.isActive && "pointer-events-none",
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
                  <PanelErrorBoundary
                    panelName="Terminal"
                    resetKey={`${scopeResetPrefix}:${terminalHostId}`}
                  >
                    <TerminalTab
                      tab={terminalTab ?? undefined}
                      selectedHostId={terminalHostId}
                      visible={isTerminalVisible}
                    />
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
                      key={`${scopeResetPrefix}:${browserTab.id}`}
                      style={styleForSlot(slot)}
                      className={cn(
                        "min-h-0 min-w-0 flex-col overflow-hidden",
                        browserVisible ? "flex flex-1" : "hidden",
                      )}
                    >
                      <PanelErrorBoundary
                        panelName="Browser"
                        resetKey={`${scopeResetPrefix}:${browserTab.url ?? ""}`}
                      >
                        <BrowserTab tab={browserTab} visible={browserVisible} />
                      </PanelErrorBoundary>
                    </div>
                  );
                })}

              {/* Persistent Review Pool */}
              {hasReviewTab && reviewTab && (
                <div
                  style={styleForSlot(reviewSlot)}
                  className={cn(
                    "min-h-0 min-w-0 flex-col overflow-hidden",
                    isReviewVisible ? "flex flex-1" : "hidden",
                  )}
                >
                  <PanelErrorBoundary
                    panelName="Review"
                    resetKey={`${scopeResetPrefix}:${reviewTab.hostId ?? selectedHostId}:${reviewTab.cwd ?? ""}`}
                  >
                    <ReviewTab
                      key={`${scopeResetPrefix}:${reviewTab.hostId ?? selectedHostId}:${reviewTab.cwd ?? ""}`}
                      tab={reviewTab}
                      selectedHostId={reviewTab.hostId ?? selectedHostId}
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
                    <PanelErrorBoundary
                      panelName="Workspace Notes"
                      resetKey={`${scopeResetPrefix}:${boardTab.id}`}
                    >
                      <WorkspaceBoardTab tab={boardTab} visible={isBoardVisible} />
                    </PanelErrorBoundary>
                  </div>
                );
              })}

              {/* Resizable Dual-Split Center Divider (order: 2 between Primary=1 and Secondary=3) */}
              {isSplitActive && (
                <ResizeSash
                  orientation="vertical"
                  variant="split"
                  style={{ order: 2 }}
                  ariaLabel={t("sidePanel.resizeSplitAria")}
                  title={t("sidePanel.resizeSplitHint")}
                  onDragStart={handleSplitDragStart}
                  onDragMove={handleSplitDragMove}
                  onDragCancel={handleSplitDragCancel}
                  onDraggingChange={setIsResizingSplit}
                  onReset={handleSplitReset}
                  onKeyboardStep={handleSplitKeyboardStep}
                />
              )}

              {/* Transient Primary Tab Body */}
              {activeTab &&
                renderTransientTabBody(
                  activeTab,
                  scopeResetPrefix,
                  primaryPaneStyle,
                  transientClass,
                )}

              {/* Transient Secondary Tab Body (when dual split active) */}
              {secondaryTab &&
                renderTransientTabBody(
                  secondaryTab,
                  scopeResetPrefix,
                  secondaryPaneStyle,
                  transientClass,
                )}

              {!activeTab && (
                <PanelErrorBoundary
                  panelName="Side Panel"
                  resetKey={`${scopeResetPrefix}:${selectedHostId}`}
                >
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
