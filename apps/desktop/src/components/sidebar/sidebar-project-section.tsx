import { memo, useCallback, useEffect, useRef, useState } from "react";
import {
  AlarmClock,
  AlertTriangle,
  ChevronRight,
  Folder,
  FolderOpen,
  Layers,
  MoreHorizontal,
  Plus,
  RefreshCw,
} from "lucide-react";
import {
  type AgentCardView,
  type HostCardView,
  type SidebarConversationView,
  type SidebarProjectGroupView,
  type WorkspaceOrganizeEvent,
  isAgentForegroundRunning,
  isAgentTimerWaiting,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { useImeCompositionGuard } from "@/lib/ime";
import { useI18n } from "@/lib/i18n";
import { SidebarConversationRow } from "@/components/sidebar/sidebar-conversation-row";
import {
  MAX_VISIBLE_CONVERSATIONS_PER_PROJECT,
  type SidebarProjectRenderModel,
  type SidebarWorkspaceRenderModel,
  isSidebarConversationUnread,
} from "@/components/sidebar/sidebar-model";
import type {
  ActiveProjectMenuState,
  ActiveWorkspaceMenuState,
  MenuAnchorBox,
} from "@/components/sidebar/sidebar-menus";
import type {
  ThreadDragSource,
  ThreadDropIndicator,
} from "@/components/sidebar/use-sidebar-drag";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SimpleTooltip } from "@/components/ui/overlays";
import { HostStatusIndicator } from "@/components/ui/status-indicator";
import { orbitActions, threadScopeKey } from "@/state/orbit-store";
import type { WorkspacePanelMode } from "@/state/store-types";
import { cn } from "@/lib/utils";

export interface InlineCreateGroupTarget {
  projectId: string;
  hostId: string;
  projectRoot: string;
  moveAgentId?: string | null;
}

interface SidebarWorkspaceGroupProps {
  ws: SidebarWorkspaceRenderModel;
  proj: SidebarProjectGroupView;
  host: HostCardView;
  workspacePanel: WorkspacePanelMode;
  isDraftNewSession: boolean;
  selectedHostId: string | null;
  selectedAgentId: string | null;
  threadCustomTitles: Record<string, string>;
  titleIndicesByThreadKey: Map<string, number[]>;
  runningParentThreadKeys: ReadonlySet<string>;
  renamingThreadKey: string | null;
  renamingWorkspaceId: string | null;
  onFinishRenameWorkspace: () => void;
  openConvMenuThreadKey: string | null;
  wsMenuOpen: boolean;
  dragOverDropId: string | null;
  draggingThread: ThreadDragSource | null;
  threadDropIndicator: ThreadDropIndicator | null;
  onOpenConvMenu: (
    threadKey: string,
    host: HostCardView,
    agent: AgentCardView,
    isPinned: boolean,
    isUnread: boolean,
    anchor: MenuAnchorBox,
  ) => void;
  onOpenWsMenu: (state: ActiveWorkspaceMenuState) => void;
  onStartRenameThread: (threadKey: string) => void;
  onFinishRenameThread: () => void;
  onRegisterNavRow: (navId: string, el: HTMLElement | null) => void;
  onRegisterThreadRow: (
    refKey: string,
    entry: {
      hostId: string;
      projectId: string;
      agentId: string;
      scopeKey: string;
      workspaceId: string | null;
    } | null,
    el: HTMLDivElement | null,
  ) => void;
  onRegisterWorkspaceHeader: (
    workspaceId: string,
    meta: { hostId: string; projectId: string },
    el: HTMLDivElement | null,
  ) => void;
  onStartThreadDrag: (e: React.PointerEvent<HTMLDivElement>, source: ThreadDragSource) => void;
  shouldSuppressThreadClick: () => boolean;
  keyboardFocusedNavId: string | null;
}

const SidebarWorkspaceGroup = memo(function SidebarWorkspaceGroup({
  ws,
  proj,
  host,
  workspacePanel,
  isDraftNewSession,
  selectedHostId,
  selectedAgentId,
  threadCustomTitles,
  titleIndicesByThreadKey,
  runningParentThreadKeys,
  renamingThreadKey,
  renamingWorkspaceId,
  onFinishRenameWorkspace,
  openConvMenuThreadKey,
  wsMenuOpen,
  dragOverDropId,
  draggingThread,
  threadDropIndicator,
  onOpenConvMenu,
  onOpenWsMenu,
  onStartRenameThread,
  onFinishRenameThread,
  onRegisterNavRow,
  onRegisterThreadRow,
  onRegisterWorkspaceHeader,
  onStartThreadDrag,
  shouldSuppressThreadClick,
  keyboardFocusedNavId,
}: SidebarWorkspaceGroupProps) {
  const { t } = useI18n();
  const ime = useImeCompositionGuard();

  const isRenamingThisWs = renamingWorkspaceId === ws.workspace_id;
  const [renameDraft, setRenameDraft] = useState(ws.title);
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const renameCancelledRef = useRef(false);
  const renameCommittedRef = useRef(false);

  useEffect(() => {
    if (!isRenamingThisWs) return;
    renameCancelledRef.current = false;
    renameCommittedRef.current = false;
    setRenameDraft(ws.title);
    const raf = window.requestAnimationFrame(() => {
      renameInputRef.current?.focus();
      renameInputRef.current?.select();
    });
    return () => window.cancelAnimationFrame(raf);
  }, [isRenamingThisWs, ws.title]);

  const commitWsRename = () => {
    if (renameCommittedRef.current) return;
    renameCommittedRef.current = true;
    if (renameCancelledRef.current) {
      renameCancelledRef.current = false;
      onFinishRenameWorkspace();
      return;
    }
    const clean = renameDraft.trim();
    onFinishRenameWorkspace();
    if (clean && clean !== ws.title) {
      void orbitActions.renameWorkspace(proj.host_id, ws.workspace_id, clean);
    }
  };

  const wsCollapsed = ws.wsCollapsed;
  const wsHasRunning = ws.visiblePeers.some(
    (c) =>
      isAgentForegroundRunning(c.agent) ||
      Boolean(c.agent.has_running_subagents) ||
      runningParentThreadKeys.has(threadScopeKey(proj.host_id, c.agent.agent_id)),
  );
  const wsHasUnread = ws.visiblePeers.some((c) =>
    isSidebarConversationUnread(c, {
      workspacePanel,
      isDraftNewSession,
      selectedHostId,
      selectedAgentId,
    }),
  );
  const wsHasTimer = ws.visiblePeers.some((c) => isAgentTimerWaiting(c.agent));
  const isDropHover = dragOverDropId === `ws:${ws.workspace_id}`;
  const setWorkspaceHeaderRef = useCallback(
    (el: HTMLDivElement | null) => {
      onRegisterWorkspaceHeader(ws.workspace_id, { hostId: proj.host_id, projectId: proj.id }, el);
    },
    [onRegisterWorkspaceHeader, ws.workspace_id, proj.host_id, proj.id],
  );

  return (
    <div
      className={cn(
        "space-y-0.5 rounded-[8px] transition-colors",
        isDropHover && "bg-accent/55 ring-border-strong ring-1",
      )}
    >
      <div
        ref={setWorkspaceHeaderRef}
        onContextMenu={(e) => {
          if (isRenamingThisWs) return;
          e.preventDefault();
          onOpenWsMenu({
            workspace: ws,
            project: proj,
            host,
            anchor: {
              left: e.clientX,
              top: e.clientY,
              width: 1,
              height: 1,
            },
          });
        }}
        className="group/ws hover:bg-accent/55 flex h-[28px] items-center justify-between gap-1 rounded-[8px] pr-2 pl-[20px] transition-colors"
      >
        {isRenamingThisWs ? (
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <Layers className="text-ink-muted size-[13.5px] shrink-0 stroke-[1.7]" />
            <input
              ref={renameInputRef}
              value={renameDraft}
              onChange={(e) => setRenameDraft(e.target.value)}
              onBlur={commitWsRename}
              {...ime.compositionProps}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (ime.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter") {
                  e.preventDefault();
                  commitWsRename();
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  renameCancelledRef.current = true;
                  onFinishRenameWorkspace();
                }
              }}
              className="bg-surface-base text-ink-primary ring-border-strong h-[22px] min-w-0 flex-1 rounded-[5px] px-1.5 text-[12.5px] ring-1 outline-none"
            />
          </div>
        ) : (
          <button
            type="button"
            title={ws.mission_goal ? `${ws.title}\n${ws.mission_goal}` : ws.title}
            onClick={() => orbitActions.toggleWorkspaceCollapsed(ws.workspace_id)}
            className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
          >
            <span className="text-ink-muted relative inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
              <Layers className="size-[13.5px] stroke-[1.7] transition-opacity duration-150 group-hover/ws:opacity-0" />
              <ChevronRight
                className={cn(
                  "absolute size-[13px] opacity-0 transition-[rotate,opacity] duration-150 ease-[var(--ease-panel)] group-hover/ws:opacity-90",
                  !wsCollapsed && "rotate-90",
                )}
              />
            </span>
            <span className="text-ink-secondary group-hover/ws:text-ink-primary truncate text-[12.5px] font-medium transition-colors">
              {ws.title}
            </span>
            {ws.visiblePeers.length > 0 && (
              <span
                className={cn(
                  "text-ink-faint shrink-0 text-[11px] tabular-nums transition-opacity duration-150 ease-[var(--ease-out-quart)]",
                  wsMenuOpen
                    ? "opacity-0"
                    : "opacity-100 group-hover/ws:opacity-0 group-focus-within/ws:opacity-0 [@media(hover:none)]:opacity-0",
                )}
              >
                {ws.visiblePeers.length}
              </span>
            )}
            {ws.has_conflict && (
              <AlertTriangle className="text-signal-warning size-[11.5px] shrink-0" />
            )}
            {!ws.has_conflict && wsHasRunning && (
              <OrbitSpinner size={11} className="text-ink-secondary shrink-0" />
            )}
            {!ws.has_conflict && !wsHasRunning && wsCollapsed && wsHasUnread && (
              <span className="bg-signal-info size-[5.5px] shrink-0 rounded-full" />
            )}
            {!ws.has_conflict && !wsHasRunning && wsCollapsed && !wsHasUnread && wsHasTimer && (
              <AlarmClock
                aria-label={t("sidebar.timerArmed")}
                className="text-ink-faint size-[11px] shrink-0 stroke-[1.7]"
              />
            )}
          </button>
        )}

        {!isRenamingThisWs && (
          <div
            className={cn(
              "flex shrink-0 items-center gap-0.5 transition-opacity duration-150 ease-[var(--ease-out-quart)]",
              wsMenuOpen
                ? "pointer-events-auto opacity-100"
                : "pointer-events-none opacity-0 group-hover/ws:pointer-events-auto group-hover/ws:opacity-100 group-focus-within/ws:pointer-events-auto group-focus-within/ws:opacity-100 [@media(hover:none)]:pointer-events-auto [@media(hover:none)]:opacity-100",
            )}
          >
            <IconButton
              size="xs"
              aria-label={t("sidebar.groupOptions", { group: ws.title })}
              onClick={(e) => {
                e.stopPropagation();
                const r = e.currentTarget.getBoundingClientRect();
                onOpenWsMenu({
                  workspace: ws,
                  project: proj,
                  host,
                  anchor: {
                    left: r.left,
                    top: r.top,
                    width: r.width,
                    height: r.height,
                  },
                });
              }}
            >
              <MoreHorizontal className="size-[13px]" />
            </IconButton>
            <IconButton
              size="xs"
              tooltip={t("sidebar.newConversationInTarget", { name: ws.title })}
              onClick={(e) => {
                e.stopPropagation();
                orbitActions.startNewThreadDraft(
                  proj.host_id,
                  ws.cwd || proj.primary_cwd,
                  undefined,
                  undefined,
                  ws.workspace_id,
                );
              }}
            >
              <Plus className="size-[13.5px] stroke-[1.85]" />
            </IconButton>
          </div>
        )}
      </div>

      {(ws.visiblePeers.length > 0 || !ws.hasActivePeersInGroup) && (
        <div
          inert={wsCollapsed}
          data-open={!wsCollapsed ? "true" : "false"}
          className="orbit-collapse-grid"
        >
          <div className="orbit-collapse-inner">
            <div className="border-border-subtle ml-[27px] space-y-0.5 border-l pl-2">
              {ws.visiblePeers.length === 0 ? (
                <div className="text-ink-faint px-2 py-1 text-[11.5px]">{t("sidebar.emptyGroup")}</div>
              ) : (
                ws.visiblePeers.map((conv) => {
                  const ag = conv.agent;
                  const tKey = threadScopeKey(proj.host_id, ag.agent_id);
                  const navId = `ws:${ws.workspace_id}:${proj.host_id}:${ag.agent_id}`;
                  const selected =
                    workspacePanel === "conversation" &&
                    !isDraftNewSession &&
                    selectedHostId === proj.host_id &&
                    selectedAgentId === ag.agent_id;
                  const unread = isSidebarConversationUnread(conv, {
                    workspacePanel,
                    isDraftNewSession,
                    selectedHostId,
                    selectedAgentId,
                  });
                  const effectiveTitle = threadCustomTitles[tKey] || ag.title;

                  return (
                    <SidebarConversationRow
                      key={ag.agent_id}
                      threadKey={tKey}
                      navId={navId}
                      host={host}
                      agent={ag}
                      projectName={proj.name}
                      effectiveTitle={effectiveTitle}
                      isWorktree={conv.is_worktree}
                      isPinned={false}
                      isUnread={unread}
                      hasRunningSubagents={
                        Boolean(ag.has_running_subagents) || runningParentThreadKeys.has(tKey)
                      }
                      selected={selected}
                      keyboardFocused={keyboardFocusedNavId === navId}
                      highlightIndices={titleIndicesByThreadKey.get(tKey)}
                      isChild={false}
                      inGroupRail
                      peerHandle={ag.handle}
                      menuOpen={openConvMenuThreadKey === tKey}
                      isRenaming={renamingThreadKey === tKey}
                      dragProjectId={proj.id}
                      dragScopeKey={ws.threadScopeKey}
                      dragWorkspaceId={ws.workspace_id}
                      isDragging={
                        draggingThread?.hostId === proj.host_id &&
                        draggingThread?.agentId === ag.agent_id
                      }
                      dropIndicator={
                        threadDropIndicator?.scopeKey === ws.threadScopeKey &&
                        threadDropIndicator.targetAgentId === ag.agent_id
                          ? threadDropIndicator.position
                          : null
                      }
                      onRegisterNavRow={onRegisterNavRow}
                      onRegisterThreadRow={onRegisterThreadRow}
                      onStartThreadDrag={onStartThreadDrag}
                      shouldSuppressClick={shouldSuppressThreadClick}
                      onOpenMenu={onOpenConvMenu}
                      onStartRename={onStartRenameThread}
                      onFinishRename={onFinishRenameThread}
                    />
                  );
                })
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
});

export interface SidebarProjectSectionProps {
  model: SidebarProjectRenderModel;
  totalProjectCount: number;
  singleHostMode: boolean;
  hasFilterActive: boolean;
  isFilteringThreads: boolean;
  organizeEvent: WorkspaceOrganizeEvent | null;
  workspacePanel: WorkspacePanelMode;
  isDraftNewSession: boolean;
  selectedHostId: string | null;
  selectedAgentId: string | null;
  threadCustomTitles: Record<string, string>;
  runningParentThreadKeys: ReadonlySet<string>;
  expandedMore: boolean;
  onSetExpandedMore: (projectId: string, expanded: boolean) => void;
  renamingThreadKey: string | null;
  isRenamingProject: boolean;
  onFinishRenameProject: () => void;
  renamingWorkspaceId: string | null;
  onFinishRenameWorkspace: () => void;
  creatingGroupTarget: InlineCreateGroupTarget | null;
  onFinishCreateGroup: () => void;
  openConvMenuThreadKey: string | null;
  projMenuOpen: boolean;
  openWsMenuId: string | null;
  onOpenConvMenu: (
    threadKey: string,
    host: HostCardView,
    agent: AgentCardView,
    isPinned: boolean,
    isUnread: boolean,
    anchor: MenuAnchorBox,
  ) => void;
  onOpenProjMenu: (state: ActiveProjectMenuState) => void;
  onOpenWsMenu: (state: ActiveWorkspaceMenuState) => void;
  onStartRenameThread: (threadKey: string) => void;
  onFinishRenameThread: () => void;
  isDraggingProject: boolean;
  projectDropPosition: "before" | "after" | null;
  draggingThread: ThreadDragSource | null;
  threadDropIndicator: ThreadDropIndicator | null;
  dragOverDropId: string | null;
  onRegisterProjectRow: (projectId: string, el: HTMLDivElement | null) => void;
  onRegisterNavRow: (navId: string, el: HTMLElement | null) => void;
  onRegisterThreadRow: (
    refKey: string,
    entry: {
      hostId: string;
      projectId: string;
      agentId: string;
      scopeKey: string;
      workspaceId: string | null;
    } | null,
    el: HTMLDivElement | null,
  ) => void;
  onRegisterWorkspaceHeader: (
    workspaceId: string,
    meta: { hostId: string; projectId: string },
    el: HTMLDivElement | null,
  ) => void;
  onRegisterInboxArea: (
    projectId: string,
    meta: { hostId: string; scopeKey: string },
    el: HTMLDivElement | null,
  ) => void;
  onStartProjectDrag: (e: React.PointerEvent<HTMLDivElement>, projectId: string) => void;
  onStartThreadDrag: (e: React.PointerEvent<HTMLDivElement>, source: ThreadDragSource) => void;
  shouldSuppressProjectClick: () => boolean;
  shouldSuppressThreadClick: () => boolean;
  keyboardFocusedNavId: string | null;
}

export const SidebarProjectSection = memo(function SidebarProjectSection({
  model,
  totalProjectCount,
  singleHostMode,
  hasFilterActive,
  isFilteringThreads,
  organizeEvent,
  workspacePanel,
  isDraftNewSession,
  selectedHostId,
  selectedAgentId,
  threadCustomTitles,
  runningParentThreadKeys,
  expandedMore,
  onSetExpandedMore,
  renamingThreadKey,
  isRenamingProject,
  onFinishRenameProject,
  renamingWorkspaceId,
  onFinishRenameWorkspace,
  creatingGroupTarget,
  onFinishCreateGroup,
  openConvMenuThreadKey,
  projMenuOpen,
  openWsMenuId,
  onOpenConvMenu,
  onOpenProjMenu,
  onOpenWsMenu,
  onStartRenameThread,
  onFinishRenameThread,
  isDraggingProject,
  projectDropPosition,
  draggingThread,
  threadDropIndicator,
  dragOverDropId,
  onRegisterProjectRow,
  onRegisterNavRow,
  onRegisterThreadRow,
  onRegisterWorkspaceHeader,
  onRegisterInboxArea,
  onStartProjectDrag,
  onStartThreadDrag,
  shouldSuppressProjectClick,
  shouldSuppressThreadClick,
  keyboardFocusedNavId,
}: SidebarProjectSectionProps) {
  const {
    project: proj,
    host,
    visibleConversations,
    collapsed,
    projThreadScopeKey,
    curatedWorkspaces,
    slicedUnclassified,
    selectedUnclassifiedIdx,
    isExpandedAll,
    totalUnclassified,
    hiddenCount,
    titleIndicesByThreadKey,
  } = model;

  const { t } = useI18n();
  const ime = useImeCompositionGuard();

  // Isolated inline project rename state (typing never re-renders sibling projects)
  const [projRenameDraft, setProjRenameDraft] = useState(proj.name);
  const projRenameInputRef = useRef<HTMLInputElement | null>(null);
  const projRenameCancelledRef = useRef(false);
  const projRenameCommittedRef = useRef(false);

  useEffect(() => {
    if (!isRenamingProject) return;
    projRenameCancelledRef.current = false;
    projRenameCommittedRef.current = false;
    setProjRenameDraft(proj.name);
    const raf = window.requestAnimationFrame(() => {
      projRenameInputRef.current?.focus();
      projRenameInputRef.current?.select();
    });
    return () => window.cancelAnimationFrame(raf);
  }, [isRenamingProject, proj.name]);

  const commitProjRename = () => {
    if (projRenameCommittedRef.current) return;
    projRenameCommittedRef.current = true;
    if (projRenameCancelledRef.current) {
      projRenameCancelledRef.current = false;
      onFinishRenameProject();
      return;
    }
    const clean = projRenameDraft.trim();
    onFinishRenameProject();
    if (clean && clean !== proj.name) {
      void orbitActions.renameProject(proj, clean);
    }
  };

  // Isolated inline group creation state (single unified submit path)
  const isCreatingGroupHere = creatingGroupTarget?.projectId === proj.id;
  const [newGroupDraft, setNewGroupDraft] = useState("");
  const newGroupInputRef = useRef<HTMLInputElement | null>(null);
  const newGroupCancelledRef = useRef(false);
  const newGroupSubmittingRef = useRef(false);

  useEffect(() => {
    if (!isCreatingGroupHere) return;
    newGroupCancelledRef.current = false;
    newGroupSubmittingRef.current = false;
    setNewGroupDraft("");
    const raf = window.requestAnimationFrame(() => {
      newGroupInputRef.current?.focus();
    });
    return () => window.cancelAnimationFrame(raf);
  }, [isCreatingGroupHere]);

  const commitNewGroup = () => {
    if (newGroupSubmittingRef.current) return;
    if (newGroupCancelledRef.current) {
      newGroupCancelledRef.current = false;
      setNewGroupDraft("");
      onFinishCreateGroup();
      return;
    }
    const clean = newGroupDraft.trim();
    const activeTarget = creatingGroupTarget;
    setNewGroupDraft("");
    onFinishCreateGroup();
    if (!clean || !activeTarget) return;
    newGroupSubmittingRef.current = true;
    const req = activeTarget.moveAgentId
      ? orbitActions.moveAgentToWorkspace(activeTarget.hostId, activeTarget.moveAgentId, null, {
          newWorkspaceTitle: clean,
        })
      : orbitActions.createMissionWorkspace({
          hostId: activeTarget.hostId,
          sourceCwd: activeTarget.projectRoot,
          title: clean,
        });
    void req.finally(() => {
      newGroupSubmittingRef.current = false;
    });
  };

  const isDraggingThis = isDraggingProject;
  const showDropBefore = projectDropPosition === "before";
  const showDropAfter = projectDropPosition === "after";

  const hasCollapsedApproval =
    collapsed && visibleConversations.some((c) => c.agent.is_awaiting_permission);
  const hasCollapsedRunning =
    collapsed &&
    !hasCollapsedApproval &&
    visibleConversations.some(
      (c) =>
        isAgentForegroundRunning(c.agent) ||
        Boolean(c.agent.has_running_subagents) ||
        runningParentThreadKeys.has(threadScopeKey(proj.host_id, c.agent.agent_id)),
    );
  const hasCollapsedUnread =
    collapsed &&
    !hasCollapsedApproval &&
    !hasCollapsedRunning &&
    visibleConversations.some((c) =>
      isSidebarConversationUnread(c, {
        workspacePanel,
        isDraftNewSession,
        selectedHostId,
        selectedAgentId,
      }),
    );
  const hasCollapsedTimer =
    collapsed &&
    !hasCollapsedApproval &&
    !hasCollapsedRunning &&
    !hasCollapsedUnread &&
    visibleConversations.some((c) => isAgentTimerWaiting(c.agent));

  const isProjectOrganizing = organizeEvent?.phase === "running";
  const showEmptyInboxDropZone =
    totalUnclassified === 0 &&
    Boolean(
      draggingThread &&
        draggingThread.hostId === proj.host_id &&
        draggingThread.projectId === proj.id &&
        draggingThread.workspaceId !== null,
    );
  const hasProjectBody =
    isCreatingGroupHere ||
    curatedWorkspaces.length > 0 ||
    slicedUnclassified.length > 0 ||
    showEmptyInboxDropZone;

  const showRemoteHostTag = !singleHostMode && !isLocalHostCard(host);
  const isHostDisconnected = !singleHostMode && host.connection_state === "disconnected";
  const projTooltip = [
    !singleHostMode ? `${host.display_name} · ${proj.primary_cwd}` : proj.primary_cwd,
    totalProjectCount > 1 ? t("sidebar.dragToReorder") : null,
  ]
    .filter(Boolean)
    .join(" · ");

  const renderUnclassifiedRow = (conv: SidebarConversationView) => {
    const ag = conv.agent;
    const tKey = threadScopeKey(proj.host_id, ag.agent_id);
    const navId = `proj:${proj.id}:${proj.host_id}:${ag.agent_id}`;
    const selected =
      workspacePanel === "conversation" &&
      !isDraftNewSession &&
      selectedHostId === proj.host_id &&
      selectedAgentId === ag.agent_id;
    const unread = isSidebarConversationUnread(conv, {
      workspacePanel,
      isDraftNewSession,
      selectedHostId,
      selectedAgentId,
    });
    const effectiveTitle = threadCustomTitles[tKey] || ag.title;

    return (
      <SidebarConversationRow
        key={ag.agent_id}
        threadKey={tKey}
        navId={navId}
        host={host}
        agent={ag}
        projectName={proj.name}
        effectiveTitle={effectiveTitle}
        isWorktree={conv.is_worktree}
        isPinned={false}
        isUnread={unread}
        hasRunningSubagents={
          Boolean(ag.has_running_subagents) || runningParentThreadKeys.has(tKey)
        }
        selected={selected}
        keyboardFocused={keyboardFocusedNavId === navId}
        highlightIndices={titleIndicesByThreadKey.get(tKey)}
        isChild={Boolean(ag.parent_agent_id)}
        menuOpen={openConvMenuThreadKey === tKey}
        isRenaming={renamingThreadKey === tKey}
        dragProjectId={proj.id}
        dragScopeKey={projThreadScopeKey}
        dragWorkspaceId={null}
        isDragging={
          draggingThread?.hostId === proj.host_id && draggingThread?.agentId === ag.agent_id
        }
        dropIndicator={
          threadDropIndicator?.scopeKey === projThreadScopeKey &&
          threadDropIndicator.targetAgentId === ag.agent_id
            ? threadDropIndicator.position
            : null
        }
        onRegisterNavRow={onRegisterNavRow}
        onRegisterThreadRow={onRegisterThreadRow}
        onStartThreadDrag={onStartThreadDrag}
        shouldSuppressClick={shouldSuppressThreadClick}
        onOpenMenu={onOpenConvMenu}
        onStartRename={onStartRenameThread}
        onFinishRename={onFinishRenameThread}
      />
    );
  };

  const setProjectRowRef = useCallback(
    (el: HTMLDivElement | null) => {
      onRegisterProjectRow(proj.id, el);
    },
    [onRegisterProjectRow, proj.id],
  );

  const setInboxAreaRef = useCallback(
    (el: HTMLDivElement | null) => {
      onRegisterInboxArea(proj.id, { hostId: proj.host_id, scopeKey: projThreadScopeKey }, el);
    },
    [onRegisterInboxArea, proj.id, proj.host_id, projThreadScopeKey],
  );

  return (
    <div
      ref={setProjectRowRef}
      className={cn(
        "relative space-y-0.5 transition-opacity duration-150",
        isDraggingThis && "opacity-45",
      )}
    >
      {showDropBefore && (
        <div className="bg-signal-info pointer-events-none absolute -top-1 right-1 left-1 z-20 h-[2px] animate-[orbit-stage-in_110ms_var(--ease-out-quart)] rounded-full" />
      )}

      {/* Project Header Row (`30px`, pointer-draggable to reorder) */}
      <div
        onPointerDown={isRenamingProject ? undefined : (e) => onStartProjectDrag(e, proj.id)}
        onContextMenu={(e) => {
          if (isRenamingProject) return;
          e.preventDefault();
          onOpenProjMenu({
            project: proj,
            host,
            anchor: {
              left: e.clientX,
              top: e.clientY,
              width: 1,
              height: 1,
            },
          });
        }}
        className="group/proj text-ink-primary hover:bg-accent/55 flex h-[30px] items-center justify-between gap-1.5 rounded-[8px] px-2 transition-colors"
      >
        {isRenamingProject ? (
          <div className="flex min-w-0 flex-1 items-center gap-2.5">
            <span className="text-ink-muted inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
              {collapsed ? (
                <Folder className="size-[15px] stroke-[1.75]" />
              ) : (
                <FolderOpen className="size-[15px] stroke-[1.75]" />
              )}
            </span>
            <input
              ref={projRenameInputRef}
              data-proj-action="true"
              value={projRenameDraft}
              onChange={(e) => setProjRenameDraft(e.target.value)}
              onBlur={commitProjRename}
              {...ime.compositionProps}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (ime.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter") {
                  e.preventDefault();
                  commitProjRename();
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  projRenameCancelledRef.current = true;
                  onFinishRenameProject();
                }
              }}
              className="bg-surface-base text-ink-primary ring-border-strong -mx-1 h-[22px] min-w-0 flex-1 rounded-[5px] px-1.5 text-[13px] font-medium ring-1 outline-none"
            />
          </div>
        ) : (
          <button
            type="button"
            title={projTooltip}
            onClick={() => {
              if (shouldSuppressProjectClick()) return;
              orbitActions.toggleProjectCollapsed(proj.id);
            }}
            className="flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 text-left"
          >
            <span className="text-ink-muted relative inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
              {collapsed ? (
                <Folder className="size-[15px] stroke-[1.75] transition-opacity duration-150 group-hover/proj:opacity-0" />
              ) : (
                <FolderOpen className="size-[15px] stroke-[1.75] transition-opacity duration-150 group-hover/proj:opacity-0" />
              )}
              <ChevronRight
                className={cn(
                  "absolute size-[13.5px] opacity-0 transition-[rotate,opacity] duration-150 ease-[var(--ease-panel)] group-hover/proj:opacity-90",
                  !collapsed && "rotate-90",
                )}
              />
            </span>

            <span className="truncate text-[13px] font-medium">{proj.name}</span>

            {showRemoteHostTag && (
              <span className="text-ink-faint ml-0.5 inline-flex shrink-0 items-center gap-1 text-[11px]">
                <HostStatusIndicator state={host.connection_state} />
                <span className="max-w-[72px] truncate">{host.display_name}</span>
              </span>
            )}
          </button>
        )}

        {/* Right Slot: Organizing stays hover-readable; resting status/count crossfades to `⋯` + `+` */}
        {!isRenamingProject && (
          <div className="flex shrink-0 items-center justify-end gap-1">
            {isProjectOrganizing && (
              <SimpleTooltip
                label={organizeEvent?.message || t("store.organizingUnfiledSessions")}
              >
                <span
                  data-proj-action="true"
                  className="text-ink-tertiary inline-flex h-[22px] items-center gap-1.5 pr-0.5 text-[11px] font-medium"
                >
                  <OrbitSpinner size={12} className="text-ink-secondary" />
                  <span className="agent-text-shimmer">{t("sidebar.organizing")}</span>
                </span>
              </SimpleTooltip>
            )}

            <div className="grid shrink-0 items-center justify-items-end">
              {!isProjectOrganizing &&
                (isHostDisconnected ||
                  hasCollapsedApproval ||
                  hasCollapsedRunning ||
                  hasCollapsedUnread ||
                  hasCollapsedTimer ||
                  (collapsed && visibleConversations.length > 0)) && (
                  <div
                    className={cn(
                      "col-start-1 row-start-1 flex items-center transition-opacity duration-150 ease-[var(--ease-out-quart)]",
                      projMenuOpen
                        ? "pointer-events-none opacity-0"
                        : "opacity-100 group-hover/proj:pointer-events-none group-hover/proj:opacity-0 group-focus-within/proj:pointer-events-none group-focus-within/proj:opacity-0 [@media(hover:none)]:pointer-events-none [@media(hover:none)]:opacity-0",
                    )}
                  >
                    {isHostDisconnected ? (
                      <IconButton
                        size="xs"
                        data-proj-action="true"
                        tooltip={t("sidebar.offlineReconnect", { host: host.display_name })}
                        onClick={() => {
                          void orbitIpc.connectHost(host.host_id);
                          void orbitIpc.refreshHost(host.host_id);
                        }}
                      >
                        <RefreshCw className="size-[12.5px]" />
                      </IconButton>
                    ) : hasCollapsedApproval ||
                      hasCollapsedRunning ||
                      hasCollapsedUnread ||
                      hasCollapsedTimer ? (
                      <span className="inline-flex h-[22px] w-[22px] items-center justify-center">
                        {hasCollapsedApproval ? (
                          <span className="bg-signal-warning size-[6px] rounded-full" />
                        ) : hasCollapsedRunning ? (
                          <OrbitSpinner size={13} className="text-ink-secondary" />
                        ) : hasCollapsedUnread ? (
                          <span className="bg-signal-info size-[6px] rounded-full" />
                        ) : (
                          <AlarmClock
                            aria-label={t("sidebar.timerArmed")}
                            className="text-ink-faint size-[11.5px] shrink-0 stroke-[1.7]"
                          />
                        )}
                      </span>
                    ) : (
                      <span className="text-ink-faint pr-1 text-[11px] tabular-nums">
                        {visibleConversations.length}
                      </span>
                    )}
                  </div>
                )}

              <div
                className={cn(
                  "col-start-1 row-start-1 flex items-center gap-0.5 transition-opacity duration-150 ease-[var(--ease-out-quart)]",
                  projMenuOpen
                    ? "pointer-events-auto opacity-100"
                    : "pointer-events-none opacity-0 group-hover/proj:pointer-events-auto group-hover/proj:opacity-100 group-focus-within/proj:pointer-events-auto group-focus-within/proj:opacity-100 [@media(hover:none)]:pointer-events-auto [@media(hover:none)]:opacity-100",
                )}
              >
                <IconButton
                  size="xs"
                  data-proj-action="true"
                  aria-label={t("sidebar.projectOptions", { project: proj.name })}
                  onClick={(e) => {
                    const r = e.currentTarget.getBoundingClientRect();
                    onOpenProjMenu({
                      project: proj,
                      host,
                      anchor: {
                        left: r.left,
                        top: r.top,
                        width: r.width,
                        height: r.height,
                      },
                    });
                  }}
                >
                  <MoreHorizontal className="size-[13.5px]" />
                </IconButton>

                <IconButton
                  size="xs"
                  data-proj-action="true"
                  tooltip={t("sidebar.newConversationInTarget", { name: proj.name })}
                  onClick={() => orbitActions.startNewThreadDraft(proj.host_id, proj.primary_cwd)}
                >
                  <Plus className="size-[14px] stroke-[1.85]" />
                </IconButton>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Collapsible Project Body (`inert` when collapsed so Tab focus never leaks inside) */}
      <div
        inert={collapsed}
        data-open={!collapsed ? "true" : "false"}
        className="orbit-collapse-grid"
      >
        <div className="orbit-collapse-inner">
          {!hasProjectBody ? (
            <div className="pt-0.5">
              {hasFilterActive ? (
                <div className="text-ink-faint flex h-[28px] items-center gap-2.5 px-2 text-[12px]">
                  <span className="inline-flex h-[16px] w-[16px] shrink-0" />
                  <span className="truncate">{t("sidebar.noActiveOrUnread")}</span>
                </div>
              ) : (
                <button
                  type="button"
                  onClick={() => orbitActions.startNewThreadDraft(proj.host_id, proj.primary_cwd)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent/55 flex h-[28px] w-full cursor-pointer items-center gap-2.5 rounded-[7px] px-2 text-left text-[12.5px] transition-colors"
                >
                  <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
                    <Plus className="size-[12.5px] stroke-[1.75] opacity-75" />
                  </span>
                  <span className="truncate">{t("sidebar.startConversation")}</span>
                </button>
              )}
            </div>
          ) : (
            <div className="space-y-0.5 py-0.5">
              {/* Inline Group Creation Row (zero-modal, Finder/VS Code style) */}
              {isCreatingGroupHere && (
                <div className="flex h-[28px] items-center gap-2 rounded-[8px] pr-2 pl-[20px]">
                  <Layers className="text-ink-muted size-[13.5px] shrink-0 stroke-[1.7]" />
                  <input
                    ref={newGroupInputRef}
                    value={newGroupDraft}
                    onChange={(e) => setNewGroupDraft(e.target.value)}
                    onBlur={commitNewGroup}
                    {...ime.compositionProps}
                    onKeyDown={(e) => {
                      e.stopPropagation();
                      if (ime.shouldIgnoreKeyDown(e)) return;
                      if (e.key === "Enter") {
                        e.preventDefault();
                        commitNewGroup();
                      } else if (e.key === "Escape") {
                        e.preventDefault();
                        newGroupCancelledRef.current = true;
                        setNewGroupDraft("");
                        onFinishCreateGroup();
                      }
                    }}
                    placeholder={t("sidebar.newGroupPlaceholder")}
                    className="bg-surface-base text-ink-primary placeholder:text-ink-faint ring-border-strong h-[22px] min-w-0 flex-1 rounded-[5px] px-1.5 text-[12.5px] ring-1 outline-none"
                  />
                </div>
              )}

              {/* 1. Optional Curated Groups */}
              {curatedWorkspaces.map((ws) => (
                <SidebarWorkspaceGroup
                  key={`ws-${ws.workspace_id}`}
                  ws={ws}
                  proj={proj}
                  host={host}
                  workspacePanel={workspacePanel}
                  isDraftNewSession={isDraftNewSession}
                  selectedHostId={selectedHostId}
                  selectedAgentId={selectedAgentId}
                  threadCustomTitles={threadCustomTitles}
                  titleIndicesByThreadKey={titleIndicesByThreadKey}
                  runningParentThreadKeys={runningParentThreadKeys}
                  renamingThreadKey={renamingThreadKey}
                  renamingWorkspaceId={renamingWorkspaceId}
                  onFinishRenameWorkspace={onFinishRenameWorkspace}
                  openConvMenuThreadKey={openConvMenuThreadKey}
                  wsMenuOpen={openWsMenuId === ws.workspace_id}
                  dragOverDropId={dragOverDropId}
                  draggingThread={draggingThread}
                  threadDropIndicator={threadDropIndicator}
                  onOpenConvMenu={onOpenConvMenu}
                  onOpenWsMenu={onOpenWsMenu}
                  onStartRenameThread={onStartRenameThread}
                  onFinishRenameThread={onFinishRenameThread}
                  onRegisterNavRow={onRegisterNavRow}
                  onRegisterThreadRow={onRegisterThreadRow}
                  onRegisterWorkspaceHeader={onRegisterWorkspaceHeader}
                  onStartThreadDrag={onStartThreadDrag}
                  shouldSuppressThreadClick={shouldSuppressThreadClick}
                  keyboardFocusedNavId={keyboardFocusedNavId}
                />
              ))}

              {/* 2. Flat Project Threads (or transient ungroup drop zone when dragging from a group) */}
              {(totalUnclassified > 0 || showEmptyInboxDropZone) && (
                <div
                  ref={setInboxAreaRef}
                  className={cn(
                    "space-y-0.5 rounded-[8px] transition-colors",
                    curatedWorkspaces.length > 0 && "pt-0.5",
                    dragOverDropId === `inbox:${proj.id}` &&
                      "bg-accent/55 ring-border-strong ring-1",
                  )}
                >
                  {showEmptyInboxDropZone ? (
                    <div className="border-border-default text-ink-muted flex h-[28px] items-center justify-center rounded-[7px] border border-dashed px-2 text-[11.5px]">
                      {t("sidebar.dropToUngroup")}
                    </div>
                  ) : (
                    <>
                      {slicedUnclassified.map(renderUnclassifiedRow)}

                      {hiddenCount > 0 && !isExpandedAll && (
                        <button
                          type="button"
                          onClick={() => onSetExpandedMore(proj.id, true)}
                          className="text-ink-muted hover:text-ink-primary hover:bg-accent/55 flex h-[26px] w-full cursor-pointer items-center gap-2.5 rounded-[7px] px-2 text-left text-[12px] transition-colors"
                        >
                          <span className="inline-flex h-[16px] w-[16px] shrink-0" />
                          <span>{t("common.showMore", { count: hiddenCount })}</span>
                        </button>
                      )}

                      {hiddenCount > 0 &&
                        expandedMore &&
                        !isFilteringThreads &&
                        selectedUnclassifiedIdx < MAX_VISIBLE_CONVERSATIONS_PER_PROJECT && (
                          <button
                            type="button"
                            onClick={() => onSetExpandedMore(proj.id, false)}
                            className="text-ink-muted hover:text-ink-primary hover:bg-accent/55 flex h-[26px] w-full cursor-pointer items-center gap-2.5 rounded-[7px] px-2 text-left text-[12px] transition-colors"
                          >
                            <span className="inline-flex h-[16px] w-[16px] shrink-0" />
                            <span>{t("common.showLess")}</span>
                          </button>
                        )}
                    </>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      </div>

      {showDropAfter && (
        <div className="bg-signal-info pointer-events-none absolute right-1 -bottom-1 left-1 z-20 h-[2px] animate-[orbit-stage-in_110ms_var(--ease-out-quart)] rounded-full" />
      )}
    </div>
  );
});
