import {
  AgentCardDto,
  ClientProjectDto,
  DiscoveredProjectDto,
  HostDetailDto,
  basenameOfPath,
  cwdPathsEquivalent,
  parseRfc3339Ms
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';

export const MAX_VISIBLE_THREADS_PER_PROJECT: number = 5;
const FALLBACK_UNPINNED_DISCOVERED_LIMIT: number = 4;

export interface SidebarProjectGroup {
  projectId: string;
  pinnedProjectId: string;
  name: string;
  primaryDir: string;
  branch: string;
  isHomeWorkspace: boolean;
  lastActivityMs: number;
  threads: AgentCardDto[];
}

export function canonicalPathKey(path: string): string {
  let trimmed = path.trim();
  if (trimmed.length === 0 || trimmed === '~') {
    return '~';
  }
  if (trimmed === '/') {
    return '/';
  }
  while (trimmed.length > 1 && trimmed.endsWith('/')) {
    trimmed = trimmed.substring(0, trimmed.length - 1);
  }
  return trimmed;
}

export function isHomePath(path: string): boolean {
  const norm = canonicalPathKey(path);
  if (norm === '~' || norm === '/root') {
    return true;
  }
  if (norm.startsWith('/Users/')) {
    const rest = norm.substring('/Users/'.length);
    return rest.length > 0 && rest.indexOf('/') < 0;
  }
  if (norm.startsWith('/home/')) {
    const rest = norm.substring('/home/'.length);
    return rest.length > 0 && rest.indexOf('/') < 0;
  }
  return false;
}

export function resolveWorktreeBasePath(rawPath: string, knownRepoBases: string[]): string {
  const clean = canonicalPathKey(rawPath);
  if (clean === '~' || clean === '/') {
    return clean;
  }
  for (let i = 0; i < knownRepoBases.length; i++) {
    const base = knownRepoBases[i];
    if (clean === base || clean.startsWith(`${base}/`)) {
      return base;
    }
  }
  const worktreeIdx = clean.indexOf('/worktrees/');
  if (worktreeIdx >= 0) {
    const after = clean.substring(worktreeIdx + '/worktrees/'.length);
    const slashIdx = after.indexOf('/');
    const seg = slashIdx >= 0 ? after.substring(0, slashIdx) : after;
    if (seg.length > 0) {
      for (let i = 0; i < knownRepoBases.length; i++) {
        const base = knownRepoBases[i];
        const repoName = basenameOfPath(base);
        if (seg === repoName || seg.startsWith(`${repoName}-`)) {
          return base;
        }
      }
    }
  }
  return clean;
}

export function compareAgentsRecentDesc(a: AgentCardDto, b: AgentCardDto): number {
  const msDiff = parseRfc3339Ms(b.updated_at_rfc3339) - parseRfc3339Ms(a.updated_at_rfc3339);
  if (msDiff !== 0) {
    return msDiff;
  }
  if (b.updated_at_rfc3339 > a.updated_at_rfc3339) {
    return 1;
  }
  if (b.updated_at_rfc3339 < a.updated_at_rfc3339) {
    return -1;
  }
  return 0;
}

export function matchesAgentSearch(agent: AgentCardDto, q: string): boolean {
  if (q.length === 0) {
    return true;
  }
  const title = (agent.title.trim().length > 0 ? agent.title : agent.provider_label).toLowerCase();
  const cwd = agent.cwd.toLowerCase();
  const base = basenameOfPath(agent.cwd).toLowerCase();
  const provider = agent.provider.toLowerCase();
  const providerLabel = agent.provider_label.toLowerCase();
  return (
    title.indexOf(q) >= 0 ||
    cwd.indexOf(q) >= 0 ||
    base.indexOf(q) >= 0 ||
    provider.indexOf(q) >= 0 ||
    providerLabel.indexOf(q) >= 0
  );
}

export function matchesSidebarThreadFilter(
  store: OrbitHarmonyStore,
  hostId: string,
  agent: AgentCardDto
): boolean {
  if (store.sidebarThreadFilter === 'all') {
    return true;
  }
  return (
    agent.is_awaiting_permission ||
    agent.is_running ||
    store.isThreadUnread(hostId, agent)
  );
}

export function buildSidebarNeedsAttention(
  detail: HostDetailDto | null,
  normalizedQuery: string
): AgentCardDto[] {
  if (normalizedQuery.length > 0 || detail === null) {
    return [];
  }
  const out: AgentCardDto[] = [];
  for (let i = 0; i < detail.agents.length; i++) {
    if (detail.agents[i].is_awaiting_permission) {
      out.push(detail.agents[i]);
    }
  }
  out.sort(compareAgentsRecentDesc);
  return out;
}

export function buildSidebarPinnedAgents(
  store: OrbitHarmonyStore,
  detail: HostDetailDto | null,
  hostId: string,
  normalizedQuery: string
): AgentCardDto[] {
  if (normalizedQuery.length > 0 || hostId.length === 0 || detail === null) {
    return [];
  }
  const out: AgentCardDto[] = [];
  for (let i = 0; i < detail.agents.length; i++) {
    const a = detail.agents[i];
    if (store.isThreadPinned(hostId, a.agent_id) && matchesSidebarThreadFilter(store, hostId, a)) {
      out.push(a);
    }
  }
  out.sort(compareAgentsRecentDesc);
  return out;
}

export function buildSidebarUnifiedProjects(
  store: OrbitHarmonyStore,
  detail: HostDetailDto | null,
  hostId: string,
  normalizedQuery: string
): SidebarProjectGroup[] {
  if (detail === null || hostId.length === 0) {
    return [];
  }

  const pinnedProjects: ClientProjectDto[] = [];
  for (let i = 0; i < store.projects.length; i++) {
    if (store.projects[i].host_id === hostId) {
      pinnedProjects.push(store.projects[i]);
    }
  }

  const discovered = detail.discovered_projects;
  const workspaces = detail.workspaces;
  const agents = detail.agents;

  const nonWorktreeSet: string[] = [];
  for (let i = 0; i < pinnedProjects.length; i++) {
    const dirs = pinnedProjects[i].directories;
    for (let j = 0; j < dirs.length; j++) {
      const key = canonicalPathKey(dirs[j]);
      if (!isHomePath(key) && key.indexOf('/worktrees/') < 0 && nonWorktreeSet.indexOf(key) < 0) {
        nonWorktreeSet.push(key);
      }
    }
  }
  for (let i = 0; i < discovered.length; i++) {
    const key = canonicalPathKey(discovered[i].path);
    if (!isHomePath(key) && key.indexOf('/worktrees/') < 0 && nonWorktreeSet.indexOf(key) < 0) {
      nonWorktreeSet.push(key);
    }
  }
  nonWorktreeSet.sort((a: string, b: string) => b.length - a.length);

  const dirToPinnedProjectId: Map<string, string> = new Map<string, string>();
  for (let i = 0; i < pinnedProjects.length; i++) {
    const proj = pinnedProjects[i];
    for (let j = 0; j < proj.directories.length; j++) {
      dirToPinnedProjectId.set(canonicalPathKey(proj.directories[j]), proj.project_id);
    }
  }

  const threadsByPinnedId: Map<string, AgentCardDto[]> = new Map<string, AgentCardDto[]>();
  const threadsByBasePath: Map<string, AgentCardDto[]> = new Map<string, AgentCardDto[]>();

  for (let i = 0; i < agents.length; i++) {
    const agent = agents[i];
    const base = resolveWorktreeBasePath(agent.cwd, nonWorktreeSet);
    const pinnedId =
      dirToPinnedProjectId.get(canonicalPathKey(agent.cwd)) ??
      dirToPinnedProjectId.get(base);
    if (pinnedId !== undefined) {
      const list = threadsByPinnedId.get(pinnedId) ?? [];
      list.push(agent);
      threadsByPinnedId.set(pinnedId, list);
    } else {
      const list = threadsByBasePath.get(base) ?? [];
      list.push(agent);
      threadsByBasePath.set(base, list);
    }
  }

  const rawGroups: SidebarProjectGroup[] = [];
  const coveredBases: string[] = [];

  for (let i = 0; i < pinnedProjects.length; i++) {
    const proj = pinnedProjects[i];
    for (let j = 0; j < proj.directories.length; j++) {
      const key = canonicalPathKey(proj.directories[j]);
      if (coveredBases.indexOf(key) < 0) {
        coveredBases.push(key);
      }
    }
    const primary = proj.directories.length > 0 ? proj.directories[0] : '~';
    const groupAgents = threadsByPinnedId.get(proj.project_id) ?? [];
    let discTs = 0;
    for (let k = 0; k < discovered.length; k++) {
      for (let j = 0; j < proj.directories.length; j++) {
        if (cwdPathsEquivalent(proj.directories[j], discovered[k].path)) {
          discTs = Math.max(discTs, discovered[k].last_modified_unix * 1000);
        }
      }
    }
    let maxAgentTs = 0;
    for (let k = 0; k < groupAgents.length; k++) {
      maxAgentTs = Math.max(maxAgentTs, parseRfc3339Ms(groupAgents[k].updated_at_rfc3339));
    }
    rawGroups.push({
      projectId: proj.project_id,
      pinnedProjectId: proj.project_id,
      name: proj.name,
      primaryDir: primary,
      branch: store.resolveWorkspaceBranch(primary),
      isHomeWorkspace: isHomePath(primary),
      lastActivityMs: Math.max(maxAgentTs, discTs, proj.updated_at_ms),
      threads: groupAgents
    });
  }

  const discByBase: Map<string, DiscoveredProjectDto> = new Map<string, DiscoveredProjectDto>();
  for (let i = 0; i < discovered.length; i++) {
    const base = resolveWorktreeBasePath(discovered[i].path, nonWorktreeSet);
    if (!discByBase.has(base)) {
      discByBase.set(base, discovered[i]);
    }
  }

  const candidateBases: string[] = [];
  threadsByBasePath.forEach((_val: AgentCardDto[], key: string) => {
    if (coveredBases.indexOf(key) < 0 && candidateBases.indexOf(key) < 0) {
      candidateBases.push(key);
    }
  });
  for (let i = 0; i < workspaces.length; i++) {
    const base = resolveWorktreeBasePath(workspaces[i].cwd, nonWorktreeSet);
    if (coveredBases.indexOf(base) < 0 && !isHomePath(base) && candidateBases.indexOf(base) < 0) {
      candidateBases.push(base);
    }
  }
  if (store.activeCwd.length > 0 && !isHomePath(store.activeCwd)) {
    const activeBase = resolveWorktreeBasePath(store.activeCwd, nonWorktreeSet);
    if (coveredBases.indexOf(activeBase) < 0 && candidateBases.indexOf(activeBase) < 0) {
      candidateBases.push(activeBase);
    }
  }

  const fallbackDiscovered: DiscoveredProjectDto[] = [];
  for (let i = 0; i < discovered.length; i++) {
    const base = resolveWorktreeBasePath(discovered[i].path, nonWorktreeSet);
    if (!isHomePath(base) && coveredBases.indexOf(base) < 0) {
      fallbackDiscovered.push(discovered[i]);
    }
  }
  fallbackDiscovered.sort(
    (a: DiscoveredProjectDto, b: DiscoveredProjectDto) => b.last_modified_unix - a.last_modified_unix
  );
  let addedFallbacks = 0;
  for (let i = 0; i < fallbackDiscovered.length && addedFallbacks < FALLBACK_UNPINNED_DISCOVERED_LIMIT; i++) {
    const base = resolveWorktreeBasePath(fallbackDiscovered[i].path, nonWorktreeSet);
    if (candidateBases.indexOf(base) < 0) {
      candidateBases.push(base);
      addedFallbacks++;
    }
  }

  for (let i = 0; i < candidateBases.length; i++) {
    const base = candidateBases[i];
    const groupAgents = threadsByBasePath.get(base) ?? [];
    const isHome = isHomePath(base);
    if (isHome && groupAgents.length === 0) {
      continue;
    }
    const disc = discByBase.get(base);
    let wsTitle = '';
    for (let w = 0; w < workspaces.length; w++) {
      if (cwdPathsEquivalent(workspaces[w].cwd, base) && workspaces[w].title.trim().length > 0) {
        wsTitle = workspaces[w].title;
        break;
      }
    }
    const displayName = isHome
      ? 'Workspace (~)'
      : (disc !== undefined && disc.name.trim().length > 0)
        ? disc.name
        : wsTitle.length > 0
          ? wsTitle
          : basenameOfPath(base);
    let maxAgentTs = 0;
    for (let k = 0; k < groupAgents.length; k++) {
      maxAgentTs = Math.max(maxAgentTs, parseRfc3339Ms(groupAgents[k].updated_at_rfc3339));
    }
    const discTs = disc !== undefined ? disc.last_modified_unix * 1000 : 0;
    rawGroups.push({
      projectId: `dir::${base}`,
      pinnedProjectId: '',
      name: displayName,
      primaryDir: base,
      branch: store.resolveWorkspaceBranch(base),
      isHomeWorkspace: isHome,
      lastActivityMs: Math.max(maxAgentTs, discTs),
      threads: groupAgents
    });
  }

  const filtered: SidebarProjectGroup[] = [];
  for (let i = 0; i < rawGroups.length; i++) {
    const group = rawGroups[i];
    const filterPass: AgentCardDto[] = [];
    for (let j = 0; j < group.threads.length; j++) {
      if (matchesSidebarThreadFilter(store, hostId, group.threads[j])) {
        filterPass.push(group.threads[j]);
      }
    }
    filterPass.sort(compareAgentsRecentDesc);

    let visibleThreads = filterPass;
    if (normalizedQuery.length > 0) {
      const projMatch =
        group.name.toLowerCase().indexOf(normalizedQuery) >= 0 ||
        group.primaryDir.toLowerCase().indexOf(normalizedQuery) >= 0 ||
        group.branch.toLowerCase().indexOf(normalizedQuery) >= 0;
      const threadMatches: AgentCardDto[] = [];
      for (let j = 0; j < filterPass.length; j++) {
        if (matchesAgentSearch(filterPass[j], normalizedQuery)) {
          threadMatches.push(filterPass[j]);
        }
      }
      if (threadMatches.length > 0) {
        visibleThreads = threadMatches;
      } else if (projMatch) {
        visibleThreads = filterPass;
      } else {
        continue;
      }
    }

    if (store.sidebarThreadFilter === 'active_unread' && visibleThreads.length === 0) {
      continue;
    }

    let latestMs = group.lastActivityMs;
    for (let j = 0; j < visibleThreads.length; j++) {
      latestMs = Math.max(latestMs, parseRfc3339Ms(visibleThreads[j].updated_at_rfc3339));
    }
    filtered.push({
      projectId: group.projectId,
      pinnedProjectId: group.pinnedProjectId,
      name: group.name,
      primaryDir: group.primaryDir,
      branch: group.branch,
      isHomeWorkspace: group.isHomeWorkspace,
      lastActivityMs: latestMs,
      threads: visibleThreads
    });
  }

  if (store.sidebarProjectSort === 'name') {
    filtered.sort((a: SidebarProjectGroup, b: SidebarProjectGroup) => {
      if (a.isHomeWorkspace !== b.isHomeWorkspace) {
        return a.isHomeWorkspace ? 1 : -1;
      }
      return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
    });
  } else {
    filtered.sort((a: SidebarProjectGroup, b: SidebarProjectGroup) => {
      if (a.isHomeWorkspace !== b.isHomeWorkspace) {
        return a.isHomeWorkspace ? 1 : -1;
      }
      const aEmpty = a.threads.length === 0;
      const bEmpty = b.threads.length === 0;
      if (aEmpty !== bEmpty) {
        return aEmpty ? 1 : -1;
      }
      if (b.lastActivityMs !== a.lastActivityMs) {
        return b.lastActivityMs - a.lastActivityMs;
      }
      return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
    });
  }
  return filtered;
}
