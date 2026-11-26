import { fuzzyMatch } from "../../lib/fuzzy-match.ts";
import type {
  AgentCardView,
  FleetSidebarView,
  HostCardView,
  HostDetailView,
  SidebarConversationView,
  SidebarProjectGroupView,
} from "../../lib/orbit-ipc.ts";
import type {
  ArchivedThreadEntry,
  SidebarProjectSort,
  StageRunningSubagentRef,
  WorkspacePanelMode,
} from "../../state/store-types.ts";

export const MAX_VISIBLE_CONVERSATIONS_PER_PROJECT = 6;
export const PINNED_MANUAL_ORDER_SCOPE = "__pinned__";

function threadKey(hostId: string, agentId: string): string {
  return `${hostId}::${agentId}`;
}

function stripVerbatimPrefix(rawPath: string): string {
  if (rawPath.startsWith("\\\\?\\UNC\\") || rawPath.startsWith("//?/UNC/")) {
    return `\\\\${rawPath.slice(8)}`;
  }
  if (rawPath.startsWith("\\\\?\\") || rawPath.startsWith("//?/")) {
    return rawPath.slice(4);
  }
  return rawPath;
}

export function normalizePath(path: string): string {
  if (!path || path === "~") return "~";
  const clean = stripVerbatimPrefix(path.trim());
  if (!clean) return "~";
  if (clean === "/" || clean === "\\") return clean;
  if (/^[A-Za-z]:[\\/]+$/.test(clean)) {
    return `${clean[0].toUpperCase()}:/`;
  }
  return clean.replace(/\\/g, "/").replace(/\/+$/, "");
}

function basenameOfPath(path: string): string {
  if (!path || path === "~") return "~";
  const clean = normalizePath(path);
  if (clean === "/" || clean === "\\" || /^[A-Za-z]:[\\/]?$/.test(clean)) {
    return clean;
  }
  const normalized = clean.replace(/\\/g, "/").replace(/\/+$/, "");
  const i = normalized.lastIndexOf("/");
  return i >= 0 ? normalized.slice(i + 1) || "/" : normalized;
}

function normalizeForCompare(path: string): string {
  const norm = normalizePath(path);
  return /^[A-Za-z]:\//.test(norm) ? norm.toLowerCase() : norm;
}

function matchesProjectDir(directories: readonly string[], cwd: string | null | undefined): boolean {
  if (!cwd) return false;
  const target = normalizeForCompare(cwd);
  return directories.some((dir) => {
    const base = normalizeForCompare(dir);
    return target === base || (base !== "/" && target.startsWith(`${base}/`));
  });
}

function projectScopeKey(hostId: string, cwd: string): string {
  return `${hostId}::${normalizePath(cwd)}`;
}

function isForegroundRunning(
  agent: Pick<AgentCardView, "is_running" | "has_background_tasks" | "status_label">,
): boolean {
  if (!agent.is_running || agent.has_background_tasks) return false;
  const label = (agent.status_label || "").trim().toLowerCase();
  if (!label) return true;
  return !label.startsWith("waiting") && !label.startsWith("background");
}

export function isSidebarAgentArchived(
  hostId: string,
  agent: Pick<AgentCardView, "agent_id" | "archived_at_rfc3339">,
  archivedThreads: Record<string, ArchivedThreadEntry>,
): boolean {
  if (agent.archived_at_rfc3339) return true;
  return Boolean(archivedThreads[threadKey(hostId, agent.agent_id)]);
}

export function applyManualThreadOrder(
  list: SidebarConversationView[],
  manualIds: readonly string[] | undefined,
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

/**
 * Computes the set of parent thread scope keys (`hostId::agentId`) that currently have
 * at least one running managed, provider-native, or stage-tracked subagent.
 * Reads ONLY `hostDetails` + lightweight `stageRunningSubagentMeta` (zero `stages` topic coupling).
 */
export function buildRunningParentThreadKeys(
  hostDetails: Record<string, HostDetailView>,
  archivedThreads: Record<string, ArchivedThreadEntry>,
  stageRunningSubagentMeta: Record<string, readonly StageRunningSubagentRef[]>,
): Set<string> {
  const keys = new Set<string>();

  // 1. Managed child agents & provider-native subagent descriptors
  const managedAgentsById = new Map<string, AgentCardView>();
  for (const [hid, detail] of Object.entries(hostDetails)) {
    for (const ag of detail.agents) {
      managedAgentsById.set(ag.agent_id, ag);
      if (!ag.parent_agent_id || isSidebarAgentArchived(hid, ag, archivedThreads)) continue;
      if (isForegroundRunning(ag) || ag.is_awaiting_permission) {
        const parentHostId = ag.parent_host_id || hid;
        keys.add(threadKey(parentHostId, ag.parent_agent_id));
      }
    }
    if (detail.provider_subagents) {
      for (const [parentAgentId, descriptors] of Object.entries(detail.provider_subagents)) {
        if (descriptors.some((ps) => ps.status === "running")) {
          keys.add(threadKey(hid, parentAgentId));
        }
      }
    }
  }

  // 2. Stage-tracked running subagent refs verified against live hostDetails
  for (const [tlKey, refs] of Object.entries(stageRunningSubagentMeta)) {
    if (refs.length === 0 || keys.has(tlKey)) continue;
    const sepIdx = tlKey.indexOf("::");
    const hostId = sepIdx >= 0 ? tlKey.slice(0, sepIdx) : "";
    const agentId = sepIdx >= 0 ? tlKey.slice(sepIdx + 2) : "";
    const provList =
      hostId && agentId ? hostDetails[hostId]?.provider_subagents?.[agentId] : undefined;

    for (let i = 0; i < refs.length; i += 1) {
      const ref = refs[i];
      if (ref.child_agent_id) {
        const managedChild = managedAgentsById.get(ref.child_agent_id);
        if (managedChild) {
          if (
            !managedChild.archived_at_rfc3339 &&
            (isForegroundRunning(managedChild) || managedChild.is_awaiting_permission)
          ) {
            keys.add(tlKey);
            break;
          }
          continue;
        }
      }
      if (ref.provider_subagent_id && provList) {
        const provMatch = provList.find((ps) => ps.subagent_id === ref.provider_subagent_id);
        if (provMatch) {
          if (provMatch.status === "running") {
            keys.add(tlKey);
            break;
          }
          continue;
        }
      }
      keys.add(tlKey);
      break;
    }
  }

  return keys;
}

export interface ParsedSidebarFilterQuery {
  hostFilters: string[];
  providerFilters: string[];
  freeQuery: string;
  terms: string[];
}

export function parseSidebarFilterQuery(raw: string): ParsedSidebarFilterQuery {
  const hostFilters: string[] = [];
  const providerFilters: string[] = [];
  const terms: string[] = [];
  for (const tok of raw.trim().split(/\s+/).filter(Boolean)) {
    const low = tok.toLowerCase();
    if (low.startsWith("@host:")) {
      const val = low.slice("@host:".length);
      if (val) hostFilters.push(val);
    } else if (low.startsWith("@provider:")) {
      const val = low.slice("@provider:".length);
      if (val) providerFilters.push(val);
    } else {
      terms.push(low);
    }
  }
  return { hostFilters, providerFilters, freeQuery: terms.join(" "), terms };
}

export function matchSidebarThread(params: {
  trimmedFilter: string;
  effectiveTitle: string;
  projectName: string;
  hostDisplayName: string;
  hostId?: string | null;
  provider?: string | null;
  providerLabel?: string | null;
  branch?: string | null;
  handle?: string | null;
  groupTitle?: string | null;
  singleHostMode: boolean;
}): { matched: boolean; titleIndices?: number[]; groupTitleMatched?: boolean } {
  const {
    trimmedFilter,
    effectiveTitle,
    projectName,
    hostDisplayName,
    hostId,
    provider,
    providerLabel,
    branch,
    handle,
    groupTitle,
  } = params;
  if (!trimmedFilter) return { matched: true };

  const parsed = parseSidebarFilterQuery(trimmedFilter);
  if (parsed.hostFilters.length > 0) {
    const hName = hostDisplayName.toLowerCase();
    const hId = (hostId ?? "").toLowerCase();
    if (!parsed.hostFilters.every((f) => hName.includes(f) || hId.includes(f))) {
      return { matched: false };
    }
  }
  if (parsed.providerFilters.length > 0) {
    const pSlug = (provider ?? "").toLowerCase();
    const pLabel = (providerLabel ?? "").toLowerCase();
    if (!parsed.providerFilters.every((f) => pSlug.includes(f) || pLabel.includes(f))) {
      return { matched: false };
    }
  }
  if (parsed.terms.length === 0) {
    return { matched: true };
  }

  const freeQuery = parsed.freeQuery;
  const titleHit = fuzzyMatch(freeQuery, effectiveTitle);
  if (titleHit) {
    return { matched: true, titleIndices: titleHit.indices };
  }
  if (groupTitle && fuzzyMatch(freeQuery, groupTitle)) {
    return { matched: true, groupTitleMatched: true };
  }
  const haystack = [
    effectiveTitle,
    groupTitle ?? "",
    projectName,
    hostDisplayName,
    hostId ?? "",
    provider ?? "",
    providerLabel ?? "",
    branch ?? "",
    handle ?? "",
  ]
    .join(" ")
    .toLowerCase();
  if (parsed.terms.every((t) => haystack.includes(t))) {
    return { matched: true };
  }
  if (fuzzyMatch(freeQuery, projectName)) {
    return { matched: true };
  }
  if (handle && fuzzyMatch(freeQuery, handle)) {
    return { matched: true };
  }
  if (fuzzyMatch(freeQuery, hostDisplayName)) {
    return { matched: true };
  }
  if (providerLabel && fuzzyMatch(freeQuery, providerLabel)) {
    return { matched: true };
  }
  if (branch && fuzzyMatch(freeQuery, branch)) {
    return { matched: true };
  }
  return { matched: false };
}

export interface SidebarAttentionItem {
  key: string;
  navId: string;
  host: HostCardView;
  conv: SidebarConversationView;
  projectName: string;
  effectiveTitle: string;
  titleIndices?: number[];
}

export interface SidebarPinnedEntry {
  key: string;
  navId: string;
  host: HostCardView;
  projectName: string;
  conv: SidebarConversationView;
  effectiveTitle: string;
  titleIndices?: number[];
}

export type SidebarWorkspaceRenderModel = NonNullable<
  SidebarProjectGroupView["workspaces"]
>[number] & {
  threadScopeKey: string;
  visiblePeers: SidebarConversationView[];
  /** True when the group actually has active peers, even if temporarily promoted to Attention/Pinned */
  hasActivePeersInGroup: boolean;
  wsCollapsed: boolean;
};

export interface SidebarProjectRenderModel {
  project: SidebarProjectGroupView;
  host: HostCardView;
  visibleConversations: SidebarConversationView[];
  collapsed: boolean;
  projThreadScopeKey: string;
  normProjRoot: string;
  curatedWorkspaces: SidebarWorkspaceRenderModel[];
  unclassifiedConversations: SidebarConversationView[];
  slicedUnclassified: SidebarConversationView[];
  selectedUnclassifiedIdx: number;
  isExpandedAll: boolean;
  totalUnclassified: number;
  hiddenCount: number;
  titleIndicesByThreadKey: Map<string, number[]>;
}

export interface SidebarVisibleNavItem {
  navId: string;
  hostId: string;
  agentId: string;
}

export interface SidebarDisplayModel {
  displayProjects: Array<{
    project: SidebarProjectGroupView;
    host: HostCardView;
    visibleConversations: SidebarConversationView[];
  }>;
  filteredAttentionItems: SidebarAttentionItem[];
  filteredPinnedEntries: SidebarPinnedEntry[];
  projectRenderModels: SidebarProjectRenderModel[];
  visibleNavItems: SidebarVisibleNavItem[];
}

export interface BuildSidebarModelInput {
  hosts: readonly HostCardView[];
  sidebarView: FleetSidebarView | null;
  hostDetails: Record<string, HostDetailView>;
  archivedThreads: Record<string, ArchivedThreadEntry>;
  hiddenProjectKeys: Record<string, boolean>;
  threadCustomTitles: Record<string, string>;
  collapsedProjectIds: Record<string, boolean>;
  collapsedWorkspaceIds: Record<string, boolean>;
  sidebarProjectSort: SidebarProjectSort;
  threadManualOrder: Record<string, string[]>;
  workspacePanel: WorkspacePanelMode;
  isDraftNewSession: boolean;
  selectedHostId: string | null;
  selectedAgentId: string | null;
  selectedWorkspaceCwd: string | null;
  expandedMoreProjects: Record<string, boolean>;
  filterQuery: string;
}

export function buildSidebarDisplayModel(input: BuildSidebarModelInput): SidebarDisplayModel {
  const {
    hosts,
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
  } = input;

  const hostById = new Map(hosts.map((h) => [h.host_id, h]));
  const rawProjects = sidebarView?.projects ?? [];
  const singleHostMode = hosts.length <= 1;
  const trimmedFilter = filterQuery.trim();
  const isFilteringThreads = trimmedFilter.length > 0;

  // Map agentId -> curated group title so Attention/Pinned search also matches group names
  const groupTitleByThreadKey = new Map<string, string>();
  for (const proj of rawProjects) {
    for (const ws of proj.workspaces ?? []) {
      if (ws.is_curated === false) continue;
      for (const c of ws.conversations) {
        groupTitleByThreadKey.set(threadKey(proj.host_id, c.agent.agent_id), ws.title);
      }
    }
  }

  // 1. Needs Attention items
  const filteredAttentionItems: SidebarAttentionItem[] = [];
  const attentionThreadKeySet = new Set<string>();
  for (const conv of sidebarView?.needs_attention ?? []) {
    if (isSidebarAgentArchived(conv.host_id, conv.agent, archivedThreads)) continue;
    const host = hostById.get(conv.host_id);
    if (!host) continue;
    const tKey = threadKey(conv.host_id, conv.agent.agent_id);
    attentionThreadKeySet.add(tKey);
    const matchedProj = rawProjects.find(
      (p) => p.host_id === conv.host_id && matchesProjectDir(p.directories, conv.agent.cwd),
    );
    const projectName = matchedProj?.name || basenameOfPath(conv.agent.cwd);
    const effectiveTitle = threadCustomTitles[tKey] || conv.agent.title;
    const m = matchSidebarThread({
      trimmedFilter,
      effectiveTitle,
      projectName,
      hostDisplayName: host.display_name,
      hostId: host.host_id,
      provider: conv.agent.provider,
      providerLabel: conv.agent.provider_label,
      branch: conv.agent.worktree_branch ?? matchedProj?.git_branch ?? null,
      handle: conv.agent.handle,
      groupTitle: groupTitleByThreadKey.get(tKey) ?? null,
      singleHostMode,
    });
    if (!m.matched) continue;
    filteredAttentionItems.push({
      key: tKey,
      navId: `attn:${conv.host_id}:${conv.agent.agent_id}`,
      host,
      conv,
      projectName,
      effectiveTitle,
      titleIndices: m.titleIndices,
    });
  }

  // 2. Pinned items (deduplicated against Needs Attention when not actively searching)
  const filteredPinnedEntries: SidebarPinnedEntry[] = [];
  const pinnedThreadKeySet = new Set<string>();
  for (const conv of sidebarView?.pinned_conversations ?? []) {
    if (isSidebarAgentArchived(conv.host_id, conv.agent, archivedThreads)) continue;
    const host = hostById.get(conv.host_id);
    if (!host) continue;
    const tKey = threadKey(conv.host_id, conv.agent.agent_id);
    if (!isFilteringThreads && attentionThreadKeySet.has(tKey)) continue;
    pinnedThreadKeySet.add(tKey);
    const matchedProj = rawProjects.find(
      (p) => p.host_id === conv.host_id && matchesProjectDir(p.directories, conv.agent.cwd),
    );
    const projectName = matchedProj?.name || basenameOfPath(conv.agent.cwd);
    const effectiveTitle = threadCustomTitles[tKey] || conv.agent.title;
    const m = matchSidebarThread({
      trimmedFilter,
      effectiveTitle,
      projectName,
      hostDisplayName: host.display_name,
      hostId: host.host_id,
      provider: conv.agent.provider,
      providerLabel: conv.agent.provider_label,
      branch: conv.agent.worktree_branch ?? matchedProj?.git_branch ?? null,
      handle: conv.agent.handle,
      groupTitle: groupTitleByThreadKey.get(tKey) ?? null,
      singleHostMode,
    });
    if (!m.matched) continue;
    filteredPinnedEntries.push({
      key: tKey,
      navId: `pin:${tKey}`,
      host,
      projectName,
      conv,
      effectiveTitle,
      titleIndices: m.titleIndices,
    });
  }

  const pinnedManualOrder = threadManualOrder[PINNED_MANUAL_ORDER_SCOPE];
  if (pinnedManualOrder && pinnedManualOrder.length > 0 && filteredPinnedEntries.length > 1) {
    const rank = new Map<string, number>();
    pinnedManualOrder.forEach((k, idx) => rank.set(k, idx));
    const orderedPinned = filteredPinnedEntries
      .map((entry, idx) => ({
        entry,
        idx,
        order: rank.get(entry.key) ?? Number.MAX_SAFE_INTEGER,
      }))
      .sort((a, b) => (a.order !== b.order ? a.order - b.order : a.idx - b.idx))
      .map((item) => item.entry);
    filteredPinnedEntries.splice(0, filteredPinnedEntries.length, ...orderedPinned);
  }

  const promotedThreadKeySet = new Set<string>([
    ...attentionThreadKeySet,
    ...pinnedThreadKeySet,
  ]);

  // 3. Display Projects (applying local archive/hide/subagent visibility overlays)
  const displayProjects: SidebarDisplayModel["displayProjects"] = [];
  for (const proj of rawProjects) {
    const host = hostById.get(proj.host_id);
    if (!host) continue;

    const isActiveTarget =
      selectedHostId === proj.host_id &&
      matchesProjectDir(proj.directories, selectedWorkspaceCwd);

    const hasCuratedWorkspaces = (proj.workspaces ?? []).some((w) => w.is_curated !== false);
    const detail = hostDetails[proj.host_id];
    const agentsById = new Map((detail?.agents ?? []).map((a) => [a.agent_id, a]));

    const visibleConversations = proj.conversations.filter((c) => {
      if (c.agent.background) return false;
      if (isSidebarAgentArchived(proj.host_id, c.agent, archivedThreads)) return false;
      if (c.agent.parent_agent_id && !c.agent.parent_host_id) {
        const parent = agentsById.get(c.agent.parent_agent_id);
        if (parent && !isSidebarAgentArchived(proj.host_id, parent, archivedThreads)) {
          return false;
        }
      }
      return true;
    });

    if (
      proj.conversations.length === 0 &&
      !hasCuratedWorkspaces &&
      !proj.pinned_project_id &&
      hiddenProjectKeys[projectScopeKey(proj.host_id, proj.primary_cwd)] &&
      !isActiveTarget
    ) {
      continue;
    }

    const allArchived = proj.conversations.length > 0 && visibleConversations.length === 0;
    if (!proj.pinned_project_id && !hasCuratedWorkspaces && allArchived && !isActiveTarget) {
      continue;
    }

    displayProjects.push({
      project: proj,
      host,
      visibleConversations,
    });
  }

  // 4. Project Render Models (search matching incl. group titles + deduplication + manual order)
  const useManualThreadOrder = sidebarProjectSort === "manual";
  const projectRenderModels: SidebarProjectRenderModel[] = [];

  for (const { project: proj, host, visibleConversations: baseVisible } of displayProjects) {
    const titleIndicesByThreadKey = new Map<string, number[]>();
    const baseVisibleIdSet = new Set(baseVisible.map((c) => c.agent.agent_id));
    const normProjRoot = normalizePath(proj.primary_cwd);
    const projThreadScopeKey = `proj:${proj.host_id}::${normProjRoot}`;

    // Map agentId -> group title inside this project for search matching
    const localGroupTitleByAgentId = new Map<string, string>();
    for (const ws of proj.workspaces ?? []) {
      if (ws.is_curated === false) continue;
      for (const c of ws.conversations) {
        localGroupTitleByAgentId.set(c.agent.agent_id, ws.title);
      }
    }

    const matchedConversations = isFilteringThreads
      ? baseVisible.filter((c) => {
          const tKey = threadKey(proj.host_id, c.agent.agent_id);
          const effectiveTitle = threadCustomTitles[tKey] || c.agent.title;
          const m = matchSidebarThread({
            trimmedFilter,
            effectiveTitle,
            projectName: proj.name,
            hostDisplayName: host.display_name,
            hostId: host.host_id,
            provider: c.agent.provider,
            providerLabel: c.agent.provider_label,
            branch: c.agent.worktree_branch ?? proj.git_branch ?? null,
            handle: c.agent.handle,
            groupTitle: localGroupTitleByAgentId.get(c.agent.agent_id) ?? null,
            singleHostMode,
          });
          if (m.matched && m.titleIndices) {
            titleIndicesByThreadKey.set(tKey, m.titleIndices);
          }
          return m.matched;
        })
      : baseVisible;

    const matchedIdSet = new Set(matchedConversations.map((c) => c.agent.agent_id));
    const curatedWorkspaces: SidebarWorkspaceRenderModel[] = (proj.workspaces ?? [])
      .filter((ws) => ws.is_curated !== false)
      .map((ws) => {
        const wsScopeKey = `ws:${proj.host_id}::${ws.workspace_id}`;
        const wsTitleMatched = isFilteringThreads && Boolean(fuzzyMatch(trimmedFilter, ws.title));
        const activePeersInGroup = ws.conversations.filter((c) =>
          baseVisibleIdSet.has(c.agent.agent_id),
        );
        const filteredPeers = activePeersInGroup.filter((c) => {
          if (isFilteringThreads) {
            return wsTitleMatched || matchedIdSet.has(c.agent.agent_id);
          }
          // Deduplicate promoted (Attention / Pinned) threads from below
          return !promotedThreadKeySet.has(threadKey(proj.host_id, c.agent.agent_id));
        });
        const visiblePeers = useManualThreadOrder
          ? applyManualThreadOrder(filteredPeers, threadManualOrder[wsScopeKey])
          : filteredPeers;
        return {
          ...ws,
          threadScopeKey: wsScopeKey,
          visiblePeers,
          hasActivePeersInGroup: activePeersInGroup.length > 0,
          wsCollapsed: isFilteringThreads ? false : Boolean(collapsedWorkspaceIds[ws.workspace_id]),
        };
      })
      .filter((ws) => {
        if (!isFilteringThreads) return true;
        return ws.visiblePeers.length > 0 || Boolean(fuzzyMatch(trimmedFilter, ws.title));
      });

    const curatedAgentIdSet = new Set<string>();
    for (const ws of proj.workspaces ?? []) {
      if (ws.is_curated === false) continue;
      for (const c of ws.conversations) {
        curatedAgentIdSet.add(c.agent.agent_id);
      }
    }

    const rawUnclassifiedBase =
      proj.unclassified_conversations !== undefined
        ? proj.unclassified_conversations.filter((c) => matchedIdSet.has(c.agent.agent_id))
        : matchedConversations.filter((c) => !curatedAgentIdSet.has(c.agent.agent_id));

    const rawUnclassified = !isFilteringThreads
      ? rawUnclassifiedBase.filter(
          (c) => !promotedThreadKeySet.has(threadKey(proj.host_id, c.agent.agent_id)),
        )
      : rawUnclassifiedBase;

    const unclassifiedConversations = useManualThreadOrder
      ? applyManualThreadOrder(rawUnclassified, threadManualOrder[projThreadScopeKey])
      : rawUnclassified;

    if (
      isFilteringThreads &&
      curatedWorkspaces.length === 0 &&
      unclassifiedConversations.length === 0
    ) {
      continue;
    }

    const collapsed = isFilteringThreads ? false : Boolean(collapsedProjectIds[proj.id]);
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
    const hiddenCount = Math.max(0, totalUnclassified - MAX_VISIBLE_CONVERSATIONS_PER_PROJECT);
    const slicedUnclassified = isExpandedAll
      ? unclassifiedConversations
      : unclassifiedConversations.slice(0, MAX_VISIBLE_CONVERSATIONS_PER_PROJECT);

    projectRenderModels.push({
      project: proj,
      host,
      visibleConversations: matchedConversations,
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

  // 5. Flat top-to-bottom visible navigation list (shared by Filter arrow nav & ⌘1..9!)
  const visibleNavItems: SidebarVisibleNavItem[] = [];
  for (const item of filteredAttentionItems) {
    visibleNavItems.push({
      navId: item.navId,
      hostId: item.host.host_id,
      agentId: item.conv.agent.agent_id,
    });
  }
  for (const entry of filteredPinnedEntries) {
    visibleNavItems.push({
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
        visibleNavItems.push({
          navId: `ws:${ws.workspace_id}:${model.project.host_id}:${conv.agent.agent_id}`,
          hostId: model.project.host_id,
          agentId: conv.agent.agent_id,
        });
      }
    }
    for (const conv of model.slicedUnclassified) {
      visibleNavItems.push({
        navId: `proj:${model.project.id}:${model.project.host_id}:${conv.agent.agent_id}`,
        hostId: model.project.host_id,
        agentId: conv.agent.agent_id,
      });
    }
  }

  return {
    displayProjects,
    filteredAttentionItems,
    filteredPinnedEntries,
    projectRenderModels,
    visibleNavItems,
  };
}

/**
 * Pure helper used by global `⌘1..9` thread switching (`fleet-slice.ts`) so keyboard
 * indices match the exact top-to-bottom order shown in `FleetSidebar`.
 */
export function collectVisibleSidebarThreads(
  input: Omit<BuildSidebarModelInput, "expandedMoreProjects" | "filterQuery">,
): Array<{ hostId: string; agentId: string }> {
  const model = buildSidebarDisplayModel({
    ...input,
    expandedMoreProjects: {},
    filterQuery: "",
  });
  return model.visibleNavItems.map(({ hostId, agentId }) => ({ hostId, agentId }));
}

export function isSidebarConversationUnread(
  conv: SidebarConversationView,
  opts: {
    workspacePanel: WorkspacePanelMode;
    isDraftNewSession: boolean;
    selectedHostId: string | null;
    selectedAgentId: string | null;
  },
): boolean {
  const isSelected =
    opts.workspacePanel === "conversation" &&
    !opts.isDraftNewSession &&
    opts.selectedHostId === conv.host_id &&
    opts.selectedAgentId === conv.agent.agent_id;
  if (isSelected || isForegroundRunning(conv.agent)) return false;
  if (conv.agent.attention && conv.agent.attention !== "none") return true;
  return conv.is_unread;
}
