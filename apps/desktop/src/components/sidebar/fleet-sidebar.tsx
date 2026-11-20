import { memo, useCallback, useMemo, useState } from "react";
import {
  Archive,
  Check,
  ChevronDown,
  ChevronRight,
  Clock,
  Folder,
  FolderOpen,
  ListFilter,
  Loader2,
  MoreHorizontal,
  Plus,
  RefreshCw,
  RotateCcw,
  Settings,
  Trash2,
} from "lucide-react";
import { modSymbol } from "@/lib/keybindings";
import {
  type AgentCardView,
  type HostCardView,
  type SidebarConversationView,
  type SidebarProjectGroupView,
  isMacDesktop,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  basenameOfProjectPath,
  matchesProjectDirectory,
  projectPathScopeKey,
} from "@/lib/projects";
import { SidebarConversationRow } from "@/components/sidebar/sidebar-conversation-row";
import {
  type ActiveConversationMenuState,
  type ActiveProjectMenuState,
  DeleteProjectDialog,
  type MenuAnchorBox,
  SharedConversationMenu,
  SharedProjectMenu,
} from "@/components/sidebar/sidebar-menus";
import {
  AddProjectButton,
  HostStateDot,
  useSidebarRelativeClock,
} from "@/components/sidebar/sidebar-primitives";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Popover,
  PopoverContent,
  PopoverTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  orbitActions,
  threadScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const MAX_VISIBLE_CONVERSATIONS_PER_PROJECT = 6;

export const FleetSidebar = memo(function FleetSidebar() {
  const sidebarOpen = useOrbitSelector((s) => s.sidebarOpen);
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedWorkspaceCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const collapsedProjectIds = useOrbitSelector((s) => s.collapsedProjectIds);
  const threadCustomTitles = useOrbitSelector((s) => s.threadCustomTitles);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const sidebarProjectSort = useOrbitSelector((s) => s.sidebarProjectSort);
  const sidebarThreadFilter = useOrbitSelector((s) => s.sidebarThreadFilter);
  const hiddenProjectKeys = useOrbitSelector((s) => s.hiddenProjectKeys);

  const [expandedMoreProjects, setExpandedMoreProjects] = useState<Record<string, boolean>>({});
  const [renamingThreadKey, setRenamingThreadKey] = useState<string | null>(null);
  const [archivedOpen, setArchivedOpen] = useState(false);
  const [convMenu, setConvMenu] = useState<ActiveConversationMenuState | null>(null);
  const [projMenu, setProjMenu] = useState<ActiveProjectMenuState | null>(null);
  const [deleteProjectTarget, setDeleteProjectTarget] =
    useState<SidebarProjectGroupView | null>(null);

  const nowMs = useSidebarRelativeClock(sidebarOpen);
  const isMac = isMacDesktop();
  const mod = modSymbol();
  const singleHostMode = fleet.hosts.length <= 1;

  const handleOpenConvMenu = useCallback(
    (
      threadKey: string,
      host: HostCardView,
      agent: AgentCardView,
      isPinned: boolean,
      isUnread: boolean,
      anchor: MenuAnchorBox,
    ) => {
      setConvMenu({ threadKey, host, agent, isPinned, isUnread, anchor });
    },
    [],
  );

  const handleCloseConvMenu = useCallback(() => {
    setConvMenu(null);
  }, []);

  const handleCloseProjMenu = useCallback(() => {
    setProjMenu(null);
  }, []);

  const handleStartRename = useCallback((threadKey: string) => {
    setRenamingThreadKey(threadKey);
  }, []);

  const handleFinishRename = useCallback(() => {
    setRenamingThreadKey(null);
  }, []);

  const hostById = useMemo(
    () => new Map(fleet.hosts.map((h) => [h.host_id, h])),
    [fleet.hosts],
  );

  const rawProjects = sidebarView?.projects ?? [];

  const isConvUnread = useCallback(
    (conv: SidebarConversationView): boolean => {
      const isSelected =
        workspacePanel === "conversation" &&
        !isDraftNewSession &&
        selectedHostId === conv.host_id &&
        selectedAgentId === conv.agent.agent_id;
      if (isSelected || conv.agent.is_running) return false;
      return conv.is_unread;
    },
    [workspacePanel, isDraftNewSession, selectedHostId, selectedAgentId],
  );

  // 1. Conditional Needs Attention items (borderless, 0px when clear)
  const attentionItems = useMemo(() => {
    const items: {
      host: HostCardView;
      agent: AgentCardView;
      projectName: string;
      effectiveTitle: string;
    }[] = [];
    for (const conv of sidebarView?.needs_attention ?? []) {
      const tKey = threadScopeKey(conv.host_id, conv.agent.agent_id);
      if (archivedThreads[tKey]) continue;
      const host = hostById.get(conv.host_id);
      if (!host) continue;
      const matchedProj = rawProjects.find(
        (p) =>
          p.host_id === conv.host_id &&
          matchesProjectDirectory(p.directories, conv.agent.cwd),
      );
      items.push({
        host,
        agent: conv.agent,
        projectName: matchedProj?.name || basenameOfProjectPath(conv.agent.cwd),
        effectiveTitle: threadCustomTitles[tKey] || conv.agent.title,
      });
    }
    return items;
  }, [sidebarView?.needs_attention, archivedThreads, hostById, rawProjects, threadCustomTitles]);

  // 2. Conditional Pinned conversations (0px when empty)
  const pinnedEntries = useMemo(() => {
    const ordered: {
      key: string;
      host: HostCardView;
      projectName: string;
      conv: SidebarConversationView;
    }[] = [];
    for (const conv of sidebarView?.pinned_conversations ?? []) {
      const tKey = threadScopeKey(conv.host_id, conv.agent.agent_id);
      if (archivedThreads[tKey]) continue;
      const host = hostById.get(conv.host_id);
      if (!host) continue;
      const matchedProj = rawProjects.find(
        (p) =>
          p.host_id === conv.host_id &&
          matchesProjectDirectory(p.directories, conv.agent.cwd),
      );
      ordered.push({
        key: tKey,
        host,
        projectName: matchedProj?.name || basenameOfProjectPath(conv.agent.cwd),
        conv,
      });
    }
    return ordered;
  }, [sidebarView?.pinned_conversations, archivedThreads, hostById, rawProjects]);

  // 3. Projects tree (sorted & filtered by Rust, plus Desktop-local archived/hidden overlays)
  const displayProjects = useMemo(() => {
    const list: {
      project: SidebarProjectGroupView;
      host: HostCardView;
      visibleConversations: SidebarConversationView[];
    }[] = [];

    for (const proj of rawProjects) {
      const host = hostById.get(proj.host_id);
      if (!host) continue;

      const isActiveTarget =
        selectedHostId === proj.host_id &&
        matchesProjectDirectory(proj.directories, selectedWorkspaceCwd);

      if (
        proj.conversations.length === 0 &&
        !proj.pinned_project_id &&
        hiddenProjectKeys[projectPathScopeKey(proj.host_id, proj.primary_cwd)] &&
        !isActiveTarget
      ) {
        continue;
      }

      const visibleConversations = proj.conversations.filter(
        (c) => !archivedThreads[threadScopeKey(proj.host_id, c.agent.agent_id)],
      );

      const allArchived =
        proj.conversations.length > 0 && visibleConversations.length === 0;
      if (!proj.pinned_project_id && allArchived && !isActiveTarget) {
        continue;
      }

      list.push({
        project: proj,
        host,
        visibleConversations,
      });
    }

    return list;
  }, [
    rawProjects,
    hostById,
    hiddenProjectKeys,
    archivedThreads,
    selectedHostId,
    selectedWorkspaceCwd,
  ]);

  // 4. Restorable Archived Conversations for Footer Drawer
  const archivedEntries = useMemo(() => {
    const keys = Object.keys(archivedThreads);
    if (keys.length === 0) return [];
    const entries: {
      key: string;
      hostId: string;
      agent: AgentCardView;
      projectName: string;
      effectiveTitle: string;
      archivedAtMs: number;
    }[] = [];

    for (const meta of Object.values(archivedThreads)) {
      const detail = hostDetails[meta.hostId];
      const ag = detail?.agents.find((a) => a.agent_id === meta.agentId);
      if (!ag) continue;
      const matchedProj = rawProjects.find(
        (p) =>
          p.host_id === meta.hostId && matchesProjectDirectory(p.directories, ag.cwd),
      );
      entries.push({
        key: meta.key,
        hostId: meta.hostId,
        agent: ag,
        projectName: matchedProj?.name || basenameOfProjectPath(ag.cwd),
        effectiveTitle: threadCustomTitles[meta.key] || ag.title,
        archivedAtMs: meta.archivedAtMs,
      });
    }

    entries.sort((a, b) => b.archivedAtMs - a.archivedAtMs);
    return entries;
  }, [archivedThreads, hostDetails, rawProjects, threadCustomTitles]);

  const isAutomationsActive = workspacePanel === "automations";
  const hasFilterActive = sidebarThreadFilter === "active_unread";

  return (
    <aside className="bg-sidebar border-border flex h-full w-full flex-col border-r [contain:layout_style] select-none">
      {/* 1. Native Window Titlebar Drag Strip (`40px`; single stationary `[◫]` button lives in shell) */}
      <div data-tauri-drag-region className="h-[40px] shrink-0" />

      {/* 2. Top Global Navigation (`30px` rows aligned strictly to `x = 24px` icon / `x = 42px` label) */}
      <div className="space-y-0.5 px-2 pt-0.5 pb-3">
        <button
          type="button"
          onClick={() => orbitActions.startNewThreadDraft()}
          className="group/new text-foreground/82 hover:text-foreground flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 text-left text-[13.5px] transition-colors hover:bg-black/[0.04] dark:hover:bg-white/[0.05]"
        >
          <span className="flex min-w-0 items-center gap-2.5">
            <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
              <Plus className="h-[15.5px] w-[15.5px] stroke-[1.8] opacity-80" />
            </span>
            <span className="truncate">New Conversation</span>
          </span>
          <span className="text-muted-foreground/55 shrink-0 text-[11.5px] opacity-0 transition-opacity group-hover/new:opacity-100">
            {mod}N
          </span>
        </button>

        <button
          type="button"
          onClick={() => orbitActions.openAutomations()}
          className={cn(
            "flex h-[30px] w-full cursor-pointer items-center gap-2.5 rounded-[8px] px-2 text-left text-[13.5px] transition-colors",
            isAutomationsActive
              ? "text-foreground bg-black/[0.06] font-medium dark:bg-white/[0.085]"
              : "text-foreground/78 hover:text-foreground hover:bg-black/[0.04] dark:hover:bg-white/[0.05]",
          )}
        >
          <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
            <Clock className="h-[15px] w-[15px] stroke-[1.75] opacity-80" />
          </span>
          <span className="truncate">Automations</span>
        </button>
      </div>

      {/* 3. Scrollable Mission-Control Body */}
      <ScrollArea className="min-h-0 flex-1" viewportClassName="px-2 pb-2">
        <div className="space-y-3.5">
        {/* 3A. Conditional `Needs Attention` Section — Borderless, Restrained, 0px When Clear */}
        {attentionItems.length > 0 && (
          <div className="orbit-stage-enter space-y-0.5">
            <div className="flex h-[22px] items-center justify-between px-2">
              <span className="text-[11.5px] font-medium text-amber-600 dark:text-amber-400">
                Needs Attention
              </span>
              <span className="tabular-nums text-[11.5px] font-medium text-amber-600/80 dark:text-amber-400/80">
                {attentionItems.length}
              </span>
            </div>

            {attentionItems.map(({ host, agent, projectName, effectiveTitle }) => {
              const active =
                workspacePanel === "conversation" &&
                !isDraftNewSession &&
                selectedHostId === host.host_id &&
                selectedAgentId === agent.agent_id;
              return (
                <button
                  key={`attn-${host.host_id}-${agent.agent_id}`}
                  type="button"
                  onClick={() => orbitActions.selectAgent(host.host_id, agent.agent_id)}
                  className={cn(
                    "flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 text-left text-[13.5px] transition-colors",
                    active
                      ? "text-foreground bg-black/[0.06] font-medium dark:bg-white/[0.085]"
                      : "text-foreground/86 hover:text-foreground hover:bg-black/[0.04] dark:hover:bg-white/[0.05]",
                  )}
                >
                  <span className="flex min-w-0 flex-1 items-center gap-2.5">
                    <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
                      <span className="h-[6.5px] w-[6.5px] rounded-full bg-amber-500" />
                    </span>
                    <span className="truncate">{effectiveTitle}</span>
                  </span>
                  <span className="text-muted-foreground/65 max-w-20 shrink-0 truncate text-[11.5px]">
                    {projectName}
                  </span>
                </button>
              );
            })}
          </div>
        )}

        {/* 3B. Conditional `Pinned` Section — 0px When Empty */}
        {pinnedEntries.length > 0 && (
          <div className="orbit-stage-enter space-y-0.5">
            <div className="flex h-[22px] items-center px-2">
              <span className="text-muted-foreground text-[12px] font-medium">Pinned</span>
            </div>

            {pinnedEntries.map(({ key, host, projectName, conv }) => {
              const selected =
                workspacePanel === "conversation" &&
                !isDraftNewSession &&
                selectedHostId === host.host_id &&
                selectedAgentId === conv.agent.agent_id;
              const unread = isConvUnread(conv);
              const effectiveTitle = threadCustomTitles[key] || conv.agent.title;

              return (
                <SidebarConversationRow
                  key={`pin-${key}`}
                  threadKey={key}
                  host={host}
                  agent={conv.agent}
                  projectName={projectName}
                  effectiveTitle={effectiveTitle}
                  isWorktree={conv.is_worktree}
                  isPinned={true}
                  isUnread={unread}
                  selected={selected}
                  isChild={false}
                  nowMs={nowMs}
                  menuOpen={convMenu?.threadKey === key}
                  isRenaming={renamingThreadKey === key}
                  onOpenMenu={handleOpenConvMenu}
                  onStartRename={handleStartRename}
                  onFinishRename={handleFinishRename}
                />
              );
            })}
          </div>
        )}

        {/* 3C. `Projects` Section (`[⇅]` Sort/Filter + `[📁+]` Add Project) */}
        <div className="space-y-0.5">
          <div className="flex h-[24px] items-center justify-between px-2">
            <span className="text-muted-foreground text-[12px] font-medium">Projects</span>

            <div className="flex items-center gap-0.5">
              <DropdownMenu>
                <SimpleTooltip label="Sort & filter">
                  <DropdownMenuTrigger asChild>
                    <button
                      type="button"
                      aria-label="Sort and filter projects"
                      className={cn(
                        "relative inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-[6px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
                        hasFilterActive
                          ? "text-foreground"
                          : "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      <ListFilter className="h-[13.5px] w-[13.5px] stroke-[1.75]" />
                      {hasFilterActive && (
                        <span className="absolute top-1 right-1 h-[4.5px] w-[4.5px] rounded-full bg-sky-500" />
                      )}
                    </button>
                  </DropdownMenuTrigger>
                </SimpleTooltip>

                <DropdownMenuContent
                  align="start"
                  sideOffset={6}
                  finalFocus={false}
                  className="w-48 p-1.5"
                >
                  <DropdownMenuLabel>Sort Projects</DropdownMenuLabel>
                  <DropdownMenuItem
                    onSelect={() => orbitActions.setSidebarViewPrefs({ projectSort: "recent" })}
                  >
                    <span>Recent activity</span>
                    {sidebarProjectSort === "recent" && (
                      <Check className="h-[13.5px] w-[13.5px] shrink-0" />
                    )}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() =>
                      orbitActions.setSidebarViewPrefs({ projectSort: "alphabetical" })
                    }
                  >
                    <span>Alphabetical</span>
                    {sidebarProjectSort === "alphabetical" && (
                      <Check className="h-[13.5px] w-[13.5px] shrink-0" />
                    )}
                  </DropdownMenuItem>

                  <DropdownMenuSeparator />

                  <DropdownMenuLabel>Show Conversations</DropdownMenuLabel>
                  <DropdownMenuItem
                    onSelect={() => orbitActions.setSidebarViewPrefs({ threadFilter: "all" })}
                  >
                    <span>All conversations</span>
                    {sidebarThreadFilter === "all" && (
                      <Check className="h-[13.5px] w-[13.5px] shrink-0" />
                    )}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() =>
                      orbitActions.setSidebarViewPrefs({ threadFilter: "active_unread" })
                    }
                  >
                    <span>Active & unread</span>
                    {sidebarThreadFilter === "active_unread" && (
                      <Check className="h-[13.5px] w-[13.5px] shrink-0" />
                    )}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>

              <AddProjectButton hosts={fleet.hosts} />
            </div>
          </div>

          {fleet.hosts.length === 0 ? (
            <div className="text-muted-foreground mx-1 mt-1 space-y-2.5 rounded-xl border border-dashed p-4 text-center text-[13px]">
              <p>No host connected</p>
              <button
                type="button"
                onClick={() => orbitActions.openSettings("hosts")}
                className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium hover:opacity-90"
              >
                <Plus className="h-[14px] w-[14px]" /> Connect Host
              </button>
            </div>
          ) : displayProjects.length === 0 ? (
            <div className="text-muted-foreground/75 px-2 py-2 text-[13px]">
              {hasFilterActive ? "No active or unread conversations" : "No projects yet"}
            </div>
          ) : (
            <div className="space-y-1">
              {displayProjects.map(({ project: proj, host, visibleConversations }) => {
                const collapsed = Boolean(collapsedProjectIds[proj.id]);
                const projMenuOpen = projMenu?.project.id === proj.id;

                const hasCollapsedApproval =
                  collapsed &&
                  visibleConversations.some((c) => c.agent.is_awaiting_permission);
                const hasCollapsedRunning =
                  collapsed &&
                  !hasCollapsedApproval &&
                  visibleConversations.some((c) => c.agent.is_running);
                const hasCollapsedUnread =
                  collapsed &&
                  !hasCollapsedApproval &&
                  !hasCollapsedRunning &&
                  visibleConversations.some((c) => isConvUnread(c));

                const selectedIdx = visibleConversations.findIndex(
                  (c) =>
                    workspacePanel === "conversation" &&
                    !isDraftNewSession &&
                    selectedHostId === proj.host_id &&
                    selectedAgentId === c.agent.agent_id,
                );
                const isExpandedAll =
                  Boolean(expandedMoreProjects[proj.id]) ||
                  selectedIdx >= MAX_VISIBLE_CONVERSATIONS_PER_PROJECT;
                const totalCount = visibleConversations.length;
                const hiddenCount = Math.max(
                  0,
                  totalCount - MAX_VISIBLE_CONVERSATIONS_PER_PROJECT,
                );
                const slicedConversations = isExpandedAll
                  ? visibleConversations
                  : visibleConversations.slice(0, MAX_VISIBLE_CONVERSATIONS_PER_PROJECT);

                return (
                  <div key={proj.id} className="space-y-0.5">
                    {/* Project Header Row (`30px`) */}
                    <div
                      onContextMenu={(e) => {
                        e.preventDefault();
                        setProjMenu({
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
                      className="group/proj text-foreground/85 hover:text-foreground flex h-[30px] items-center justify-between gap-1.5 rounded-[8px] px-2 transition-colors hover:bg-black/[0.035] dark:hover:bg-white/[0.045]"
                    >
                      <button
                        type="button"
                        title={proj.primary_cwd}
                        onClick={() => orbitActions.toggleProjectCollapsed(proj.id)}
                        className="flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 text-left"
                      >
                        <span className="text-muted-foreground relative inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
                          {collapsed ? (
                            <Folder className="h-[15px] w-[15px] stroke-[1.75] transition-opacity group-hover/proj:opacity-0" />
                          ) : (
                            <FolderOpen className="h-[15px] w-[15px] stroke-[1.75] transition-opacity group-hover/proj:opacity-0" />
                          )}
                          {collapsed ? (
                            <ChevronRight className="absolute h-[13.5px] w-[13.5px] opacity-0 transition-opacity group-hover/proj:opacity-90" />
                          ) : (
                            <ChevronDown className="absolute h-[13.5px] w-[13.5px] opacity-0 transition-opacity group-hover/proj:opacity-90" />
                          )}
                        </span>

                        <span className="truncate text-[13.5px] font-medium">{proj.name}</span>

                        {!singleHostMode && (
                          <span className="text-muted-foreground/75 ml-0.5 inline-flex shrink-0 items-center gap-1 text-[11.5px]">
                            <HostStateDot state={host.connection_state} />
                            <span className="max-w-18 truncate">{host.display_name}</span>
                          </span>
                        )}
                      </button>

                      {/* Right Slot: Collapsed Status Dot at Rest ↔ Project Actions on Hover */}
                      <div className="flex shrink-0 items-center justify-end">
                        {(hasCollapsedApproval || hasCollapsedRunning || hasCollapsedUnread) && (
                          <span
                            className={cn(
                              "h-[22px] w-[22px] items-center justify-center",
                              projMenuOpen ? "hidden" : "inline-flex group-hover/proj:hidden",
                            )}
                          >
                            {hasCollapsedApproval ? (
                              <span className="h-[6px] w-[6px] rounded-full bg-amber-500" />
                            ) : hasCollapsedRunning ? (
                              <Loader2 className="text-muted-foreground h-[12px] w-[12px] animate-spin" />
                            ) : (
                              <span className="h-[6px] w-[6px] rounded-full bg-sky-500" />
                            )}
                          </span>
                        )}

                        <div
                          className={cn(
                            "items-center gap-0.5",
                            projMenuOpen ? "inline-flex" : "hidden group-hover/proj:inline-flex",
                          )}
                        >
                          {!singleHostMode &&
                            host.connection_state === "disconnected" && (
                              <button
                                type="button"
                                title={`Reconnect ${host.display_name}`}
                                onClick={() => {
                                  void orbitIpc.connectHost(host.host_id);
                                  void orbitIpc.refreshHost(host.host_id);
                                }}
                                className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[5px] hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
                              >
                                <RefreshCw className="h-[13px] w-[13px]" />
                              </button>
                            )}

                          <button
                            type="button"
                            aria-label={`Project options for ${proj.name}`}
                            onClick={(e) => {
                              const r = e.currentTarget.getBoundingClientRect();
                              setProjMenu({
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
                            className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[5px] hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
                          >
                            <MoreHorizontal className="h-[13.5px] w-[13.5px]" />
                          </button>

                          <button
                            type="button"
                            title={`New conversation in ${proj.name}`}
                            onClick={() =>
                              orbitActions.startNewThreadDraft(
                                proj.host_id,
                                proj.primary_cwd,
                              )
                            }
                            className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[5px] hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
                          >
                            <Plus className="h-[14px] w-[14px] stroke-[1.85]" />
                          </button>
                        </div>
                      </div>
                    </div>

                    {/* Project Conversations (Smooth `0fr <-> 1fr` Accordion + Capped at 6 by Default) */}
                    {slicedConversations.length > 0 && (
                      <div
                        data-open={!collapsed ? "true" : "false"}
                        className="orbit-collapse-grid"
                      >
                        <div className="orbit-collapse-inner">
                          <div className="space-y-0.5 pt-0.5">
                            {slicedConversations.map((conv) => {
                              const ag = conv.agent;
                              const tKey = threadScopeKey(proj.host_id, ag.agent_id);
                              const selected =
                                workspacePanel === "conversation" &&
                                !isDraftNewSession &&
                                selectedHostId === proj.host_id &&
                                selectedAgentId === ag.agent_id;
                              const unread = isConvUnread(conv);
                              const effectiveTitle = threadCustomTitles[tKey] || ag.title;

                              return (
                                <SidebarConversationRow
                                  key={ag.agent_id}
                                  threadKey={tKey}
                                  host={host}
                                  agent={ag}
                                  projectName={proj.name}
                                  effectiveTitle={effectiveTitle}
                                  isWorktree={conv.is_worktree}
                                  isPinned={false}
                                  isUnread={unread}
                                  selected={selected}
                                  isChild={Boolean(ag.parent_agent_id)}
                                  nowMs={nowMs}
                                  menuOpen={convMenu?.threadKey === tKey}
                                  isRenaming={renamingThreadKey === tKey}
                                  onOpenMenu={handleOpenConvMenu}
                                  onStartRename={handleStartRename}
                                  onFinishRename={handleFinishRename}
                                />
                              );
                            })}

                            {hiddenCount > 0 && !expandedMoreProjects[proj.id] && !isExpandedAll && (
                              <button
                                type="button"
                                onClick={() =>
                                  setExpandedMoreProjects((prev) => ({
                                    ...prev,
                                    [proj.id]: true,
                                  }))
                                }
                                className="text-muted-foreground/68 hover:text-foreground flex h-[26px] w-full cursor-pointer items-center gap-2.5 rounded-[7px] px-2 text-left text-[12px] transition-colors"
                              >
                                <span className="inline-flex h-[16px] w-[16px] shrink-0" />
                                <span>Show {hiddenCount} more…</span>
                              </button>
                            )}

                            {hiddenCount > 0 &&
                              Boolean(expandedMoreProjects[proj.id]) &&
                              selectedIdx < MAX_VISIBLE_CONVERSATIONS_PER_PROJECT && (
                                <button
                                  type="button"
                                  onClick={() =>
                                    setExpandedMoreProjects((prev) => ({
                                      ...prev,
                                      [proj.id]: false,
                                    }))
                                  }
                                  className="text-muted-foreground/68 hover:text-foreground flex h-[26px] w-full cursor-pointer items-center gap-2.5 rounded-[7px] px-2 text-left text-[12px] transition-colors"
                                >
                                  <span className="inline-flex h-[16px] w-[16px] shrink-0" />
                                  <span>Show less</span>
                                </button>
                              )}
                          </div>
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
        </div>
      </ScrollArea>

      {/* 4. Clean Borderless Footer: Settings + Conditional Archived Recovery */}
      <div className="flex shrink-0 items-center justify-between gap-1 px-2 pt-1 pb-2.5">
        <button
          type="button"
          onClick={() => orbitActions.openSettings("general")}
          className="text-foreground/78 hover:text-foreground flex h-[30px] min-w-0 flex-1 cursor-pointer items-center gap-2.5 rounded-[8px] px-2 text-[13.5px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.055]"
        >
          <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
            <Settings className="h-[15.5px] w-[15.5px] stroke-[1.75] opacity-80" />
          </span>
          <span className="truncate">Settings</span>
        </button>

        {archivedEntries.length > 0 && (
          <Popover open={archivedOpen} onOpenChange={setArchivedOpen}>
            <SimpleTooltip label={`Archived (${archivedEntries.length})`}>
              <PopoverTrigger asChild>
                <button
                  type="button"
                  aria-label="Archived conversations"
                  className="text-muted-foreground hover:text-foreground inline-flex h-[30px] w-[30px] shrink-0 cursor-pointer items-center justify-center rounded-[8px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.055]"
                >
                  <Archive className="h-[14.5px] w-[14.5px] stroke-[1.75]" />
                </button>
              </PopoverTrigger>
            </SimpleTooltip>

            <PopoverContent
              side="top"
              align="start"
              sideOffset={8}
              finalFocus={false}
              className="w-72 space-y-1.5 p-2 select-none"
            >
              <div className="text-muted-foreground flex items-center justify-between px-2 pt-0.5 text-[12px] font-medium">
                <span>Archived Conversations</span>
                <span className="tabular-nums">{archivedEntries.length}</span>
              </div>

              <ScrollArea viewportClassName="max-h-60">
                <div className="space-y-0.5">
                  {archivedEntries.map((item) => (
                    <div
                      key={item.key}
                      onClick={() => {
                        orbitActions.restoreArchivedThread(item.hostId, item.agent.agent_id);
                        setArchivedOpen(false);
                      }}
                      className="group/arch hover:bg-accent flex h-[32px] cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 text-[13px] transition-colors"
                    >
                      <div className="flex min-w-0 flex-1 items-center gap-2">
                        <span className="text-foreground truncate">{item.effectiveTitle}</span>
                        <span className="text-muted-foreground/70 max-w-20 shrink-0 truncate text-[11.5px]">
                          {item.projectName}
                        </span>
                      </div>

                      <div
                        className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover/arch:opacity-100"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <button
                          type="button"
                          title="Restore conversation"
                          onClick={() => {
                            orbitActions.restoreArchivedThread(item.hostId, item.agent.agent_id);
                          }}
                          className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[5px] hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
                        >
                          <RotateCcw className="h-[13px] w-[13px] stroke-[1.75]" />
                        </button>

                        <button
                          type="button"
                          title="Delete permanently"
                          onClick={() => {
                            void orbitActions.deleteThreadPermanently(
                              item.hostId,
                              item.agent.agent_id,
                            );
                          }}
                          className="inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[5px] text-rose-600/80 hover:bg-rose-500/10 hover:text-rose-600 dark:text-rose-400/80 dark:hover:text-rose-400"
                        >
                          <Trash2 className="h-[13px] w-[13px] stroke-[1.75]" />
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              </ScrollArea>
            </PopoverContent>
          </Popover>
        )}
      </div>

      {/* Shared Single-Instance Context/Options Menus (0 Radix instances per row!) */}
      <SharedConversationMenu
        target={convMenu}
        onClose={handleCloseConvMenu}
        onStartRename={handleStartRename}
      />
      <SharedProjectMenu
        target={projMenu}
        isMac={isMac}
        onClose={handleCloseProjMenu}
        onRequestDelete={setDeleteProjectTarget}
      />
      <DeleteProjectDialog
        project={deleteProjectTarget}
        archivedThreads={archivedThreads}
        onClose={() => setDeleteProjectTarget(null)}
      />
    </aside>
  );
});
