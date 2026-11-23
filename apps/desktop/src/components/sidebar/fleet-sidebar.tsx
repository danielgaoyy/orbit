import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  Archive,
  ArrowDownAZ,
  ArrowUpDown,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  CircleDot,
  Clock,
  Folder,
  FolderOpen,
  Layers,
  List,
  ListFilter,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Settings,
  X,
} from "lucide-react";
import { fuzzyMatch, splitHighlightSegments } from "@/lib/fuzzy-match";
import { modSymbol } from "@/lib/keybindings";
import {
  type AgentCardView,
  type HostCardView,
  type SidebarConversationView,
  type SidebarProjectGroupView,
  compareSemver,
  isAgentBackgroundWaiting,
  isAgentForegroundRunning,
  isMacDesktop,
  normalizeSemver,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  basenameOfProjectPath,
  isLocalHostCard,
  matchesProjectDirectory,
  normalizeProjectPath,
  projectPathScopeKey,
} from "@/lib/projects";
import { useImeCompositionGuard } from "@/lib/ime";
import { useI18n } from "@/lib/i18n";
import { SidebarConversationRow } from "@/components/sidebar/sidebar-conversation-row";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import {
  type ActiveConversationMenuState,
  type ActiveProjectMenuState,
  type ActiveWorkspaceMenuState,
  CreateWorkspaceDialog,
  type CreateWorkspaceDialogTarget,
  DeleteConversationDialog,
  type DeleteConversationTarget,
  DeleteProjectDialog,
  type MenuAnchorBox,
  SharedConversationMenu,
  SharedProjectMenu,
  SharedWorkspaceMenu,
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

const MAX_VISIBLE_CONVERSATIONS_PER_PROJECT = 6;

function applyManualThreadOrder(
  list: SidebarConversationView[],
  manualIds: string[] | undefined,
): SidebarConversationView[] {
  if (!manualIds || manualIds.length === 0) return list;
  const rank = new Map<string, number>();
  manualIds.forEach((id, idx) => rank.set(id, idx));
  const unordered: SidebarConversationView[] = [];
  const ordered: SidebarConversationView[] = [];
  for (const item of list) {
    if (rank.has(item.agent.agent_id)) {
      ordered.push(item);
    } else {
      unordered.push(item);
    }
  }
  ordered.sort(
    (a, b) => (rank.get(a.agent.agent_id) ?? 0) - (rank.get(b.agent.agent_id) ?? 0),
  );
  return [...unordered, ...ordered];
}

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
  const collapsedWorkspaceIds = useOrbitSelector((s) => s.collapsedWorkspaceIds);
  const threadCustomTitles = useOrbitSelector((s) => s.threadCustomTitles);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const threadManualOrder = useOrbitSelector((s) => s.threadManualOrder);
  const sidebarProjectSort = useOrbitSelector((s) => s.sidebarProjectSort);
  const sidebarThreadFilter = useOrbitSelector((s) => s.sidebarThreadFilter);
  const hiddenProjectKeys = useOrbitSelector((s) => s.hiddenProjectKeys);
  const fleetHydrated = useOrbitSelector((s) => s.fleetHydrated);
  const appVersion = useOrbitSelector((s) => s.appVersion);
  const releaseUpdateInfo = useOrbitSelector((s) => s.releaseUpdateInfo);
  const fleetUpdate = useOrbitSelector((s) => s.fleetUpdate);
  const stageRunningSubagentSignature = useOrbitSelector((s) => {
    let sig = "";
    for (const [tlKey, st] of Object.entries(s.stages)) {
      const subs = st.insights.subagents;
      if (subs.length === 0) continue;
      const sepIdx = tlKey.indexOf("::");
      const hostId = sepIdx >= 0 ? tlKey.slice(0, sepIdx) : "";
      const agentId = sepIdx >= 0 ? tlKey.slice(sepIdx + 2) : "";
      const provList = hostId && agentId ? s.hostDetails[hostId]?.provider_subagents?.[agentId] : undefined;
      let hasRunning = false;
      for (let i = 0; i < subs.length; i += 1) {
        const sa = subs[i];
        if (sa.status !== "running") continue;
        if (sa.child_agent_id) {
          let managedChild: AgentCardView | undefined;
          for (const d of Object.values(s.hostDetails)) {
            const match = d.agents.find((a) => a.agent_id === sa.child_agent_id);
            if (match) {
              managedChild = match;
              break;
            }
          }
          if (managedChild) {
            if (
              !managedChild.archived_at_rfc3339 &&
              (managedChild.is_running ||
                isAgentBackgroundWaiting(managedChild) ||
                managedChild.is_awaiting_permission)
            ) {
              hasRunning = true;
              break;
            }
            continue;
          }
        }
        if (sa.provider_subagent_id && provList) {
          const provMatch = provList.find((ps) => ps.subagent_id === sa.provider_subagent_id);
          if (provMatch) {
            if (provMatch.status === "running") {
              hasRunning = true;
              break;
            }
            continue;
          }
        }
        hasRunning = true;
        break;
      }
      if (hasRunning) {
        sig += sig ? `\0${tlKey}` : tlKey;
      }
    }
    return sig;
  });

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

  const [expandedMoreProjects, setExpandedMoreProjects] = useState<Record<string, boolean>>({});
  const [filterQuery, setFilterQuery] = useState("");
  const [isFilterFocused, setIsFilterFocused] = useState(false);
  const [keyboardFocusedNavId, setKeyboardFocusedNavId] = useState<string | null>(null);
  const filterInputRef = useRef<HTMLInputElement | null>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const navRowRefs = useRef<Map<string, HTMLElement>>(new Map());
  const [renamingThreadKey, setRenamingThreadKey] = useState<string | null>(null);
  const [renamingProjectId, setRenamingProjectId] = useState<string | null>(null);
  const [projectRenameDraft, setProjectRenameDraft] = useState("");
  const projectRenameCancelledRef = useRef(false);
  const [renamingWorkspaceId, setRenamingWorkspaceId] = useState<string | null>(null);
  const [workspaceRenameDraft, setWorkspaceRenameDraft] = useState("");
  const [convMenu, setConvMenu] = useState<ActiveConversationMenuState | null>(null);
  const [projMenu, setProjMenu] = useState<ActiveProjectMenuState | null>(null);
  const [wsMenu, setWsMenu] = useState<ActiveWorkspaceMenuState | null>(null);
  const [createWsTarget, setCreateWsTarget] = useState<CreateWorkspaceDialogTarget | null>(null);
  const [dragOverDropId, setDragOverDropId] = useState<string | null>(null);
  const [draggingThread, setDraggingThread] = useState<{
    hostId: string;
    projectId: string;
    agentId: string;
    title: string;
    scopeKey: string;
    workspaceId: string | null;
    clientX: number;
    clientY: number;
  } | null>(null);
  const [threadDropIndicator, setThreadDropIndicator] = useState<{
    scopeKey: string;
    targetAgentId: string;
    position: "before" | "after";
  } | null>(null);
  const [deleteConversationTarget, setDeleteConversationTarget] =
    useState<DeleteConversationTarget | null>(null);
  const [deleteProjectTarget, setDeleteProjectTarget] =
    useState<SidebarProjectGroupView | null>(null);
  const ime = useImeCompositionGuard();
  const [draggingProjectId, setDraggingProjectId] = useState<string | null>(null);
  const [dropIndicator, setDropIndicator] = useState<{
    targetId: string;
    position: "before" | "after";
  } | null>(null);
  const projectRowRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const threadRowRefs = useRef<
    Map<
      string,
      {
        el: HTMLDivElement;
        hostId: string;
        projectId: string;
        agentId: string;
        scopeKey: string;
        workspaceId: string | null;
      }
    >
  >(new Map());
  const workspaceHeaderRefs = useRef<
    Map<string, { el: HTMLDivElement; hostId: string; projectId: string; workspaceId: string }>
  >(new Map());
  const inboxAreaRefs = useRef<
    Map<string, { el: HTMLDivElement; hostId: string; projectId: string; scopeKey: string }>
  >(new Map());
  const scopeAgentIdsRef = useRef<Map<string, string[]>>(new Map());
  const suppressNextProjectClickRef = useRef(false);
  const suppressNextThreadClickRef = useRef(false);
  const dragCleanupRef = useRef<(() => void) | null>(null);
  const threadDragCleanupRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    return () => {
      dragCleanupRef.current?.();
      threadDragCleanupRef.current?.();
    };
  }, []);

  const shouldSuppressThreadClick = useCallback(() => suppressNextThreadClickRef.current, []);

  const startThreadDrag = useCallback(
    (
      e: React.PointerEvent<HTMLDivElement>,
      source: {
        hostId: string;
        projectId: string;
        agentId: string;
        title: string;
        scopeKey: string;
        workspaceId: string | null;
      },
    ) => {
      if (e.button !== 0) return;
      const targetEl = e.target as HTMLElement | null;
      if (targetEl?.closest("[data-thread-action]") || targetEl?.tagName === "INPUT") return;

      const startX = e.clientX;
      const startY = e.clientY;
      let activeDrag = false;

      type DropTarget =
        | {
            kind: "thread";
            scopeKey: string;
            workspaceId: string | null;
            targetAgentId: string;
            position: "before" | "after";
          }
        | { kind: "group"; workspaceId: string }
        | { kind: "inbox"; projectId: string; scopeKey: string }
        | null;

      let currentTarget: DropTarget = null;

      const onMove = (ev: PointerEvent) => {
        if (!activeDrag) {
          if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < 4) return;
          activeDrag = true;
          suppressNextThreadClickRef.current = true;
          document.body.style.cursor = "grabbing";
        }

        setDraggingThread({
          ...source,
          clientX: ev.clientX,
          clientY: ev.clientY,
        });

        let best: DropTarget = null;

        // 1. Check Curated Group headers in the same project (drop into group)
        for (const wsEntry of workspaceHeaderRefs.current.values()) {
          if (
            wsEntry.hostId !== source.hostId ||
            wsEntry.projectId !== source.projectId ||
            wsEntry.workspaceId === source.workspaceId
          ) {
            continue;
          }
          const rect = wsEntry.el.getBoundingClientRect();
          if (
            ev.clientX >= rect.left - 8 &&
            ev.clientX <= rect.right + 16 &&
            ev.clientY >= rect.top - 2 &&
            ev.clientY <= rect.bottom + 2
          ) {
            best = { kind: "group", workspaceId: wsEntry.workspaceId };
            break;
          }
        }

        // 2. Check Thread rows in the same project (reorder or cross-group insert at exact slot)
        if (!best) {
          const sameScopeEntries: {
            entry: {
              el: HTMLDivElement;
              hostId: string;
              projectId: string;
              agentId: string;
              scopeKey: string;
              workspaceId: string | null;
            };
            rect: DOMRect;
          }[] = [];

          for (const entry of threadRowRefs.current.values()) {
            if (entry.hostId !== source.hostId || entry.projectId !== source.projectId) {
              continue;
            }
            const rect = entry.el.getBoundingClientRect();
            if (rect.height === 0) continue;
            if (entry.scopeKey === source.scopeKey) {
              sameScopeEntries.push({ entry, rect });
            }
            if (
              ev.clientX >= rect.left - 12 &&
              ev.clientX <= rect.right + 24 &&
              ev.clientY >= rect.top - 4 &&
              ev.clientY <= rect.bottom + 4
            ) {
              if (entry.agentId !== source.agentId) {
                best = {
                  kind: "thread",
                  scopeKey: entry.scopeKey,
                  workspaceId: entry.workspaceId,
                  targetAgentId: entry.agentId,
                  position: ev.clientY < rect.top + rect.height / 2 ? "before" : "after",
                };
              }
              break;
            }
          }

          // Edge snap: if dragging slightly above first or below last thread in the same scope
          if (!best && sameScopeEntries.length > 1) {
            sameScopeEntries.sort((a, b) => a.rect.top - b.rect.top);
            const first = sameScopeEntries[0];
            const last = sameScopeEntries[sameScopeEntries.length - 1];
            const inHorizontalBounds =
              ev.clientX >= first.rect.left - 12 && ev.clientX <= first.rect.right + 24;
            if (
              inHorizontalBounds &&
              ev.clientY < first.rect.top &&
              ev.clientY >= first.rect.top - 28 &&
              first.entry.agentId !== source.agentId
            ) {
              best = {
                kind: "thread",
                scopeKey: first.entry.scopeKey,
                workspaceId: first.entry.workspaceId,
                targetAgentId: first.entry.agentId,
                position: "before",
              };
            } else if (
              inHorizontalBounds &&
              ev.clientY > last.rect.bottom &&
              ev.clientY <= last.rect.bottom + 28 &&
              last.entry.agentId !== source.agentId
            ) {
              best = {
                kind: "thread",
                scopeKey: last.entry.scopeKey,
                workspaceId: last.entry.workspaceId,
                targetAgentId: last.entry.agentId,
                position: "after",
              };
            }
          }
        }

        // 3. Check Unclassified inbox area (ungroup when dragging out of a curated group)
        if (!best && source.workspaceId !== null) {
          for (const inboxEntry of inboxAreaRefs.current.values()) {
            if (inboxEntry.hostId !== source.hostId || inboxEntry.projectId !== source.projectId) {
              continue;
            }
            const rect = inboxEntry.el.getBoundingClientRect();
            if (
              ev.clientY >= rect.top - 4 &&
              ev.clientY <= rect.bottom + 12 &&
              ev.clientX <= rect.right + 24
            ) {
              best = {
                kind: "inbox",
                projectId: inboxEntry.projectId,
                scopeKey: inboxEntry.scopeKey,
              };
              break;
            }
          }
        }

        currentTarget = best;

        if (best?.kind === "thread") {
          setDragOverDropId(null);
          setThreadDropIndicator({
            scopeKey: best.scopeKey,
            targetAgentId: best.targetAgentId,
            position: best.position,
          });
        } else if (best?.kind === "group") {
          setThreadDropIndicator(null);
          setDragOverDropId(`ws:${best.workspaceId}`);
        } else if (best?.kind === "inbox") {
          setThreadDropIndicator(null);
          setDragOverDropId(`inbox:${best.projectId}`);
        } else {
          setThreadDropIndicator(null);
          setDragOverDropId(null);
        }
      };

      const finish = (commit: boolean) => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onPointerUp);
        window.removeEventListener("pointercancel", onCancel);
        window.removeEventListener("keydown", onKeyDown);
        threadDragCleanupRef.current = null;
        document.body.style.cursor = "";

        if (activeDrag) {
          setDraggingThread(null);
          setThreadDropIndicator(null);
          setDragOverDropId(null);

          if (commit) {
            if (currentTarget?.kind === "thread") {
              const fullIds = scopeAgentIdsRef.current.get(currentTarget.scopeKey) ?? [];
              const orderedIds = fullIds.filter((id) => id !== source.agentId);
              let toIdx = orderedIds.indexOf(currentTarget.targetAgentId);
              if (toIdx >= 0) {
                if (currentTarget.position === "after") toIdx += 1;
                orderedIds.splice(toIdx, 0, source.agentId);
              } else {
                orderedIds.unshift(source.agentId);
              }

              if (source.workspaceId !== currentTarget.workspaceId) {
                if (currentTarget.workspaceId) {
                  void orbitActions.moveAgentToWorkspace(
                    source.hostId,
                    source.agentId,
                    currentTarget.workspaceId,
                  );
                } else {
                  void orbitActions.moveAgentToWorkspace(source.hostId, source.agentId, null, {
                    makeUnclassified: true,
                  });
                }
              }

              orbitActions.reorderThreads(currentTarget.scopeKey, orderedIds);
            } else if (currentTarget?.kind === "group") {
              void orbitActions.moveAgentToWorkspace(
                source.hostId,
                source.agentId,
                currentTarget.workspaceId,
              );
            } else if (currentTarget?.kind === "inbox") {
              void orbitActions.moveAgentToWorkspace(source.hostId, source.agentId, null, {
                makeUnclassified: true,
              });
            }
          }

          window.setTimeout(() => {
            suppressNextThreadClickRef.current = false;
          }, 80);
        }
      };

      const onPointerUp = () => finish(true);
      const onCancel = () => finish(false);
      const onKeyDown = (ev: KeyboardEvent) => {
        if (ev.key === "Escape") finish(false);
      };

      threadDragCleanupRef.current?.();
      threadDragCleanupRef.current = onCancel;
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onPointerUp);
      window.addEventListener("pointercancel", onCancel);
      window.addEventListener("keydown", onKeyDown);
    },
    [],
  );

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

  const isAgentArchived = useCallback(
    (hostId: string, ag: AgentCardView): boolean => {
      if (ag.archived_at_rfc3339) return true;
      return Boolean(archivedThreads[threadScopeKey(hostId, ag.agent_id)]);
    },
    [archivedThreads],
  );

  // O(N) lookup of parent threads that currently have any active managed, provider-native, or cached-stage subagent
  const runningParentThreadKeys = useMemo(() => {
    const keys = new Set<string>();
    if (stageRunningSubagentSignature) {
      for (const tKey of stageRunningSubagentSignature.split("\0")) {
        if (tKey) keys.add(tKey);
      }
    }
    for (const [hid, detail] of Object.entries(hostDetails)) {
      for (const ag of detail.agents) {
        if (!ag.parent_agent_id || isAgentArchived(hid, ag)) continue;
        if (ag.is_running || isAgentBackgroundWaiting(ag) || ag.is_awaiting_permission) {
          const parentHostId = ag.parent_host_id || hid;
          keys.add(threadScopeKey(parentHostId, ag.parent_agent_id));
        }
      }
      if (detail.provider_subagents) {
        for (const [parentAgentId, descriptors] of Object.entries(detail.provider_subagents)) {
          if (descriptors.some((ps) => ps.status === "running")) {
            keys.add(threadScopeKey(hid, parentAgentId));
          }
        }
      }
    }
    return keys;
  }, [hostDetails, isAgentArchived, stageRunningSubagentSignature]);

  const hasAgentRunningSubagents = useCallback(
    (hostId: string, ag: AgentCardView): boolean =>
      Boolean((ag as AgentCardView & { has_running_subagents?: boolean }).has_running_subagents) ||
      runningParentThreadKeys.has(threadScopeKey(hostId, ag.agent_id)),
    [runningParentThreadKeys],
  );

  const isConvUnread = useCallback(
    (conv: SidebarConversationView): boolean => {
      const isSelected =
        workspacePanel === "conversation" &&
        !isDraftNewSession &&
        selectedHostId === conv.host_id &&
        selectedAgentId === conv.agent.agent_id;
      if (isSelected || isAgentForegroundRunning(conv.agent)) return false;
      if (conv.agent.attention && conv.agent.attention !== "none") return true;
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
      if (isAgentArchived(conv.host_id, conv.agent)) continue;
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
  }, [sidebarView?.needs_attention, isAgentArchived, hostById, rawProjects, threadCustomTitles]);

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
      if (isAgentArchived(conv.host_id, conv.agent)) continue;
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
  }, [sidebarView?.pinned_conversations, isAgentArchived, hostById, rawProjects]);

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

      const hasCuratedWorkspaces = (proj.workspaces ?? []).some((w) => w.is_curated !== false);
      const detail = hostDetails[proj.host_id];
      const agentsById = new Map((detail?.agents ?? []).map((a) => [a.agent_id, a]));

      const visibleConversations = proj.conversations.filter((c) => {
        if (c.agent.background) return false;
        if (isAgentArchived(proj.host_id, c.agent)) return false;
        // Collapse same-host managed subagents under their parent's Subagents Track
        if (c.agent.parent_agent_id && !c.agent.parent_host_id) {
          const parent = agentsById.get(c.agent.parent_agent_id);
          if (parent && !isAgentArchived(proj.host_id, parent)) {
            return false;
          }
        }
        return true;
      });

      if (
        proj.conversations.length === 0 &&
        !hasCuratedWorkspaces &&
        !proj.pinned_project_id &&
        hiddenProjectKeys[projectPathScopeKey(proj.host_id, proj.primary_cwd)] &&
        !isActiveTarget
      ) {
        continue;
      }

      const allArchived =
        proj.conversations.length > 0 && visibleConversations.length === 0;
      if (!proj.pinned_project_id && !hasCuratedWorkspaces && allArchived && !isActiveTarget) {
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
    hostDetails,
    hiddenProjectKeys,
    isAgentArchived,
    selectedHostId,
    selectedWorkspaceCwd,
  ]);

  // 4. Archived conversation count (full management lives in Settings -> Archived)
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

  const isAutomationsActive = workspacePanel === "automations";
  const hasFilterActive = sidebarThreadFilter === "active_unread";
  const { t } = useI18n();

  const trimmedFilter = filterQuery.trim();
  const isFilteringThreads = trimmedFilter.length > 0;

  const matchSidebarThread = useCallback(
    (
      effectiveTitle: string,
      projectName: string,
      host: HostCardView,
      agent: AgentCardView,
    ): { matched: boolean; titleIndices?: number[] } => {
      if (!isFilteringThreads) return { matched: true };
      const titleHit = fuzzyMatch(trimmedFilter, effectiveTitle);
      if (titleHit) {
        return { matched: true, titleIndices: titleHit.indices };
      }
      if (fuzzyMatch(trimmedFilter, projectName)) {
        return { matched: true };
      }
      if (agent.handle && fuzzyMatch(trimmedFilter, agent.handle)) {
        return { matched: true };
      }
      if (!singleHostMode && fuzzyMatch(trimmedFilter, host.display_name)) {
        return { matched: true };
      }
      return { matched: false };
    },
    [isFilteringThreads, trimmedFilter, singleHostMode],
  );

  const filteredAttentionItems = useMemo(() => {
    const out: Array<
      (typeof attentionItems)[number] & { navId: string; titleIndices?: number[] }
    > = [];
    for (const item of attentionItems) {
      const m = matchSidebarThread(item.effectiveTitle, item.projectName, item.host, item.agent);
      if (!m.matched) continue;
      out.push({
        ...item,
        navId: `attn:${item.host.host_id}:${item.agent.agent_id}`,
        titleIndices: m.titleIndices,
      });
    }
    return out;
  }, [attentionItems, matchSidebarThread]);

  const filteredPinnedEntries = useMemo(() => {
    const out: Array<
      (typeof pinnedEntries)[number] & {
        navId: string;
        effectiveTitle: string;
        titleIndices?: number[];
      }
    > = [];
    for (const entry of pinnedEntries) {
      const effectiveTitle = threadCustomTitles[entry.key] || entry.conv.agent.title;
      const m = matchSidebarThread(effectiveTitle, entry.projectName, entry.host, entry.conv.agent);
      if (!m.matched) continue;
      out.push({
        ...entry,
        navId: `pin:${entry.key}`,
        effectiveTitle,
        titleIndices: m.titleIndices,
      });
    }
    return out;
  }, [pinnedEntries, threadCustomTitles, matchSidebarThread]);

  const projectRenderModels = useMemo(() => {
    const useManualThreadOrder = sidebarProjectSort === "manual";
    const models: Array<{
      project: SidebarProjectGroupView;
      host: HostCardView;
      visibleConversations: SidebarConversationView[];
      collapsed: boolean;
      projThreadScopeKey: string;
      normProjRoot: string;
      curatedWorkspaces: Array<
        NonNullable<SidebarProjectGroupView["workspaces"]>[number] & {
          threadScopeKey: string;
          visiblePeers: SidebarConversationView[];
          wsCollapsed: boolean;
        }
      >;
      unclassifiedConversations: SidebarConversationView[];
      slicedUnclassified: SidebarConversationView[];
      selectedUnclassifiedIdx: number;
      isExpandedAll: boolean;
      totalUnclassified: number;
      hiddenCount: number;
      titleIndicesByThreadKey: Map<string, number[]>;
    }> = [];

    for (const { project: proj, host, visibleConversations: baseVisible } of displayProjects) {
      const titleIndicesByThreadKey = new Map<string, number[]>();
      const visibleConversations = isFilteringThreads
        ? baseVisible.filter((c) => {
            const tKey = threadScopeKey(proj.host_id, c.agent.agent_id);
            const effectiveTitle = threadCustomTitles[tKey] || c.agent.title;
            const m = matchSidebarThread(effectiveTitle, proj.name, host, c.agent);
            if (m.matched && m.titleIndices) {
              titleIndicesByThreadKey.set(tKey, m.titleIndices);
            }
            return m.matched;
          })
        : baseVisible;

      if (isFilteringThreads && visibleConversations.length === 0) {
        continue;
      }

      const collapsed = isFilteringThreads ? false : Boolean(collapsedProjectIds[proj.id]);
      const visibleIdSet = new Set(visibleConversations.map((c) => c.agent.agent_id));
      const normProjRoot = normalizeProjectPath(proj.primary_cwd);
      const projThreadScopeKey = `proj:${proj.host_id}::${normProjRoot}`;

      const curatedWorkspaces = (proj.workspaces ?? [])
        .filter((ws) => ws.is_curated !== false)
        .map((ws) => {
          const wsScopeKey = `ws:${proj.host_id}::${ws.workspace_id}`;
          const filteredPeers = ws.conversations.filter((c) =>
            visibleIdSet.has(c.agent.agent_id),
          );
          const visiblePeers = useManualThreadOrder
            ? applyManualThreadOrder(filteredPeers, threadManualOrder[wsScopeKey])
            : filteredPeers;
          return {
            ...ws,
            threadScopeKey: wsScopeKey,
            visiblePeers,
            wsCollapsed: isFilteringThreads
              ? false
              : Boolean(collapsedWorkspaceIds[ws.workspace_id]),
          };
        })
        .filter((ws) => !isFilteringThreads || ws.visiblePeers.length > 0);

      const curatedAgentIdSet = new Set<string>();
      for (const ws of curatedWorkspaces) {
        for (const c of ws.visiblePeers) {
          curatedAgentIdSet.add(c.agent.agent_id);
        }
      }

      const rawUnclassified =
        proj.unclassified_conversations !== undefined
          ? proj.unclassified_conversations.filter((c) => visibleIdSet.has(c.agent.agent_id))
          : visibleConversations.filter((c) => !curatedAgentIdSet.has(c.agent.agent_id));
      const unclassifiedConversations = useManualThreadOrder
        ? applyManualThreadOrder(rawUnclassified, threadManualOrder[projThreadScopeKey])
        : rawUnclassified;

      const selectedUnclassifiedIdx = unclassifiedConversations.findIndex(
        (c) =>
          workspacePanel === "conversation" &&
          !isDraftNewSession &&
          selectedHostId === proj.host_id &&
          selectedAgentId === c.agent.agent_id,
      );
      const isExpandedAll =
        isFilteringThreads ||
        Boolean(expandedMoreProjects[proj.id]) ||
        selectedUnclassifiedIdx >= MAX_VISIBLE_CONVERSATIONS_PER_PROJECT;
      const totalUnclassified = unclassifiedConversations.length;
      const hiddenCount = Math.max(
        0,
        totalUnclassified - MAX_VISIBLE_CONVERSATIONS_PER_PROJECT,
      );
      const slicedUnclassified = isExpandedAll
        ? unclassifiedConversations
        : unclassifiedConversations.slice(0, MAX_VISIBLE_CONVERSATIONS_PER_PROJECT);

      models.push({
        project: proj,
        host,
        visibleConversations,
        collapsed,
        projThreadScopeKey,
        normProjRoot,
        curatedWorkspaces,
        unclassifiedConversations,
        slicedUnclassified,
        selectedUnclassifiedIdx,
        isExpandedAll,
        totalUnclassified,
        hiddenCount,
        titleIndicesByThreadKey,
      });
    }

    return models;
  }, [
    displayProjects,
    isFilteringThreads,
    threadCustomTitles,
    matchSidebarThread,
    collapsedProjectIds,
    collapsedWorkspaceIds,
    sidebarProjectSort,
    threadManualOrder,
    workspacePanel,
    isDraftNewSession,
    selectedHostId,
    selectedAgentId,
    expandedMoreProjects,
  ]);

  const visibleNavItems = useMemo(() => {
    const items: Array<{ navId: string; hostId: string; agentId: string }> = [];
    for (const item of filteredAttentionItems) {
      items.push({ navId: item.navId, hostId: item.host.host_id, agentId: item.agent.agent_id });
    }
    for (const entry of filteredPinnedEntries) {
      items.push({
        navId: entry.navId,
        hostId: entry.host.host_id,
        agentId: entry.conv.agent.agent_id,
      });
    }
    for (const model of projectRenderModels) {
      if (model.collapsed) continue;
      for (const ws of model.curatedWorkspaces) {
        if (ws.wsCollapsed) continue;
        for (const conv of ws.visiblePeers) {
          items.push({
            navId: `ws:${ws.workspace_id}:${model.project.host_id}:${conv.agent.agent_id}`,
            hostId: model.project.host_id,
            agentId: conv.agent.agent_id,
          });
        }
      }
      for (const conv of model.slicedUnclassified) {
        items.push({
          navId: `proj:${model.project.id}:${model.project.host_id}:${conv.agent.agent_id}`,
          hostId: model.project.host_id,
          agentId: conv.agent.agent_id,
        });
      }
    }
    return items;
  }, [filteredAttentionItems, filteredPinnedEntries, projectRenderModels]);

  // Keep keyboard highlight synced with live filtered results only while filter input is focused
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
    <aside className="bg-sidebar border-border flex h-full w-full flex-col border-r select-none">
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
            <span className="truncate">{t("sidebar.newConversation")}</span>
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
          <span className="truncate">{t("sidebar.automations")}</span>
        </button>
      </div>

      {/* 2B. Sticky Sidebar Quick Filter (keyboard-navigable ↑/↓/Enter/Esc without hijacking Composer) */}
      <div className="px-2 pb-2">
        <div className="group/filter flex h-[28px] items-center gap-1.5 rounded-[8px] border border-transparent bg-black/[0.03] px-2 transition-colors focus-within:border-foreground/15 focus-within:bg-background/85 dark:bg-white/[0.04] dark:focus-within:border-white/[0.14] dark:focus-within:bg-white/[0.06]">
          <Search className="text-muted-foreground/65 group-focus-within/filter:text-foreground/75 h-[13px] w-[13px] shrink-0 stroke-[1.8] transition-colors" />
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
            }}
            onChange={(e) => setFilterQuery(e.target.value)}
            {...ime.compositionProps}
            onKeyDown={handleFilterKeyDown}
            placeholder={t("sidebar.filterThreadsPlaceholder")}
            aria-label={t("sidebar.filterThreadsAria")}
            className="text-foreground placeholder:text-muted-foreground/60 min-w-0 flex-1 bg-transparent text-[12.5px] select-text focus:outline-none"
          />
          {isFilteringThreads && (
            <span className="text-muted-foreground/60 shrink-0 text-[10.5px] tabular-nums">
              {visibleNavItems.length}
            </span>
          )}
          {filterQuery.length > 0 && (
            <button
              type="button"
              aria-label={t("sidebar.clearThreadFilter")}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setFilterQuery("");
                setKeyboardFocusedNavId(null);
                filterInputRef.current?.focus({ preventScroll: true });
              }}
              className="text-muted-foreground/65 hover:text-foreground inline-flex h-[18px] w-[18px] shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
            >
              <X className="h-[11.5px] w-[11.5px]" />
            </button>
          )}
        </div>
      </div>

      {/* 3. Scrollable Mission-Control Body */}
      <ScrollArea className="min-h-0 flex-1" viewportClassName="px-2 pb-2">
        <div className="space-y-3.5">
        {/* 3A. Conditional `Needs Attention` Section — Borderless, Restrained, 0px When Clear */}
        {filteredAttentionItems.length > 0 && (
          <div className="orbit-stage-enter space-y-0.5">
            <div className="flex h-[22px] items-center justify-between px-2">
              <span className="text-[11.5px] font-medium text-amber-600 dark:text-amber-400">
                {t("sidebar.needsAttention")}
              </span>
              <span className="tabular-nums text-[11.5px] font-medium text-amber-600/80 dark:text-amber-400/80">
                {filteredAttentionItems.length}
              </span>
            </div>

            {filteredAttentionItems.map(
              ({ host, agent, projectName, effectiveTitle, navId, titleIndices }) => {
                const active =
                  workspacePanel === "conversation" &&
                  !isDraftNewSession &&
                  selectedHostId === host.host_id &&
                  selectedAgentId === agent.agent_id;
                const isKbFocused = keyboardFocusedNavId === navId;
                return (
                  <button
                    key={`attn-${host.host_id}-${agent.agent_id}`}
                    ref={(el) => {
                      if (el) navRowRefs.current.set(navId, el);
                      else navRowRefs.current.delete(navId);
                    }}
                    type="button"
                    onClick={() => orbitActions.selectAgent(host.host_id, agent.agent_id)}
                    className={cn(
                      "flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 text-left text-[13.5px] transition-colors",
                      active
                        ? "text-foreground bg-black/[0.06] font-medium dark:bg-white/[0.085]"
                        : "text-foreground/86 hover:text-foreground hover:bg-black/[0.04] dark:hover:bg-white/[0.05]",
                      isKbFocused &&
                        "bg-black/[0.055] text-foreground ring-foreground/20 ring-1 ring-inset dark:bg-white/[0.085]",
                    )}
                  >
                    <span className="flex min-w-0 flex-1 items-center gap-2.5">
                      <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
                        <span className="h-[6.5px] w-[6.5px] rounded-full bg-amber-500" />
                      </span>
                      <span className="truncate">
                        {titleIndices && titleIndices.length > 0
                          ? splitHighlightSegments(effectiveTitle, titleIndices).map((seg, i) =>
                              seg.matched ? (
                                <mark
                                  key={i}
                                  className="text-foreground rounded-[2px] bg-foreground/[0.08] font-semibold dark:bg-foreground/[0.15]"
                                >
                                  {seg.text}
                                </mark>
                              ) : (
                                <span key={i}>{seg.text}</span>
                              ),
                            )
                          : effectiveTitle}
                      </span>
                    </span>
                    <span className="text-muted-foreground/65 max-w-20 shrink-0 truncate text-[11.5px]">
                      {projectName}
                    </span>
                  </button>
                );
              },
            )}
          </div>
        )}

        {/* 3B. Conditional `Pinned` Section — 0px When Empty */}
        {filteredPinnedEntries.length > 0 && (
          <div className="orbit-stage-enter space-y-0.5">
            <div className="flex h-[22px] items-center px-2">
              <span className="text-muted-foreground text-[12px] font-medium">
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
                const unread = isConvUnread(conv);

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
                    hasRunningSubagents={hasAgentRunningSubagents(host.host_id, conv.agent)}
                    selected={selected}
                    keyboardFocused={keyboardFocusedNavId === navId}
                    highlightIndices={titleIndices}
                    isChild={false}
                    nowMs={nowMs}
                    menuOpen={convMenu?.threadKey === key}
                    isRenaming={renamingThreadKey === key}
                    rowRef={(el) => {
                      if (el) navRowRefs.current.set(navId, el);
                      else navRowRefs.current.delete(navId);
                    }}
                    onOpenMenu={handleOpenConvMenu}
                    onStartRename={handleStartRename}
                    onFinishRename={handleFinishRename}
                  />
                );
              },
            )}
          </div>
        )}

        {/* 3C. `Projects` Section (`[⇅]` Sort/Filter + `[📁+]` Add Project) */}
        <div className="space-y-0.5">
          <div className="flex h-[24px] items-center justify-between px-2">
            <span className="text-muted-foreground text-[12px] font-medium">
              {t("sidebar.projects")}
            </span>

            <div className="flex items-center gap-0.5">
              <DropdownMenu>
                <SimpleTooltip label={t("sidebar.sortAndFilter")}>
                  <DropdownMenuTrigger asChild>
                    <button
                      type="button"
                      aria-label={t("sidebar.sortAndFilterAria")}
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
                  className="w-[200px] rounded-[12px] p-1"
                >
                  <DropdownMenuLabel className="text-muted-foreground/70 px-2 pt-1 pb-0.5 text-[11px] font-medium">
                    {t("sidebar.sortBy")}
                  </DropdownMenuLabel>
                  <DropdownMenuItem
                    onSelect={() => orbitActions.setSidebarViewPrefs({ projectSort: "recent" })}
                    className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <Clock
                        className={cn(
                          "h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]",
                          sidebarProjectSort === "recent"
                            ? "text-foreground/80"
                            : "text-muted-foreground",
                        )}
                      />
                      <span className="truncate">{t("sidebar.sortRecent")}</span>
                    </span>
                    {sidebarProjectSort === "recent" && (
                      <Check className="text-foreground h-[13px] w-[13px] shrink-0 stroke-[2]" />
                    )}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() =>
                      orbitActions.setSidebarViewPrefs({ projectSort: "alphabetical" })
                    }
                    className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <ArrowDownAZ
                        className={cn(
                          "h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]",
                          sidebarProjectSort === "alphabetical"
                            ? "text-foreground/80"
                            : "text-muted-foreground",
                        )}
                      />
                      <span className="truncate">{t("sidebar.sortName")}</span>
                    </span>
                    {sidebarProjectSort === "alphabetical" && (
                      <Check className="text-foreground h-[13px] w-[13px] shrink-0 stroke-[2]" />
                    )}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() => orbitActions.setSidebarViewPrefs({ projectSort: "manual" })}
                    className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <ArrowUpDown
                        className={cn(
                          "h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]",
                          sidebarProjectSort === "manual"
                            ? "text-foreground/80"
                            : "text-muted-foreground",
                        )}
                      />
                      <span className="truncate">{t("sidebar.sortCustom")}</span>
                    </span>
                    {sidebarProjectSort === "manual" && (
                      <Check className="text-foreground h-[13px] w-[13px] shrink-0 stroke-[2]" />
                    )}
                  </DropdownMenuItem>

                  <DropdownMenuSeparator className="my-1" />

                  <DropdownMenuLabel className="text-muted-foreground/70 px-2 pt-0.5 pb-0.5 text-[11px] font-medium">
                    {t("sidebar.show")}
                  </DropdownMenuLabel>
                  <DropdownMenuItem
                    onSelect={() => orbitActions.setSidebarViewPrefs({ threadFilter: "all" })}
                    className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <List
                        className={cn(
                          "h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]",
                          sidebarThreadFilter === "all"
                            ? "text-foreground/80"
                            : "text-muted-foreground",
                        )}
                      />
                      <span className="truncate">{t("sidebar.filterAll")}</span>
                    </span>
                    {sidebarThreadFilter === "all" && (
                      <Check className="text-foreground h-[13px] w-[13px] shrink-0 stroke-[2]" />
                    )}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() =>
                      orbitActions.setSidebarViewPrefs({ threadFilter: "active_unread" })
                    }
                    className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <CircleDot
                        className={cn(
                          "h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]",
                          sidebarThreadFilter === "active_unread"
                            ? "text-foreground/80"
                            : "text-muted-foreground",
                        )}
                      />
                      <span className="truncate">{t("sidebar.filterActiveUnread")}</span>
                    </span>
                    {sidebarThreadFilter === "active_unread" && (
                      <Check className="text-foreground h-[13px] w-[13px] shrink-0 stroke-[2]" />
                    )}
                  </DropdownMenuItem>

                  <DropdownMenuSeparator className="my-1" />

                  {displayProjects.length > 1 && (
                    <DropdownMenuItem
                      onSelect={() => {
                        orbitActions.setAllProjectsCollapsed(
                          displayProjects.map(({ project }) => project.id),
                          anyProjectExpanded,
                        );
                      }}
                      className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
                    >
                      <span className="flex min-w-0 items-center gap-2">
                        {anyProjectExpanded ? (
                          <ChevronsDownUp className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                        ) : (
                          <ChevronsUpDown className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                        )}
                        <span className="truncate">
                          {anyProjectExpanded ? t("sidebar.collapseAll") : t("sidebar.expandAll")}
                        </span>
                      </span>
                    </DropdownMenuItem>
                  )}

                  <DropdownMenuItem
                    onSelect={() => orbitActions.openSettings("archived")}
                    className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <Archive className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                      <span className="truncate">{t("sidebar.archived")}</span>
                    </span>
                    {archivedCount > 0 && (
                      <span className="text-muted-foreground/70 shrink-0 text-[11px] tabular-nums">
                        {archivedCount}
                      </span>
                    )}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>

              <AddProjectButton hosts={fleet.hosts} />
            </div>
          </div>

          {hasFilterActive && (
            <div className="flex h-[24px] items-center justify-between gap-2 px-2 text-[11.5px]">
              <span className="text-sky-600 dark:text-sky-400 truncate font-medium">
                {t("sidebar.showingActiveUnread")}
              </span>
              <button
                type="button"
                onClick={() => orbitActions.setSidebarViewPrefs({ threadFilter: "all" })}
                className="text-muted-foreground hover:text-foreground shrink-0 cursor-pointer transition-colors"
              >
                {t("common.reset")}
              </button>
            </div>
          )}

          {!fleetHydrated && fleet.hosts.length === 0 ? null : fleet.hosts.length === 0 ? (
            <div className="text-muted-foreground mx-1 mt-1 space-y-2.5 rounded-xl border border-dashed p-4 text-center text-[13px]">
              <p>{t("sidebar.noHostConnected")}</p>
              <button
                type="button"
                onClick={() => orbitActions.openSettings("hosts")}
                className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium hover:opacity-90"
              >
                <Plus className="h-[14px] w-[14px]" /> {t("sidebar.connectHost")}
              </button>
            </div>
          ) : projectRenderModels.length === 0 ? (
            <div className="text-muted-foreground/75 px-2 py-2 text-[13px]">
              {isFilteringThreads ? (
                <div className="flex items-center justify-between gap-2">
                  <span>{t("sidebar.noMatchingThreads")}</span>
                  <button
                    type="button"
                    onClick={() => {
                      setFilterQuery("");
                      setKeyboardFocusedNavId(null);
                    }}
                    className="text-muted-foreground hover:text-foreground shrink-0 cursor-pointer text-[12px] transition-colors"
                  >
                    {t("common.reset")}
                  </button>
                </div>
              ) : hasFilterActive ? (
                t("sidebar.noActiveOrUnread")
              ) : (
                t("sidebar.noProjectsYet")
              )}
            </div>
          ) : (
            <div className="space-y-0.5">
              {projectRenderModels.map(
                ({
                  project: proj,
                  host,
                  visibleConversations,
                  collapsed,
                  projThreadScopeKey,
                  normProjRoot,
                  curatedWorkspaces,
                  unclassifiedConversations,
                  slicedUnclassified,
                  selectedUnclassifiedIdx,
                  isExpandedAll,
                  totalUnclassified,
                  hiddenCount,
                  titleIndicesByThreadKey,
                }) => {
                const projMenuOpen = projMenu?.project.id === proj.id;
                const isDraggingThis = draggingProjectId === proj.id;
                const showDropBefore =
                  dropIndicator?.targetId === proj.id && dropIndicator.position === "before";
                const showDropAfter =
                  dropIndicator?.targetId === proj.id && dropIndicator.position === "after";

                const startProjectDrag = (e: React.PointerEvent<HTMLDivElement>) => {
                  if (e.button !== 0 || displayProjects.length < 2) return;
                  const targetEl = e.target as HTMLElement | null;
                  if (targetEl?.closest("[data-proj-action]")) return;
                  const startY = e.clientY;
                  let activeDrag = false;
                  let currentDrop: { targetId: string; position: "before" | "after" } | null =
                    null;

                  const onMove = (ev: PointerEvent) => {
                    if (!activeDrag) {
                      if (Math.abs(ev.clientY - startY) < 4) return;
                      activeDrag = true;
                      suppressNextProjectClickRef.current = true;
                      setDraggingProjectId(proj.id);
                      document.body.style.cursor = "grabbing";
                    }

                    let best: { targetId: string; position: "before" | "after" } | null = null;
                    for (const entry of displayProjects) {
                      const el = projectRowRefs.current.get(entry.project.id);
                      if (!el) continue;
                      const rect = el.getBoundingClientRect();
                      if (ev.clientY >= rect.top - 8 && ev.clientY <= rect.bottom + 8) {
                        const mid = rect.top + rect.height / 2;
                        best = {
                          targetId: entry.project.id,
                          position: ev.clientY < mid ? "before" : "after",
                        };
                        break;
                      }
                    }
                    if (!best && displayProjects.length > 0) {
                      const firstRect = projectRowRefs.current
                        .get(displayProjects[0].project.id)
                        ?.getBoundingClientRect();
                      if (firstRect && ev.clientY < firstRect.top) {
                        best = { targetId: displayProjects[0].project.id, position: "before" };
                      } else {
                        best = {
                          targetId: displayProjects[displayProjects.length - 1].project.id,
                          position: "after",
                        };
                      }
                    }
                    currentDrop = best;
                    setDropIndicator(best);
                  };

                  const finish = (commit: boolean) => {
                    window.removeEventListener("pointermove", onMove);
                    window.removeEventListener("pointerup", onPointerUp);
                    window.removeEventListener("pointercancel", onCancel);
                    window.removeEventListener("keydown", onKeyDown);
                    dragCleanupRef.current = null;
                    document.body.style.cursor = "";

                    if (activeDrag) {
                      setDraggingProjectId(null);
                      setDropIndicator(null);
                      if (commit && currentDrop && currentDrop.targetId !== proj.id) {
                        const ordered = displayProjects.map((d) => d.project);
                        const fromIdx = ordered.findIndex((p) => p.id === proj.id);
                        const [moved] = ordered.splice(fromIdx, 1);
                        let toIdx = ordered.findIndex((p) => p.id === currentDrop?.targetId);
                        if (toIdx >= 0) {
                          if (currentDrop.position === "after") toIdx += 1;
                          ordered.splice(toIdx, 0, moved);
                          const keys = ordered.map(
                            (p) =>
                              p.sort_order_key ||
                              p.pinned_project_id ||
                              projectPathScopeKey(p.host_id, p.primary_cwd),
                          );
                          void orbitActions.reorderProjects(keys);
                        }
                      }
                      window.setTimeout(() => {
                        suppressNextProjectClickRef.current = false;
                      }, 80);
                    }
                  };

                  const onPointerUp = () => finish(true);
                  const onCancel = () => finish(false);
                  const onKeyDown = (ev: KeyboardEvent) => {
                    if (ev.key === "Escape") finish(false);
                  };

                  dragCleanupRef.current?.();
                  dragCleanupRef.current = onCancel;
                  window.addEventListener("pointermove", onMove);
                  window.addEventListener("pointerup", onPointerUp);
                  window.addEventListener("pointercancel", onCancel);
                  window.addEventListener("keydown", onKeyDown);
                };

                const hasCollapsedApproval =
                  collapsed &&
                  visibleConversations.some((c) => c.agent.is_awaiting_permission);
                const hasCollapsedRunning =
                  collapsed &&
                  !hasCollapsedApproval &&
                  visibleConversations.some(
                    (c) =>
                      isAgentForegroundRunning(c.agent) ||
                      hasAgentRunningSubagents(proj.host_id, c.agent),
                  );
                const hasCollapsedUnread =
                  collapsed &&
                  !hasCollapsedApproval &&
                  !hasCollapsedRunning &&
                  visibleConversations.some((c) => isConvUnread(c));

                const organizeEvent =
                  hostDetails[proj.host_id]?.workspace_organize_status?.[normProjRoot] ??
                  hostDetails[proj.host_id]?.workspace_organize_status?.[proj.primary_cwd] ??
                  null;
                const isProjectOrganizing = organizeEvent?.phase === "running";
                const hasProjectBody =
                  curatedWorkspaces.length > 0 || slicedUnclassified.length > 0;

                // Keep latest full scope orders synced for pointer-drop handlers
                scopeAgentIdsRef.current.set(
                  projThreadScopeKey,
                  unclassifiedConversations.map((c) => c.agent.agent_id),
                );
                for (const ws of curatedWorkspaces) {
                  scopeAgentIdsRef.current.set(
                    ws.threadScopeKey,
                    ws.visiblePeers.map((c) => c.agent.agent_id),
                  );
                }

                const makeThreadPointerProps = (
                  scopeKey: string,
                  workspaceId: string | null,
                  agentId: string,
                  title: string,
                  navId: string,
                ) => {
                  const refKey = `${proj.host_id}::${agentId}`;
                  return {
                    rowRef: (el: HTMLDivElement | null) => {
                      if (el) {
                        navRowRefs.current.set(navId, el);
                        threadRowRefs.current.set(refKey, {
                          el,
                          hostId: proj.host_id,
                          projectId: proj.id,
                          agentId,
                          scopeKey,
                          workspaceId,
                        });
                      } else {
                        navRowRefs.current.delete(navId);
                        const existing = threadRowRefs.current.get(refKey);
                        if (existing && existing.scopeKey === scopeKey) {
                          threadRowRefs.current.delete(refKey);
                        }
                      }
                    },
                    isDragging:
                      draggingThread?.hostId === proj.host_id &&
                      draggingThread?.agentId === agentId,
                    dropIndicator:
                      threadDropIndicator?.scopeKey === scopeKey &&
                      threadDropIndicator.targetAgentId === agentId
                        ? threadDropIndicator.position
                        : null,
                    shouldSuppressClick: shouldSuppressThreadClick,
                    onThreadPointerDown: (e: React.PointerEvent<HTMLDivElement>) =>
                      startThreadDrag(e, {
                        hostId: proj.host_id,
                        projectId: proj.id,
                        agentId,
                        title,
                        scopeKey,
                        workspaceId,
                      }),
                  };
                };

                return (
                  <div
                    key={proj.id}
                    ref={(el) => {
                      if (el) projectRowRefs.current.set(proj.id, el);
                      else projectRowRefs.current.delete(proj.id);
                    }}
                    className={cn(
                      "relative space-y-0.5 transition-opacity",
                      isDraggingThis && "opacity-45",
                    )}
                  >
                    {showDropBefore && (
                      <div className="bg-primary pointer-events-none absolute -top-1 right-1 left-1 z-20 h-[2px] rounded-full" />
                    )}

                    {/* Project Header Row (`30px`, pointer-draggable to reorder) */}
                    {(() => {
                      const isRenamingThisProj = renamingProjectId === proj.id;
                      const showRemoteHostTag = !singleHostMode && !isLocalHostCard(host);
                      const isHostDisconnected =
                        !singleHostMode && host.connection_state === "disconnected";
                      const commitProjRename = () => {
                        if (projectRenameCancelledRef.current) {
                          projectRenameCancelledRef.current = false;
                          setRenamingProjectId(null);
                          return;
                        }
                        const clean = projectRenameDraft.trim();
                        setRenamingProjectId(null);
                        if (clean && clean !== proj.name) {
                          void orbitActions.renameProject(proj, clean);
                        }
                      };
                      const projTooltip = [
                        !singleHostMode ? `${host.display_name} · ${proj.primary_cwd}` : proj.primary_cwd,
                        displayProjects.length > 1 ? t("sidebar.dragToReorder") : null,
                      ]
                        .filter(Boolean)
                        .join(" · ");

                      return (
                        <div
                          onPointerDown={isRenamingThisProj ? undefined : startProjectDrag}
                          onContextMenu={(e) => {
                            if (isRenamingThisProj) return;
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
                          {isRenamingThisProj ? (
                            <div className="flex min-w-0 flex-1 items-center gap-2">
                              <span className="text-muted-foreground inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
                                {collapsed ? (
                                  <Folder className="h-[15px] w-[15px] stroke-[1.75]" />
                                ) : (
                                  <FolderOpen className="h-[15px] w-[15px] stroke-[1.75]" />
                                )}
                              </span>
                              <input
                                autoFocus
                                data-proj-action="true"
                                value={projectRenameDraft}
                                onFocus={(e) => e.currentTarget.select()}
                                onChange={(e) => setProjectRenameDraft(e.target.value)}
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
                                    projectRenameCancelledRef.current = true;
                                    setRenamingProjectId(null);
                                  }
                                }}
                                className="bg-background text-foreground ring-foreground/25 -mx-1 h-[22px] min-w-0 flex-1 rounded-[5px] px-1.5 text-[13.5px] font-medium ring-1 outline-none"
                              />
                            </div>
                          ) : (
                            <button
                              type="button"
                              title={projTooltip}
                              onClick={() => {
                                if (suppressNextProjectClickRef.current) return;
                                orbitActions.toggleProjectCollapsed(proj.id);
                              }}
                              className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
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

                              <span className="truncate text-[13.5px] font-medium">
                                {proj.name}
                              </span>

                              {showRemoteHostTag && (
                                <span className="text-muted-foreground/60 ml-0.5 inline-flex shrink-0 items-center gap-1 text-[11px]">
                                  <HostStateDot state={host.connection_state} />
                                  <span className="max-w-[72px] truncate">{host.display_name}</span>
                                </span>
                              )}
                            </button>
                          )}

                          {/* Right Slot: Resting Status / Collapsed Count ↔ Compact 2-Action Hover (`⋯` + `+`) */}
                          {!isRenamingThisProj && (
                            <div className="flex shrink-0 items-center justify-end">
                              {isHostDisconnected ? (
                                <button
                                  type="button"
                                  data-proj-action="true"
                                  title={t("sidebar.offlineReconnect", { host: host.display_name })}
                                  onClick={() => {
                                    void orbitIpc.connectHost(host.host_id);
                                    void orbitIpc.refreshHost(host.host_id);
                                  }}
                                  className={cn(
                                    "text-muted-foreground/75 hover:text-foreground h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[5px]",
                                    projMenuOpen ? "hidden" : "inline-flex group-hover/proj:hidden",
                                  )}
                                >
                                  <RefreshCw className="h-[12.5px] w-[12.5px]" />
                                </button>
                              ) : hasCollapsedApproval || hasCollapsedRunning || hasCollapsedUnread ? (
                                <span
                                  className={cn(
                                    "h-[22px] w-[22px] items-center justify-center",
                                    projMenuOpen ? "hidden" : "inline-flex group-hover/proj:hidden",
                                  )}
                                >
                                  {hasCollapsedApproval ? (
                                    <span className="h-[6px] w-[6px] rounded-full bg-amber-500" />
                                  ) : hasCollapsedRunning ? (
                                    <OrbitSpinner size={13} className="text-foreground/65" />
                                  ) : (
                                    <span className="h-[6px] w-[6px] rounded-full bg-sky-500" />
                                  )}
                                </span>
                              ) : collapsed && visibleConversations.length > 0 ? (
                                <span
                                  className={cn(
                                    "text-muted-foreground/45 pr-1 text-[11px] tabular-nums",
                                    projMenuOpen ? "hidden" : "inline-flex group-hover/proj:hidden",
                                  )}
                                >
                                  {visibleConversations.length}
                                </span>
                              ) : null}

                              <div
                                className={cn(
                                  "items-center gap-0.5",
                                  projMenuOpen
                                    ? "inline-flex"
                                    : "hidden group-hover/proj:inline-flex",
                                )}
                              >
                                <button
                                  type="button"
                                  data-proj-action="true"
                                  aria-label={t("sidebar.projectOptions", { project: proj.name })}
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
                                  data-proj-action="true"
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
                          )}
                        </div>
                      );
                    })()}

                    {/* Optional Groups + Flat Project Threads (or quiet empty-project CTA) */}
                    <div
                      data-open={!collapsed ? "true" : "false"}
                      className="orbit-collapse-grid"
                    >
                      <div className="orbit-collapse-inner">
                        {!hasProjectBody ? (
                          <div className="pt-0.5">
                            {hasFilterActive ? (
                              <div className="text-muted-foreground/55 flex h-[28px] items-center gap-2.5 px-2 text-[12px]">
                                <span className="inline-flex h-[16px] w-[16px] shrink-0" />
                                <span className="truncate">No active or unread threads</span>
                              </div>
                            ) : (
                              <button
                                type="button"
                                onClick={() =>
                                  orbitActions.startNewThreadDraft(proj.host_id, proj.primary_cwd)
                                }
                                className="text-muted-foreground/65 hover:text-foreground flex h-[28px] w-full cursor-pointer items-center gap-2.5 rounded-[7px] px-2 text-left text-[12.5px] transition-colors hover:bg-black/[0.035] dark:hover:bg-white/[0.045]"
                              >
                                <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
                                  <Plus className="h-[12.5px] w-[12.5px] stroke-[1.75] opacity-75" />
                                </span>
                                <span className="truncate">Start a conversation</span>
                              </button>
                            )}
                          </div>
                        ) : (
                          <div className="space-y-0.5 py-0.5">
                            {/* 1. Optional Groups (borderless, lightweight collapsible sections — zero chrome when none exist) */}
                            {curatedWorkspaces.map((ws) => {
                              const wsCollapsed = ws.wsCollapsed;
                              const wsHasRunning = ws.visiblePeers.some(
                                (c) =>
                                  isAgentForegroundRunning(c.agent) ||
                                  hasAgentRunningSubagents(proj.host_id, c.agent),
                              );
                              const wsHasUnread = ws.visiblePeers.some((c) => isConvUnread(c));
                              const isRenamingThisWs = renamingWorkspaceId === ws.workspace_id;
                              const isDropHover = dragOverDropId === `ws:${ws.workspace_id}`;

                              const commitWsRename = () => {
                                const clean = workspaceRenameDraft.trim();
                                setRenamingWorkspaceId(null);
                                if (clean && clean !== ws.title) {
                                  void orbitActions.renameWorkspace(
                                    proj.host_id,
                                    ws.workspace_id,
                                    clean,
                                  );
                                }
                              };

                              return (
                                <div
                                  key={`ws-${ws.workspace_id}`}
                                  className={cn(
                                    "space-y-0.5 rounded-[8px] transition-colors",
                                    isDropHover &&
                                      "bg-foreground/[0.04] ring-1 ring-foreground/20",
                                  )}
                                >
                                  <div
                                    ref={(el) => {
                                      if (el) {
                                        workspaceHeaderRefs.current.set(ws.workspace_id, {
                                          el,
                                          hostId: proj.host_id,
                                          projectId: proj.id,
                                          workspaceId: ws.workspace_id,
                                        });
                                      } else {
                                        workspaceHeaderRefs.current.delete(ws.workspace_id);
                                      }
                                    }}
                                    onContextMenu={(e) => {
                                      e.preventDefault();
                                      setWsMenu({
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
                                    className="group/ws flex h-[28px] items-center justify-between gap-1 rounded-[8px] px-2 transition-colors hover:bg-black/[0.035] dark:hover:bg-white/[0.045]"
                                  >
                                    {isRenamingThisWs ? (
                                      <div className="flex min-w-0 flex-1 items-center gap-2">
                                        <Layers className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0" />
                                        <input
                                          autoFocus
                                          value={workspaceRenameDraft}
                                          onChange={(e) => setWorkspaceRenameDraft(e.target.value)}
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
                                              setRenamingWorkspaceId(null);
                                            }
                                          }}
                                          className="bg-background text-foreground ring-foreground/25 h-[20px] min-w-0 flex-1 rounded px-1.5 text-[12.5px] ring-1 outline-none"
                                        />
                                      </div>
                                    ) : (
                                      <button
                                        type="button"
                                        title={
                                          ws.mission_goal
                                            ? `${ws.title}\n${ws.mission_goal}`
                                            : `${ws.title} · Double-click to rename`
                                        }
                                        onClick={() =>
                                          orbitActions.toggleWorkspaceCollapsed(ws.workspace_id)
                                        }
                                        onDoubleClick={(e) => {
                                          e.stopPropagation();
                                          setWorkspaceRenameDraft(ws.title);
                                          setRenamingWorkspaceId(ws.workspace_id);
                                        }}
                                        className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
                                      >
                                        <span className="text-muted-foreground relative inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
                                          <Layers className="h-[13.5px] w-[13.5px] stroke-[1.7] transition-opacity group-hover/ws:opacity-0" />
                                          {wsCollapsed ? (
                                            <ChevronRight className="absolute h-[13px] w-[13px] opacity-0 transition-opacity group-hover/ws:opacity-85" />
                                          ) : (
                                            <ChevronDown className="absolute h-[13px] w-[13px] opacity-0 transition-opacity group-hover/ws:opacity-85" />
                                          )}
                                        </span>
                                        <span className="text-foreground/82 truncate text-[12.5px] font-medium">
                                          {ws.title}
                                        </span>
                                        <span className="text-muted-foreground/65 shrink-0 text-[11px] tabular-nums">
                                          {ws.visiblePeers.length}
                                        </span>
                                        {ws.has_conflict && (
                                          <AlertTriangle className="h-[11.5px] w-[11.5px] shrink-0 text-amber-500" />
                                        )}
                                        {!ws.has_conflict && wsHasRunning && (
                                          <OrbitSpinner
                                            size={11}
                                            className="text-foreground/65 shrink-0"
                                          />
                                        )}
                                        {!ws.has_conflict &&
                                          !wsHasRunning &&
                                          wsCollapsed &&
                                          wsHasUnread && (
                                            <span className="h-[5.5px] w-[5.5px] shrink-0 rounded-full bg-sky-500" />
                                          )}
                                      </button>
                                    )}

                                    {!isRenamingThisWs && (
                                      <div
                                        className={cn(
                                          "shrink-0 items-center gap-0.5",
                                          wsMenu?.workspace.workspace_id === ws.workspace_id
                                            ? "flex"
                                            : "hidden group-hover/ws:flex",
                                        )}
                                      >
                                        <button
                                          type="button"
                                          aria-label={`Group options for ${ws.title}`}
                                          onClick={(e) => {
                                            e.stopPropagation();
                                            const r = e.currentTarget.getBoundingClientRect();
                                            setWsMenu({
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
                                          className="text-muted-foreground hover:text-foreground inline-flex h-[20px] w-[20px] cursor-pointer items-center justify-center rounded hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
                                        >
                                          <MoreHorizontal className="h-[12.5px] w-[12.5px]" />
                                        </button>
                                        <button
                                          type="button"
                                          title={`New thread in ${ws.title}`}
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
                                          className="text-muted-foreground hover:text-foreground inline-flex h-[20px] w-[20px] cursor-pointer items-center justify-center rounded hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
                                        >
                                          <Plus className="h-[12.5px] w-[12.5px]" />
                                        </button>
                                      </div>
                                    )}
                                  </div>

                                  {!wsCollapsed && (
                                    <div className="ml-[15px] space-y-0.5 border-l border-black/[0.06] pl-1.5 dark:border-white/[0.08]">
                                      {ws.visiblePeers.length === 0 ? (
                                        <div className="text-muted-foreground/55 px-2 py-1 text-[11.5px]">
                                          Empty group
                                        </div>
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
                                          const unread = isConvUnread(conv);
                                          const effectiveTitle =
                                            threadCustomTitles[tKey] || ag.title;

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
                                              hasRunningSubagents={hasAgentRunningSubagents(
                                                proj.host_id,
                                                ag,
                                              )}
                                              selected={selected}
                                              keyboardFocused={keyboardFocusedNavId === navId}
                                              highlightIndices={titleIndicesByThreadKey.get(tKey)}
                                              isChild={false}
                                              peerHandle={ag.handle}
                                              nowMs={nowMs}
                                              menuOpen={convMenu?.threadKey === tKey}
                                              isRenaming={renamingThreadKey === tKey}
                                              {...makeThreadPointerProps(
                                                ws.threadScopeKey,
                                                ws.workspace_id,
                                                ag.agent_id,
                                                effectiveTitle,
                                                navId,
                                              )}
                                              onOpenMenu={handleOpenConvMenu}
                                              onStartRename={handleStartRename}
                                              onFinishRename={handleFinishRename}
                                            />
                                          );
                                        })
                                      )}
                                    </div>
                                  )}
                                </div>
                              );
                            })}

                            {/* 2. Flat Project Threads (No "Unclassified" box — drop here to ungroup) */}
                            {isProjectOrganizing && organizeEvent?.message && (
                              <div className="text-muted-foreground px-2 py-1 text-[11.5px]">
                                {organizeEvent.message}
                              </div>
                            )}

                            {totalUnclassified > 0 && (
                              <div
                                ref={(el) => {
                                  if (el) {
                                    inboxAreaRefs.current.set(proj.id, {
                                      el,
                                      hostId: proj.host_id,
                                      projectId: proj.id,
                                      scopeKey: projThreadScopeKey,
                                    });
                                  } else {
                                    inboxAreaRefs.current.delete(proj.id);
                                  }
                                }}
                                className={cn(
                                  "space-y-0.5 rounded-[8px] transition-colors",
                                  curatedWorkspaces.length > 0 && "pt-0.5",
                                  dragOverDropId === `inbox:${proj.id}` &&
                                    "bg-foreground/[0.04] ring-1 ring-foreground/20",
                                )}
                              >
                                {slicedUnclassified.map((conv) => {
                                  const ag = conv.agent;
                                  const tKey = threadScopeKey(proj.host_id, ag.agent_id);
                                  const navId = `proj:${proj.id}:${proj.host_id}:${ag.agent_id}`;
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
                                      hasRunningSubagents={hasAgentRunningSubagents(
                                        proj.host_id,
                                        ag,
                                      )}
                                      selected={selected}
                                      keyboardFocused={keyboardFocusedNavId === navId}
                                      highlightIndices={titleIndicesByThreadKey.get(tKey)}
                                      isChild={Boolean(ag.parent_agent_id)}
                                      nowMs={nowMs}
                                      menuOpen={convMenu?.threadKey === tKey}
                                      isRenaming={renamingThreadKey === tKey}
                                      {...makeThreadPointerProps(
                                        projThreadScopeKey,
                                        null,
                                        ag.agent_id,
                                        effectiveTitle,
                                        navId,
                                      )}
                                      onOpenMenu={handleOpenConvMenu}
                                      onStartRename={handleStartRename}
                                      onFinishRename={handleFinishRename}
                                    />
                                  );
                                })}

                                {hiddenCount > 0 &&
                                  !expandedMoreProjects[proj.id] &&
                                  !isExpandedAll && (
                                    <button
                                      type="button"
                                      onClick={() =>
                                        setExpandedMoreProjects((prev) => ({
                                          ...prev,
                                          [proj.id]: true,
                                        }))
                                      }
                                  className="text-muted-foreground/68 hover:text-foreground flex h-[26px] w-full cursor-pointer items-center gap-2.5 rounded-[7px] px-2 text-left text-[12px] transition-colors hover:bg-black/[0.03] dark:hover:bg-white/[0.04]"
                                >
                                  <span className="inline-flex h-[16px] w-[16px] shrink-0" />
                                  <span>{t("common.showMore", { count: hiddenCount })}</span>
                                </button>
                              )}

                            {hiddenCount > 0 &&
                              Boolean(expandedMoreProjects[proj.id]) &&
                              selectedUnclassifiedIdx <
                                MAX_VISIBLE_CONVERSATIONS_PER_PROJECT && (
                                <button
                                  type="button"
                                  onClick={() =>
                                    setExpandedMoreProjects((prev) => ({
                                      ...prev,
                                      [proj.id]: false,
                                    }))
                                  }
                                  className="text-muted-foreground/68 hover:text-foreground flex h-[26px] w-full cursor-pointer items-center gap-2.5 rounded-[7px] px-2 text-left text-[12px] transition-colors hover:bg-black/[0.03] dark:hover:bg-white/[0.04]"
                                >
                                  <span className="inline-flex h-[16px] w-[16px] shrink-0" />
                                  <span>{t("common.showLess")}</span>
                                </button>
                              )}
                          </div>
                        )}
                          </div>
                        )}
                      </div>
                    </div>

                    {showDropAfter && (
                      <div className="bg-primary pointer-events-none absolute right-1 -bottom-1 left-1 z-20 h-[2px] rounded-full" />
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
        </div>
      </ScrollArea>

      {/* 4. Ultra-Clean Footer: Settings (`⌘,`) + Conditional Multi-Host Health Pill */}
      <div className="flex shrink-0 items-center justify-between gap-1 px-2 py-2">
        <button
          type="button"
          onClick={() => orbitActions.openSettings("general")}
          className="group/settings text-foreground/78 hover:text-foreground flex h-[30px] min-w-0 flex-1 cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 text-[13.5px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.055]"
        >
          <span className="flex min-w-0 items-center gap-2.5">
            <span className="relative inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
              {isFleetUpdating ? (
                <OrbitSpinner size={14} className="text-sky-500" />
              ) : (
                <Settings className="h-[15.5px] w-[15.5px] stroke-[1.75] opacity-80" />
              )}
              {!isFleetUpdating && hasFleetUpdatesAvailable && (
                <span
                  aria-label={t("sidebar.updateAvailable")}
                  className="absolute -top-0.5 -right-0.5 h-[6px] w-[6px] rounded-full bg-sky-500"
                />
              )}
            </span>
            <span className="truncate">{t("sidebar.settings")}</span>
          </span>

          {!isFleetUpdating && hasFleetUpdatesAvailable ? (
            <span className="shrink-0 rounded-full bg-sky-500/12 px-1.5 py-0.5 text-[10px] font-medium text-sky-600 dark:text-sky-400">
              {t("sidebar.updateBadge")}
            </span>
          ) : (
            <span className="text-muted-foreground/55 shrink-0 text-[11.5px] opacity-0 transition-opacity group-hover/settings:opacity-100">
              {mod},
            </span>
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
              className="text-muted-foreground hover:text-foreground inline-flex h-[28px] shrink-0 cursor-pointer items-center gap-1.5 rounded-[7px] px-2 text-[11.5px] tabular-nums transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.055]"
            >
              {fleetHealth.connecting > 0 ? (
                <OrbitSpinner size={11} className="text-amber-500" />
              ) : (
                <span
                  className={cn(
                    "h-[6px] w-[6px] rounded-full",
                    fleetHealth.online === fleetHealth.total
                      ? "bg-emerald-500"
                      : fleetHealth.online > 0
                        ? "bg-amber-500"
                        : "bg-zinc-400/80",
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

      {/* Shared Single-Instance Context/Options Menus (0 Radix instances per row!) */}
      <SharedConversationMenu
        target={convMenu}
        onClose={handleCloseConvMenu}
        onStartRename={handleStartRename}
        onRequestCreateWorkspace={setCreateWsTarget}
        onRequestDelete={setDeleteConversationTarget}
      />
      <SharedProjectMenu
        target={projMenu}
        isMac={isMac}
        onClose={handleCloseProjMenu}
        onStartRenameProject={(proj) => {
          projectRenameCancelledRef.current = false;
          setProjectRenameDraft(proj.name);
          setRenamingProjectId(proj.id);
        }}
        onRequestDelete={setDeleteProjectTarget}
        onRequestCreateWorkspace={setCreateWsTarget}
      />
      <SharedWorkspaceMenu
        target={wsMenu}
        onClose={() => setWsMenu(null)}
        onStartRenameWorkspace={(wsId) => {
          const found = rawProjects
            .flatMap((p) => p.workspaces ?? [])
            .find((w) => w.workspace_id === wsId);
          setWorkspaceRenameDraft(found?.title ?? "");
          setRenamingWorkspaceId(wsId);
        }}
      />
      <CreateWorkspaceDialog
        target={createWsTarget}
        onClose={() => setCreateWsTarget(null)}
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

      {/* Pointer-driven floating drag preview (100% compatible with macOS Tauri WKWebView) */}
      {draggingThread && (
        <div
          style={{
            left: `${draggingThread.clientX + 12}px`,
            top: `${draggingThread.clientY - 14}px`,
          }}
          className="bg-elevated/95 text-foreground floating-surface pointer-events-none fixed z-50 max-w-[210px] truncate rounded-[8px] border border-black/[0.09] px-2.5 py-1 text-[12.5px] font-medium dark:border-white/[0.12]"
        >
          {draggingThread.title}
        </div>
      )}
    </aside>
  );
});
