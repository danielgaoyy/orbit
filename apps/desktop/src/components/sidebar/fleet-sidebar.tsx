import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Archive,
  ArrowDownAZ,
  ArrowUpDown,
  Check,
  ChevronsDownUp,
  ChevronsUpDown,
  CircleDot,
  Clock,
  List,
  ListFilter,
  Plus,
  Search,
  Settings,
  Sparkles,
  X,
} from "lucide-react";
import { modSymbol } from "@/lib/keybindings";
import {
  type AgentCardView,
  type HostCardView,
  type SidebarProjectGroupView,
  compareSemver,
  isMacDesktop,
  normalizeSemver,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { normalizeProjectPath } from "@/lib/projects";
import { useImeCompositionGuard } from "@/lib/ime";
import { useI18n } from "@/lib/i18n";
import { SidebarConversationRow } from "@/components/sidebar/sidebar-conversation-row";
import {
  type ActiveConversationMenuState,
  type ActiveProjectMenuState,
  type ActiveWorkspaceMenuState,
  type CreateWorkspaceRequest,
  DeleteConversationDialog,
  type DeleteConversationTarget,
  DeleteProjectDialog,
  type MenuAnchorBox,
  SharedConversationMenu,
  SharedProjectMenu,
  SharedWorkspaceMenu,
} from "@/components/sidebar/sidebar-menus";
import {
  buildRunningParentThreadKeys,
  buildSidebarDisplayModel,
  isSidebarConversationUnread,
} from "@/components/sidebar/sidebar-model";
import { AddProjectButton } from "@/components/sidebar/sidebar-primitives";
import {
  type InlineCreateGroupTarget,
  SidebarProjectSection,
} from "@/components/sidebar/sidebar-project-section";
import { useSidebarDrag } from "@/components/sidebar/use-sidebar-drag";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import { ShortcutKbd } from "@/components/ui/kbd";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  orbitActions,
  resolveFleetTargetVersion,
  threadScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export const FleetSidebar = memo(function FleetSidebar() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  const stageRunningSubagentMeta = useOrbitSelector((s) => s.stageRunningSubagentMeta);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedWorkspaceCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const collapsedProjectIds = useOrbitSelector((s) => s.collapsedProjectIds);
  const collapsedWorkspaceIds = useOrbitSelector((s) => s.collapsedWorkspaceIds);
  const threadCustomTitles = useOrbitSelector((s) => s.threadCustomTitles);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const threadManualOrder = useOrbitSelector((s) => s.threadManualOrder);
  const sidebarProjectSort = useOrbitSelector((s) => s.sidebarProjectSort);
  const sidebarThreadFilter = useOrbitSelector((s) => s.sidebarThreadFilter);
  const sidebarFilterFocusNonce = useOrbitSelector((s) => s.sidebarFilterFocusNonce);
  const hiddenProjectKeys = useOrbitSelector((s) => s.hiddenProjectKeys);
  const fleetHydrated = useOrbitSelector((s) => s.fleetHydrated);
  const appVersion = useOrbitSelector((s) => s.appVersion);
  const releaseUpdateInfo = useOrbitSelector((s) => s.releaseUpdateInfo);
  const fleetUpdate = useOrbitSelector((s) => s.fleetUpdate);

  const { t } = useI18n();
  const ime = useImeCompositionGuard();
  const isMac = isMacDesktop();
  const mod = modSymbol();
  const singleHostMode = fleet.hosts.length <= 1;
  const isAutomationsActive = workspacePanel === "automations";
  const hasFilterActive = sidebarThreadFilter === "active_unread";

  const [expandedMoreProjects, setExpandedMoreProjects] = useState<Record<string, boolean>>({});
  const [filterOpen, setFilterOpen] = useState(false);
  const [filterQuery, setFilterQuery] = useState("");
  const [isFilterFocused, setIsFilterFocused] = useState(false);
  const [keyboardFocusedNavId, setKeyboardFocusedNavId] = useState<string | null>(null);
  const filterInputRef = useRef<HTMLInputElement | null>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const scrollViewportRef = useRef<HTMLDivElement | null>(null);
  const navRowRefs = useRef<Map<string, HTMLElement>>(new Map());

  const [renamingThreadKey, setRenamingThreadKey] = useState<string | null>(null);
  const [renamingProjectId, setRenamingProjectId] = useState<string | null>(null);
  const [renamingWorkspaceId, setRenamingWorkspaceId] = useState<string | null>(null);
  const [creatingGroupTarget, setCreatingGroupTarget] = useState<InlineCreateGroupTarget | null>(
    null,
  );
  const [convMenu, setConvMenu] = useState<ActiveConversationMenuState | null>(null);
  const [projMenu, setProjMenu] = useState<ActiveProjectMenuState | null>(null);
  const [wsMenu, setWsMenu] = useState<ActiveWorkspaceMenuState | null>(null);
  const [deleteConversationTarget, setDeleteConversationTarget] =
    useState<DeleteConversationTarget | null>(null);
  const [deleteProjectTarget, setDeleteProjectTarget] =
    useState<SidebarProjectGroupView | null>(null);

  const trimmedFilter = filterQuery.trim();
  const isFilteringThreads = trimmedFilter.length > 0;
  const showFilterBar = filterOpen || isFilteringThreads;

  // Respond to global `⌘F` / `Ctrl+F` (`orbitActions.focusSidebarFilter()`)
  const lastFilterNonceRef = useRef(sidebarFilterFocusNonce);
  useEffect(() => {
    if (sidebarFilterFocusNonce === lastFilterNonceRef.current) return;
    lastFilterNonceRef.current = sidebarFilterFocusNonce;
    const active = document.activeElement as HTMLElement | null;
    if (active && active !== filterInputRef.current) {
      previousFocusRef.current = active;
    }
    setFilterOpen(true);
    window.requestAnimationFrame(() => {
      filterInputRef.current?.focus();
      filterInputRef.current?.select();
    });
  }, [sidebarFilterFocusNonce]);

  const openFilterBar = useCallback(() => {
    const active = document.activeElement as HTMLElement | null;
    if (active && active !== filterInputRef.current) {
      previousFocusRef.current = active;
    }
    setFilterOpen(true);
    window.requestAnimationFrame(() => {
      filterInputRef.current?.focus();
      filterInputRef.current?.select();
    });
  }, []);

  const hasFleetUpdatesAvailable = useMemo(() => {
    if (releaseUpdateInfo?.has_newer_version) return true;
    const target = resolveFleetTargetVersion(appVersion, releaseUpdateInfo);
    if (!target) return false;
    return fleet.hosts.some((h) => {
      const online =
        h.connection_state === "online_direct" || h.connection_state === "online_relay";
      if (!online && !h.ssh_alias) return false;
      const v = normalizeSemver(h.daemon_version);
      return Boolean(v && compareSemver(v, target) < 0);
    });
  }, [appVersion, releaseUpdateInfo, fleet.hosts]);

  const isFleetUpdating =
    fleetUpdate.phase === "upgrading_hosts" || fleetUpdate.phase === "upgrading_desktop";

  const [primaryRouterHostId, setPrimaryRouterHostId] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    void orbitIpc
      .getPrimaryRouterSelection()
      .then((sel) => {
        if (active && sel?.host_id) {
          setPrimaryRouterHostId(sel.host_id);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [fleet.hosts.length]);

  // O(N) lookup of parent threads with active subagents — zero `s.stages` topic subscription!
  const runningParentThreadKeys = useMemo(
    () => buildRunningParentThreadKeys(hostDetails, archivedThreads, stageRunningSubagentMeta),
    [hostDetails, archivedThreads, stageRunningSubagentMeta],
  );

  // Full pure sidebar display model (Attention, Pinned, Projects, Groups, Filter, Nav order)
  const {
    displayProjects,
    filteredAttentionItems,
    filteredPinnedEntries,
    projectRenderModels,
    visibleNavItems,
  } = useMemo(
    () =>
      buildSidebarDisplayModel({
        hosts: fleet.hosts,
        sidebarView,
        hostDetails,
        archivedThreads,
        hiddenProjectKeys,
        threadCustomTitles,
        collapsedProjectIds,
        collapsedWorkspaceIds,
        sidebarProjectSort,
        threadManualOrder,
        workspacePanel,
        isDraftNewSession,
        selectedHostId,
        selectedAgentId,
        selectedWorkspaceCwd,
        expandedMoreProjects,
        filterQuery,
      }),
    [
      fleet.hosts,
      sidebarView,
      hostDetails,
      archivedThreads,
      hiddenProjectKeys,
      threadCustomTitles,
      collapsedProjectIds,
      collapsedWorkspaceIds,
      sidebarProjectSort,
      threadManualOrder,
      workspacePanel,
      isDraftNewSession,
      selectedHostId,
      selectedAgentId,
      selectedWorkspaceCwd,
      expandedMoreProjects,
      filterQuery,
    ],
  );

  // Raw pinned key set so `Needs Attention` context menus show accurate Pin/Unpin state
  const rawPinnedKeySet = useMemo(() => {
    const set = new Set<string>();
    for (const c of sidebarView?.pinned_conversations ?? []) {
      set.add(threadScopeKey(c.host_id, c.agent.agent_id));
    }
    return set;
  }, [sidebarView?.pinned_conversations]);

  const archivedCount = useMemo(() => {
    const seen = new Set<string>();
    for (const [hostId, detail] of Object.entries(hostDetails)) {
      for (const ag of detail.agents) {
        if (ag.background) continue;
        const tKey = threadScopeKey(hostId, ag.agent_id);
        if (!ag.archived_at_rfc3339 && !archivedThreads[tKey]) continue;
        seen.add(tKey);
      }
    }
    return seen.size;
  }, [archivedThreads, hostDetails]);

  const fleetHealth = useMemo(() => {
    let online = 0;
    let connecting = 0;
    let offline = 0;
    for (const h of fleet.hosts) {
      if (h.connection_state === "online_direct" || h.connection_state === "online_relay") {
        online += 1;
      } else if (
        h.connection_state === "connecting_direct" ||
        h.connection_state === "connecting_relay"
      ) {
        connecting += 1;
      } else {
        offline += 1;
      }
    }
    return {
      total: fleet.hosts.length,
      online,
      connecting,
      offline,
      showFooterPill: fleet.hosts.length > 1 || offline > 0 || connecting > 0,
    };
  }, [fleet.hosts]);

  const anyProjectExpanded = useMemo(
    () => displayProjects.some(({ project }) => !collapsedProjectIds[project.id]),
    [displayProjects, collapsedProjectIds],
  );

  // Pointer-driven zero-rerender drag engine (with edge auto-scroll)
  const {
    draggingThread,
    threadDropIndicator,
    dragOverDropId,
    draggingProjectId,
    projectDropIndicator,
    floatingPreviewRef,
    syncScopeAgentIds,
    registerProjectRow,
    registerThreadRow,
    registerWorkspaceHeader,
    registerInboxArea,
    shouldSuppressThreadClick,
    shouldSuppressProjectClick,
    startThreadDrag,
    startProjectDrag,
  } = useSidebarDrag({
    displayProjects,
    sidebarProjectSort,
    scrollViewportRef,
  });

  // Sync latest full scope orders inside an effect (never mutating refs during render)
  useEffect(() => {
    const nextScopes = new Map<string, string[]>();
    for (const model of projectRenderModels) {
      nextScopes.set(
        model.projThreadScopeKey,
        model.unclassifiedConversations.map((c) => c.agent.agent_id),
      );
      for (const ws of model.curatedWorkspaces) {
        nextScopes.set(
          ws.threadScopeKey,
          ws.visiblePeers.map((c) => c.agent.agent_id),
        );
      }
    }
    syncScopeAgentIds(nextScopes);
  }, [projectRenderModels, syncScopeAgentIds]);

  // Keep keyboard highlight synced with live filtered results while filter input is focused
  useEffect(() => {
    if (!isFilterFocused) {
      setKeyboardFocusedNavId(null);
      return;
    }
    if (!isFilteringThreads) {
      setKeyboardFocusedNavId((prev) =>
        prev && visibleNavItems.some((it) => it.navId === prev) ? prev : null,
      );
      return;
    }
    setKeyboardFocusedNavId((prev) => {
      if (prev && visibleNavItems.some((it) => it.navId === prev)) return prev;
      return visibleNavItems[0]?.navId ?? null;
    });
  }, [isFilterFocused, isFilteringThreads, trimmedFilter, visibleNavItems]);

  // Stable callbacks to keep `SidebarConversationRow` & `SidebarProjectSection` `memo` intact
  const registerNavRow = useCallback((navId: string, el: HTMLElement | null) => {
    if (el) navRowRefs.current.set(navId, el);
    else navRowRefs.current.delete(navId);
  }, []);

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

  const handleCloseConvMenu = useCallback(() => setConvMenu(null), []);
  const handleCloseProjMenu = useCallback(() => setProjMenu(null), []);
  const handleCloseWsMenu = useCallback(() => setWsMenu(null), []);
  const handleStartRenameThread = useCallback((threadKey: string) => {
    setRenamingThreadKey(threadKey);
  }, []);
  const handleFinishRenameThread = useCallback(() => setRenamingThreadKey(null), []);
  const handleStartRenameProject = useCallback((proj: SidebarProjectGroupView) => {
    setRenamingProjectId(proj.id);
  }, []);
  const handleFinishRenameProject = useCallback(() => setRenamingProjectId(null), []);
  const handleStartRenameWorkspace = useCallback((workspaceId: string) => {
    setRenamingWorkspaceId(workspaceId);
  }, []);
  const handleFinishRenameWorkspace = useCallback(() => setRenamingWorkspaceId(null), []);
  const handleFinishCreateGroup = useCallback(() => setCreatingGroupTarget(null), []);
  const handleSetExpandedMore = useCallback((projectId: string, expanded: boolean) => {
    setExpandedMoreProjects((prev) => ({ ...prev, [projectId]: expanded }));
  }, []);

  const rawProjects = sidebarView?.projects ?? [];
  const startInlineCreateGroup = useCallback(
    (req: CreateWorkspaceRequest) => {
      const normReqRoot = normalizeProjectPath(req.projectRoot);
      const matchedProj =
        rawProjects.find((p) => req.projectId && p.id === req.projectId) ??
        rawProjects.find(
          (p) =>
            p.host_id === req.hostId &&
            (normalizeProjectPath(p.primary_cwd) === normReqRoot ||
              normReqRoot.startsWith(`${normalizeProjectPath(p.primary_cwd)}/`)),
        ) ??
        null;
      if (!matchedProj) return;
      if (collapsedProjectIds[matchedProj.id]) {
        orbitActions.toggleProjectCollapsed(matchedProj.id);
      }
      window.requestAnimationFrame(() => {
        setCreatingGroupTarget({
          projectId: matchedProj.id,
          hostId: req.hostId,
          projectRoot: matchedProj.primary_cwd,
          moveAgentId: req.moveAgentId ?? null,
        });
      });
    },
    [rawProjects, collapsedProjectIds],
  );

  const handleFilterKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (ime.shouldIgnoreKeyDown(e)) return;
    if (e.key === "ArrowDown") {
      if (visibleNavItems.length === 0) return;
      e.preventDefault();
      const curIdx = keyboardFocusedNavId
        ? visibleNavItems.findIndex((it) => it.navId === keyboardFocusedNavId)
        : -1;
      const next = visibleNavItems[(curIdx + 1) % visibleNavItems.length];
      setKeyboardFocusedNavId(next.navId);
      navRowRefs.current.get(next.navId)?.scrollIntoView({ block: "nearest" });
      return;
    }
    if (e.key === "ArrowUp") {
      if (visibleNavItems.length === 0) return;
      e.preventDefault();
      const curIdx = keyboardFocusedNavId
        ? visibleNavItems.findIndex((it) => it.navId === keyboardFocusedNavId)
        : -1;
      const nextIdx = curIdx <= 0 ? visibleNavItems.length - 1 : curIdx - 1;
      const next = visibleNavItems[nextIdx];
      setKeyboardFocusedNavId(next.navId);
      navRowRefs.current.get(next.navId)?.scrollIntoView({ block: "nearest" });
      return;
    }
    if (e.key === "Enter") {
      const target =
        (keyboardFocusedNavId
          ? visibleNavItems.find((it) => it.navId === keyboardFocusedNavId)
          : undefined) ?? (isFilteringThreads ? visibleNavItems[0] : undefined);
      if (!target) return;
      e.preventDefault();
      setKeyboardFocusedNavId(null);
      filterInputRef.current?.blur();
      orbitActions.selectAgent(target.hostId, target.agentId);
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      e.nativeEvent.stopImmediatePropagation?.();
      setFilterQuery("");
      setFilterOpen(false);
      setKeyboardFocusedNavId(null);
      filterInputRef.current?.blur();
      const prev = previousFocusRef.current;
      previousFocusRef.current = null;
      if (prev && prev.isConnected && prev !== filterInputRef.current) {
        prev.focus({ preventScroll: true });
      } else {
        orbitActions.focusComposer();
      }
    }
  };

  return (
    <aside className="bg-surface-base flex h-full w-full flex-col select-none">
      {/* 1. Native Window Titlebar Drag Strip (`40px`; `[◫]` lives left in shell, symmetric `[🔍]` lives right) */}
      <div className="flex h-[40px] shrink-0 items-center pr-2">
        <div data-tauri-drag-region className="h-full flex-1" />
        <IconButton
          size="md"
          active={showFilterBar}
          tooltip={t("sidebar.filterThreadsAria")}
          shortcut={`${mod}F`}
          indicatorTone={isFilteringThreads ? "info" : "none"}
          onMouseDown={(e) => {
            if (showFilterBar && !isFilteringThreads) {
              e.preventDefault();
            }
          }}
          onClick={() => {
            if (showFilterBar && !isFilteringThreads) {
              setFilterOpen(false);
            } else {
              openFilterBar();
            }
          }}
          className={cn(isMac && "translate-y-[1px]")}
        >
          <Search className="size-[15px] stroke-[1.75]" />
        </IconButton>
      </div>

      {/* 2. Top Global Navigation: Primary Action (`New Conversation`) + Secondary View (`Automations`) */}
      <div className="space-y-1 px-2 pt-0.5 pb-2.5">
        <button
          type="button"
          onClick={() => orbitActions.startNewThreadDraft()}
          className="group/new orbit-press bg-surface-raised border-border-subtle text-ink-primary hover:border-border-default flex h-[32px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] border px-2 text-left text-[13px] font-medium shadow-2xs transition-[border-color,background-color,color,transform]"
        >
          <span className="flex min-w-0 items-center gap-2.5">
            <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
              <Plus className="text-ink-secondary group-hover/new:text-ink-primary size-[14.5px] stroke-[2] transition-colors" />
            </span>
            <span className="truncate">{t("sidebar.newConversation")}</span>
          </span>
          <ShortcutKbd
            keys={`${mod}N`}
            size="xs"
            variant="subtle"
            className="shrink-0 opacity-0 transition-opacity group-hover/new:opacity-100"
          />
        </button>

        <button
          type="button"
          onClick={() => orbitActions.openAutomations()}
          className={cn(
            "flex h-[30px] w-full cursor-pointer items-center gap-2.5 rounded-[8px] px-2 text-left text-[13px] transition-colors",
            isAutomationsActive
              ? "bg-accent text-ink-primary font-medium"
              : "text-ink-secondary hover:bg-accent/55 hover:text-ink-primary",
          )}
        >
          <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
            <Clock
              className={cn(
                "size-[14.5px] stroke-[1.75]",
                isAutomationsActive ? "text-ink-primary" : "text-ink-muted",
              )}
            />
          </span>
          <span className="truncate">{t("sidebar.automations")}</span>
        </button>

        {(() => {
          const orderedHosts = primaryRouterHostId
            ? [
                ...fleet.hosts.filter((h) => h.host_id === primaryRouterHostId),
                ...fleet.hosts.filter((h) => h.host_id !== primaryRouterHostId),
              ]
            : fleet.hosts;
          let activeRouter: { hostId: string; hostName: string; agent: AgentCardView } | null = null;
          for (const h of orderedHosts) {
            const detail = hostDetails[h.host_id];
            if (!detail) continue;
            const found = detail.agents.find(
              (a) =>
                !archivedThreads[threadScopeKey(h.host_id, a.agent_id)] &&
                (a.handle === "router" || a.title.includes("Personal Router")),
            );
            if (found) {
              activeRouter = {
                hostId: h.host_id,
                hostName: h.display_name,
                agent: found,
              };
              break;
            }
          }
          const isRouterActive =
            activeRouter !== null &&
            workspacePanel === "conversation" &&
            !isDraftNewSession &&
            selectedHostId === activeRouter.hostId &&
            selectedAgentId === activeRouter.agent.agent_id;

          return (
            <button
              type="button"
              onClick={() => {
                if (activeRouter) {
                  orbitActions.selectAgent(activeRouter.hostId, activeRouter.agent.agent_id);
                } else {
                  orbitActions.openSettings("personal_router");
                }
              }}
              className={cn(
                "flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 text-left text-[13px] transition-colors",
                isRouterActive
                  ? "bg-accent text-ink-primary font-medium"
                  : "text-ink-secondary hover:bg-accent/55 hover:text-ink-primary",
              )}
            >
              <span className="flex min-w-0 flex-1 items-center gap-2.5">
                <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
                  <Sparkles
                    className={cn(
                      "size-[14.5px] stroke-[1.75]",
                      isRouterActive ? "text-ink-primary" : "text-ink-muted",
                    )}
                  />
                </span>
                <span className="truncate">{t("sidebar.personalRouter")}</span>
              </span>
              {activeRouter && (
                <span className="text-ink-faint max-w-20 shrink-0 truncate text-[11px]">
                  {activeRouter.hostName}
                </span>
              )}
            </button>
          );
        })()}
      </div>

      {/* 2B. On-Demand Sidebar Filter Bar (0px at rest; expands above ALL list sections via Titlebar `🔍` or `⌘F`) */}
      {showFilterBar && (
        <div className="orbit-stage-enter border-border-subtle/75 border-t px-2 pt-2.5 pb-1.5">
          <div className="group/filter bg-surface-recessed focus-within:border-border-strong focus-within:bg-surface-raised flex h-[28px] items-center gap-2 rounded-[7px] border border-transparent px-2 transition-colors">
            <Search className="text-ink-faint group-focus-within/filter:text-ink-secondary size-[13px] shrink-0 stroke-[1.85] transition-colors" />
            <input
              ref={filterInputRef}
              type="text"
              value={filterQuery}
              onFocus={(e) => {
                const prev = e.relatedTarget as HTMLElement | null;
                if (prev && prev !== filterInputRef.current) {
                  previousFocusRef.current = prev;
                }
                setIsFilterFocused(true);
              }}
              onBlur={() => {
                setIsFilterFocused(false);
                setKeyboardFocusedNavId(null);
                if (filterQuery.trim().length === 0) {
                  setFilterOpen(false);
                }
              }}
              onChange={(e) => setFilterQuery(e.target.value)}
              {...ime.compositionProps}
              onKeyDown={handleFilterKeyDown}
              placeholder={t("sidebar.filterThreadsPlaceholder")}
              aria-label={t("sidebar.filterThreadsAria")}
              className="text-ink-primary placeholder:text-ink-faint min-w-0 flex-1 bg-transparent text-[12.5px] select-text focus:outline-none"
            />
            {isFilteringThreads && (
              <span className="text-ink-faint shrink-0 font-mono text-[10.5px] tabular-nums">
                {visibleNavItems.length}
              </span>
            )}
            <button
              type="button"
              aria-label={t("sidebar.clearThreadFilter")}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                if (filterQuery.length > 0) {
                  setFilterQuery("");
                  setKeyboardFocusedNavId(null);
                  filterInputRef.current?.focus({ preventScroll: true });
                } else {
                  setFilterOpen(false);
                }
              }}
              className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex h-[18px] w-[18px] shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors"
            >
              <X className="size-[11.5px]" />
            </button>
          </div>
        </div>
      )}

      {/* 3. Scrollable Mission-Control Body */}
      <ScrollArea
        viewportRef={scrollViewportRef}
        className={cn("min-h-0 flex-1", !showFilterBar && "border-border-subtle/75 border-t")}
        viewportClassName={cn("px-2 pb-2", showFilterBar ? "pt-1.5" : "pt-2.5")}
      >
        <div className="space-y-3.5">
          {/* 3A. Conditional `Needs Attention` Section — Full `SidebarConversationRow`, 0px When Clear */}
          {filteredAttentionItems.length > 0 && (
            <div className="orbit-stage-enter space-y-0.5">
              <div className="flex h-[22px] items-center justify-between px-2">
                <span className="text-signal-warning text-[11px] font-medium tracking-[0.03em] uppercase">
                  {t("sidebar.needsAttention")}
                </span>
                <span className="text-signal-warning/80 text-[11px] font-medium tabular-nums">
                  {filteredAttentionItems.length}
                </span>
              </div>

              {filteredAttentionItems.map(
                ({ key, navId, host, conv, projectName, effectiveTitle, titleIndices }) => {
                  const selected =
                    workspacePanel === "conversation" &&
                    !isDraftNewSession &&
                    selectedHostId === host.host_id &&
                    selectedAgentId === conv.agent.agent_id;
                  const unread = isSidebarConversationUnread(conv, {
                    workspacePanel,
                    isDraftNewSession,
                    selectedHostId,
                    selectedAgentId,
                  });

                  return (
                    <SidebarConversationRow
                      key={`attn-${key}`}
                      threadKey={key}
                      navId={navId}
                      host={host}
                      agent={conv.agent}
                      projectName={projectName}
                      effectiveTitle={effectiveTitle}
                      contextLabel={projectName}
                      isWorktree={conv.is_worktree}
                      isPinned={rawPinnedKeySet.has(key)}
                      isUnread={unread}
                      hasRunningSubagents={
                        Boolean(conv.agent.has_running_subagents) ||
                        runningParentThreadKeys.has(key)
                      }
                      selected={selected}
                      keyboardFocused={keyboardFocusedNavId === navId}
                      highlightIndices={titleIndices}
                      isChild={false}
                      menuOpen={convMenu?.threadKey === key}
                      isRenaming={renamingThreadKey === key}
                      onRegisterNavRow={registerNavRow}
                      onOpenMenu={handleOpenConvMenu}
                      onStartRename={handleStartRenameThread}
                      onFinishRename={handleFinishRenameThread}
                    />
                  );
                },
              )}
            </div>
          )}

          {/* 3B. Conditional `Pinned` Section — 0px When Empty */}
          {filteredPinnedEntries.length > 0 && (
            <div className="orbit-stage-enter space-y-0.5">
              <div className="flex h-[22px] items-center px-2">
                <span className="text-ink-tertiary text-[11px] font-medium tracking-[0.03em] uppercase">
                  {t("sidebar.pinned")}
                </span>
              </div>

              {filteredPinnedEntries.map(
                ({ key, host, projectName, conv, navId, effectiveTitle, titleIndices }) => {
                  const selected =
                    workspacePanel === "conversation" &&
                    !isDraftNewSession &&
                    selectedHostId === host.host_id &&
                    selectedAgentId === conv.agent.agent_id;
                  const unread = isSidebarConversationUnread(conv, {
                    workspacePanel,
                    isDraftNewSession,
                    selectedHostId,
                    selectedAgentId,
                  });

                  return (
                    <SidebarConversationRow
                      key={`pin-${key}`}
                      threadKey={key}
                      navId={navId}
                      host={host}
                      agent={conv.agent}
                      projectName={projectName}
                      effectiveTitle={effectiveTitle}
                      contextLabel={displayProjects.length > 1 ? projectName : undefined}
                      isWorktree={conv.is_worktree}
                      isPinned={true}
                      isUnread={unread}
                      hasRunningSubagents={
                        Boolean(conv.agent.has_running_subagents) ||
                        runningParentThreadKeys.has(key)
                      }
                      selected={selected}
                      keyboardFocused={keyboardFocusedNavId === navId}
                      highlightIndices={titleIndices}
                      isChild={false}
                      menuOpen={convMenu?.threadKey === key}
                      isRenaming={renamingThreadKey === key}
                      onRegisterNavRow={registerNavRow}
                      onOpenMenu={handleOpenConvMenu}
                      onStartRename={handleStartRenameThread}
                      onFinishRename={handleFinishRenameThread}
                    />
                  );
                },
              )}
            </div>
          )}

          {/* Global Empty Search State (shown only when ALL sections have 0 matches) */}
          {isFilteringThreads && visibleNavItems.length === 0 && (
            <div className="text-ink-muted flex items-center justify-between gap-2 px-2 py-2 text-[12.5px]">
              <span>{t("sidebar.noMatchingThreads")}</span>
              <button
                type="button"
                onClick={() => {
                  setFilterQuery("");
                  setFilterOpen(false);
                  setKeyboardFocusedNavId(null);
                }}
                className="text-ink-muted hover:text-ink-primary shrink-0 cursor-pointer text-[12px] transition-colors"
              >
                {t("common.reset")}
              </button>
            </div>
          )}

          {/* 3C. `Projects` Section (`[⇅]` Sort/Filter + `[📁+]` Add Project — strictly 2 buttons) */}
          {(!isFilteringThreads || projectRenderModels.length > 0) && (
          <div className="space-y-0.5">
            <div className="flex h-[24px] items-center justify-between px-2">
              <span className="text-ink-tertiary text-[11px] font-medium tracking-[0.03em] uppercase">
                {t("sidebar.projects")}
              </span>

              <div className="flex items-center gap-0.5">
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <IconButton
                      size="xs"
                      active={hasFilterActive}
                      tooltip={t("sidebar.sortAndFilter")}
                      aria-label={t("sidebar.sortAndFilterAria")}
                      indicatorTone={hasFilterActive ? "info" : "none"}
                    >
                      <ListFilter className="size-[13.5px] stroke-[1.75]" />
                    </IconButton>
                  </DropdownMenuTrigger>

                  <DropdownMenuContent align="start" sideOffset={6} finalFocus={false}>
                    <DropdownMenuGroup>
                      <DropdownMenuLabel>{t("sidebar.sortBy")}</DropdownMenuLabel>
                      <DropdownMenuItem
                        onSelect={() => orbitActions.setSidebarViewPrefs({ projectSort: "recent" })}
                      >
                        <span className="flex min-w-0 items-center gap-2 whitespace-nowrap">
                          <Clock
                            className={cn(
                              "size-[13.5px] stroke-[1.75]",
                              sidebarProjectSort === "recent"
                                ? "text-ink-primary"
                                : "text-ink-muted",
                            )}
                          />
                          <span>{t("sidebar.sortRecent")}</span>
                        </span>
                        {sidebarProjectSort === "recent" && (
                          <Check className="text-ink-primary size-[13px] stroke-[2]" />
                        )}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={() =>
                          orbitActions.setSidebarViewPrefs({ projectSort: "alphabetical" })
                        }
                      >
                        <span className="flex min-w-0 items-center gap-2 whitespace-nowrap">
                          <ArrowDownAZ
                            className={cn(
                              "size-[13.5px] stroke-[1.75]",
                              sidebarProjectSort === "alphabetical"
                                ? "text-ink-primary"
                                : "text-ink-muted",
                            )}
                          />
                          <span>{t("sidebar.sortName")}</span>
                        </span>
                        {sidebarProjectSort === "alphabetical" && (
                          <Check className="text-ink-primary size-[13px] stroke-[2]" />
                        )}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={() => orbitActions.setSidebarViewPrefs({ projectSort: "manual" })}
                      >
                        <span className="flex min-w-0 items-center gap-2 whitespace-nowrap">
                          <ArrowUpDown
                            className={cn(
                              "size-[13.5px] stroke-[1.75]",
                              sidebarProjectSort === "manual"
                                ? "text-ink-primary"
                                : "text-ink-muted",
                            )}
                          />
                          <span>{t("sidebar.sortCustom")}</span>
                        </span>
                        {sidebarProjectSort === "manual" && (
                          <Check className="text-ink-primary size-[13px] stroke-[2]" />
                        )}
                      </DropdownMenuItem>
                    </DropdownMenuGroup>

                    <DropdownMenuSeparator />

                    <DropdownMenuGroup>
                      <DropdownMenuLabel>{t("sidebar.show")}</DropdownMenuLabel>
                      <DropdownMenuItem
                        onSelect={() => orbitActions.setSidebarViewPrefs({ threadFilter: "all" })}
                      >
                        <span className="flex min-w-0 items-center gap-2 whitespace-nowrap">
                          <List
                            className={cn(
                              "size-[13.5px] stroke-[1.75]",
                              sidebarThreadFilter === "all"
                                ? "text-ink-primary"
                                : "text-ink-muted",
                            )}
                          />
                          <span>{t("sidebar.filterAll")}</span>
                        </span>
                        {sidebarThreadFilter === "all" && (
                          <Check className="text-ink-primary size-[13px] stroke-[2]" />
                        )}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        onSelect={() =>
                          orbitActions.setSidebarViewPrefs({ threadFilter: "active_unread" })
                        }
                      >
                        <span className="flex min-w-0 items-center gap-2 whitespace-nowrap">
                          <CircleDot
                            className={cn(
                              "size-[13.5px] stroke-[1.75]",
                              sidebarThreadFilter === "active_unread"
                                ? "text-ink-primary"
                                : "text-ink-muted",
                            )}
                          />
                          <span>{t("sidebar.filterActiveUnread")}</span>
                        </span>
                        {sidebarThreadFilter === "active_unread" && (
                          <Check className="text-ink-primary size-[13px] stroke-[2]" />
                        )}
                      </DropdownMenuItem>
                    </DropdownMenuGroup>

                    <DropdownMenuSeparator />

                    <DropdownMenuGroup>
                      {displayProjects.length > 1 && (
                        <DropdownMenuItem
                          onSelect={() => {
                            orbitActions.setAllProjectsCollapsed(
                              displayProjects.map(({ project }) => project.id),
                              anyProjectExpanded,
                            );
                          }}
                        >
                          <span className="flex min-w-0 items-center gap-2 whitespace-nowrap">
                            {anyProjectExpanded ? (
                              <ChevronsDownUp className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                            ) : (
                              <ChevronsUpDown className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                            )}
                            <span>
                              {anyProjectExpanded
                                ? t("sidebar.collapseAll")
                                : t("sidebar.expandAll")}
                            </span>
                          </span>
                        </DropdownMenuItem>
                      )}

                      <DropdownMenuItem onSelect={() => orbitActions.openSettings("archived")}>
                        <span className="flex min-w-0 items-center gap-2 whitespace-nowrap">
                          <Archive className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                          <span>{t("sidebar.archived")}</span>
                        </span>
                        {archivedCount > 0 && (
                          <span className="text-ink-faint shrink-0 text-[11px] tabular-nums">
                            {archivedCount}
                          </span>
                        )}
                      </DropdownMenuItem>
                    </DropdownMenuGroup>
                  </DropdownMenuContent>
                </DropdownMenu>

                <AddProjectButton hosts={fleet.hosts} />
              </div>
            </div>

            {hasFilterActive && (
              <div className="flex h-[24px] items-center justify-between gap-2 px-2 text-[11.5px]">
                <span className="text-signal-info truncate font-medium">
                  {t("sidebar.showingActiveUnread")}
                </span>
                <button
                  type="button"
                  onClick={() => orbitActions.setSidebarViewPrefs({ threadFilter: "all" })}
                  className="text-ink-muted hover:text-ink-primary shrink-0 cursor-pointer transition-colors"
                >
                  {t("common.reset")}
                </button>
              </div>
            )}

            {!fleetHydrated && fleet.hosts.length === 0 ? null : fleet.hosts.length === 0 ? (
              <div className="border-border-default text-ink-muted mx-1 mt-1 space-y-2.5 rounded-xl border border-dashed p-4 text-center text-[13px]">
                <p>{t("sidebar.noHostConnected")}</p>
                <Button
                  type="button"
                  size="sm"
                  onClick={() => orbitActions.openSettings("hosts")}
                >
                  <Plus className="size-[14px]" />
                  <span>{t("sidebar.connectHost")}</span>
                </Button>
              </div>
            ) : projectRenderModels.length === 0 ? (
              <div className="text-ink-muted px-2 py-2 text-[12.5px]">
                {hasFilterActive ? t("sidebar.noActiveOrUnread") : t("sidebar.noProjectsYet")}
              </div>
            ) : (
              <div className="space-y-0.5">
                {projectRenderModels.map((model) => {
                  const organizeEvent =
                    hostDetails[model.project.host_id]?.workspace_organize_status?.[
                      model.normProjRoot
                    ] ??
                    hostDetails[model.project.host_id]?.workspace_organize_status?.[
                      model.project.primary_cwd
                    ] ??
                    null;

                  const projWsIdSet =
                    model.curatedWorkspaces.length > 0
                      ? new Set(model.curatedWorkspaces.map((w) => w.workspace_id))
                      : null;

                  const openConvMenuThreadKeyForProj =
                    convMenu && convMenu.host.host_id === model.project.host_id
                      ? convMenu.threadKey
                      : null;

                  const openWsMenuIdForProj =
                    wsMenu && projWsIdSet?.has(wsMenu.workspace.workspace_id)
                      ? wsMenu.workspace.workspace_id
                      : null;

                  const renamingWsIdForProj =
                    renamingWorkspaceId && projWsIdSet?.has(renamingWorkspaceId)
                      ? renamingWorkspaceId
                      : null;

                  const draggingThreadForProj =
                    draggingThread && draggingThread.projectId === model.project.id
                      ? draggingThread
                      : null;

                  const threadDropForProj =
                    threadDropIndicator &&
                    (threadDropIndicator.scopeKey === model.projThreadScopeKey ||
                      model.curatedWorkspaces.some(
                        (w) => w.threadScopeKey === threadDropIndicator.scopeKey,
                      ))
                      ? threadDropIndicator
                      : null;

                  const dragOverForProj =
                    dragOverDropId &&
                    (dragOverDropId === `inbox:${model.project.id}` ||
                      (dragOverDropId.startsWith("ws:") &&
                        Boolean(projWsIdSet?.has(dragOverDropId.slice(3)))))
                      ? dragOverDropId
                      : null;

                  return (
                    <SidebarProjectSection
                      key={model.project.id}
                      model={model}
                      totalProjectCount={displayProjects.length}
                      singleHostMode={singleHostMode}
                      hasFilterActive={hasFilterActive}
                      isFilteringThreads={isFilteringThreads}
                      organizeEvent={organizeEvent}
                      workspacePanel={workspacePanel}
                      isDraftNewSession={isDraftNewSession}
                      selectedHostId={selectedHostId}
                      selectedAgentId={selectedAgentId}
                      threadCustomTitles={threadCustomTitles}
                      runningParentThreadKeys={runningParentThreadKeys}
                      expandedMore={Boolean(expandedMoreProjects[model.project.id])}
                      onSetExpandedMore={handleSetExpandedMore}
                      renamingThreadKey={renamingThreadKey}
                      isRenamingProject={renamingProjectId === model.project.id}
                      onFinishRenameProject={handleFinishRenameProject}
                      renamingWorkspaceId={renamingWsIdForProj}
                      onFinishRenameWorkspace={handleFinishRenameWorkspace}
                      creatingGroupTarget={
                        creatingGroupTarget?.projectId === model.project.id
                          ? creatingGroupTarget
                          : null
                      }
                      onFinishCreateGroup={handleFinishCreateGroup}
                      openConvMenuThreadKey={openConvMenuThreadKeyForProj}
                      projMenuOpen={projMenu?.project.id === model.project.id}
                      openWsMenuId={openWsMenuIdForProj}
                      onOpenConvMenu={handleOpenConvMenu}
                      onOpenProjMenu={setProjMenu}
                      onOpenWsMenu={setWsMenu}
                      onStartRenameThread={handleStartRenameThread}
                      onFinishRenameThread={handleFinishRenameThread}
                      isDraggingProject={draggingProjectId === model.project.id}
                      projectDropPosition={
                        projectDropIndicator?.targetId === model.project.id
                          ? projectDropIndicator.position
                          : null
                      }
                      draggingThread={draggingThreadForProj}
                      threadDropIndicator={threadDropForProj}
                      dragOverDropId={dragOverForProj}
                      onRegisterProjectRow={registerProjectRow}
                      onRegisterNavRow={registerNavRow}
                      onRegisterThreadRow={registerThreadRow}
                      onRegisterWorkspaceHeader={registerWorkspaceHeader}
                      onRegisterInboxArea={registerInboxArea}
                      onStartProjectDrag={startProjectDrag}
                      onStartThreadDrag={startThreadDrag}
                      shouldSuppressProjectClick={shouldSuppressProjectClick}
                      shouldSuppressThreadClick={shouldSuppressThreadClick}
                      keyboardFocusedNavId={keyboardFocusedNavId}
                    />
                  );
                })}
              </div>
            )}
          </div>
          )}
        </div>
      </ScrollArea>

      {/* 4. Ultra-Clean Footer: Settings (`⌘,`) + Conditional Multi-Host Health Pill */}
      <div className="border-border-subtle/75 flex shrink-0 items-center justify-between gap-1 border-t px-2 py-2">
        <button
          type="button"
          onClick={() => orbitActions.openSettings("general")}
          className="group/settings text-ink-secondary hover:text-ink-primary hover:bg-accent/55 flex h-[30px] min-w-0 flex-1 cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 text-[13px] transition-colors"
        >
          <span className="flex min-w-0 items-center gap-2.5">
            <span className="relative inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
              {isFleetUpdating ? (
                <OrbitSpinner size={14} className="text-signal-info" />
              ) : (
                <Settings className="text-ink-muted group-hover/settings:text-ink-primary size-[14.5px] stroke-[1.75] transition-colors" />
              )}
              {!isFleetUpdating && hasFleetUpdatesAvailable && (
                <span
                  aria-label={t("sidebar.updateAvailable")}
                  className="bg-signal-info absolute -top-0.5 -right-0.5 size-[6px] rounded-full"
                />
              )}
            </span>
            <span className="truncate">{t("sidebar.settings")}</span>
          </span>

          {!isFleetUpdating && hasFleetUpdatesAvailable ? (
            <span className="bg-signal-info/12 text-signal-info shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-medium">
              {t("sidebar.updateBadge")}
            </span>
          ) : (
            <ShortcutKbd
              keys={`${mod},`}
              size="xs"
              variant="subtle"
              className="shrink-0 opacity-0 transition-opacity group-hover/settings:opacity-100"
            />
          )}
        </button>

        {fleetHealth.showFooterPill && (
          <SimpleTooltip
            label={
              fleetHealth.total > 1
                ? t("sidebar.hostsOnline", {
                    online: fleetHealth.online,
                    total: fleetHealth.total,
                  })
                : fleetHealth.connecting > 0
                  ? t("sidebar.connectingToHost")
                  : t("sidebar.hostOffline")
            }
          >
            <button
              type="button"
              aria-label={t("sidebar.manageHosts")}
              onClick={() => orbitActions.openSettings("hosts")}
              className="text-ink-muted hover:text-ink-primary hover:bg-accent/55 inline-flex h-[28px] shrink-0 cursor-pointer items-center gap-1.5 rounded-[7px] px-2 text-[11.5px] tabular-nums transition-colors"
            >
              {fleetHealth.connecting > 0 ? (
                <OrbitSpinner size={11} className="text-signal-warning" />
              ) : (
                <span
                  className={cn(
                    "size-[6px] rounded-full",
                    fleetHealth.online === fleetHealth.total
                      ? "bg-signal-success"
                      : fleetHealth.online > 0
                        ? "bg-signal-warning"
                        : "bg-ink-faint",
                  )}
                />
              )}
              <span>
                {fleetHealth.total > 1
                  ? `${fleetHealth.online}/${fleetHealth.total}`
                  : fleetHealth.connecting > 0
                    ? t("common.sync")
                    : t("common.offline")}
              </span>
            </button>
          </SimpleTooltip>
        )}
      </div>

      {/* Shared Single-Instance Context/Options Menus */}
      <SharedConversationMenu
        target={convMenu}
        onClose={handleCloseConvMenu}
        onStartRename={handleStartRenameThread}
        onRequestCreateWorkspace={startInlineCreateGroup}
        onRequestDelete={setDeleteConversationTarget}
      />
      <SharedProjectMenu
        target={projMenu}
        isMac={isMac}
        onClose={handleCloseProjMenu}
        onStartRenameProject={handleStartRenameProject}
        onRequestDelete={setDeleteProjectTarget}
        onRequestCreateWorkspace={startInlineCreateGroup}
      />
      <SharedWorkspaceMenu
        target={wsMenu}
        onClose={handleCloseWsMenu}
        onStartRenameWorkspace={handleStartRenameWorkspace}
      />
      <DeleteConversationDialog
        target={deleteConversationTarget}
        onClose={() => setDeleteConversationTarget(null)}
      />
      <DeleteProjectDialog
        project={deleteProjectTarget}
        archivedThreads={archivedThreads}
        onClose={() => setDeleteProjectTarget(null)}
      />

      {/* Pointer-driven GPU-translated drag preview (0 React re-renders per frame) */}
      {draggingThread && (
        <div
          ref={floatingPreviewRef}
          className="floating-surface text-ink-primary pointer-events-none fixed top-0 left-0 z-50 max-w-[210px] truncate rounded-[8px] border px-2.5 py-1 text-[12.5px] font-medium will-change-transform"
        >
          {draggingThread.title}
        </div>
      )}
    </aside>
  );
});
