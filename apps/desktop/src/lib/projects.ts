import type {
  AgentCardView,
  ClientProjectView,
  HostCardView,
  HostDetailView,
} from "@/lib/orbit-ipc";

export function basenameOfProjectPath(path: string): string {
  if (!path || path === "~") return "~";
  const clean = path.replace(/\/+$/, "");
  const i = clean.lastIndexOf("/");
  return i >= 0 ? clean.slice(i + 1) || "/" : clean;
}

export function normalizeProjectPath(path: string): string {
  if (!path || path === "~") return "~";
  return path.replace(/\/+$/, "") || "/";
}

export function projectPathScopeKey(hostId: string, dirPath: string): string {
  return `${hostId}::${normalizeProjectPath(dirPath)}`;
}

export function isBareHomeDir(path: string): boolean {
  const clean = normalizeProjectPath(path);
  if (clean === "~" || clean === "/") return true;
  const parts = clean.split("/");
  return parts.length === 3 && (parts[1] === "Users" || parts[1] === "home");
}

export function matchesProjectDirectory(directories: string[], cwd: string): boolean {
  const cleanCwd = normalizeProjectPath(cwd);
  for (const dir of directories) {
    const cleanDir = normalizeProjectPath(dir);
    if (cleanDir === cleanCwd) return true;
    const repoBase = basenameOfProjectPath(cleanDir);
    if (repoBase && repoBase !== "~" && cleanCwd.includes(`/worktrees/${repoBase}-`)) {
      return true;
    }
  }
  return false;
}

/**
 * Determine whether a paired `HostCardView` represents the local machine running this desktop app,
 * where native OS folder pickers (`NSOpenPanel` / `zenity` / `FolderBrowserDialog`) apply directly.
 */
export function isLocalHostCard(host: HostCardView): boolean {
  if (
    host.via_ssh_tunnel ||
    Boolean(host.ssh_alias) ||
    host.route_mode === "ssh_tunnel" ||
    host.route_mode === "relay_only" ||
    host.connection_state === "online_relay"
  ) {
    return false;
  }
  return host.direct_endpoints.some(
    (ep) => ep.includes("127.0.0.1") || ep.includes("localhost") || ep.includes("[::1]"),
  );
}

export interface ProjectConversationEntry {
  agent: AgentCardView;
  isWorktree: boolean;
}

export interface UnifiedProjectEntry {
  id: string;
  name: string;
  primaryCwd: string;
  directories: string[];
  gitBranch: string | null;
  host: HostCardView;
  pinnedProject: ClientProjectView | null;
  conversations: ProjectConversationEntry[];
  sortWeight: number;
}

function resolveGitBranchForPath(
  detail: HostDetailView | undefined,
  rawPath: string,
): string | null {
  if (!detail) return null;
  const clean = normalizeProjectPath(rawPath);
  const ws = detail.workspaces.find(
    (w) => !w.is_worktree && normalizeProjectPath(w.cwd) === clean,
  );
  if (ws?.git_branch) return ws.git_branch;
  const dp = detail.discovered_projects.find(
    (p) => normalizeProjectPath(p.path) === clean,
  );
  return dp?.git_branch ?? null;
}

export function buildUnifiedProjects(
  hosts: HostCardView[],
  pinnedProjects: ClientProjectView[],
  hostDetails: Record<string, HostDetailView>,
  hiddenProjectKeys?: Record<string, boolean>,
): UnifiedProjectEntry[] {
  const hostMap = new Map(hosts.map((h) => [h.host_id, h]));
  const entries: UnifiedProjectEntry[] = [];
  const entriesByKey = new Map<string, UnifiedProjectEntry>();

  // 1. Pinned ClientProjectView entries from Orbit store
  for (const proj of pinnedProjects) {
    const host = hostMap.get(proj.host_id);
    if (!host) continue;
    const detail = hostDetails[host.host_id];
    const primary = normalizeProjectPath(proj.directories[0] ?? "~");
    const entry: UnifiedProjectEntry = {
      id: proj.project_id,
      name: proj.name || basenameOfProjectPath(primary),
      primaryCwd: primary,
      directories:
        proj.directories.length > 0
          ? proj.directories.map(normalizeProjectPath)
          : [primary],
      gitBranch: resolveGitBranchForPath(detail, primary),
      host,
      pinnedProject: proj,
      conversations: [],
      sortWeight: proj.updated_at_ms || 1,
    };
    entries.push(entry);
    entriesByKey.set(entry.id, entry);
  }

  // 2. Ensure every active conversation belongs to a formal Project
  for (const host of hosts) {
    const detail = hostDetails[host.host_id];
    if (!detail) continue;

    const wsWorktreeMap = new Map<string, boolean>();
    for (const ws of detail.workspaces) {
      wsWorktreeMap.set(normalizeProjectPath(ws.cwd), ws.is_worktree);
    }

    for (const ag of detail.agents) {
      const cleanCwd = normalizeProjectPath(ag.cwd);
      const isWt = wsWorktreeMap.get(cleanCwd) ?? cleanCwd.includes("/worktrees/");

      let matched = entries.find(
        (e) =>
          e.host.host_id === host.host_id && matchesProjectDirectory(e.directories, cleanCwd),
      );

      // Match worktree sessions back to their base repository
      if (!matched && isWt) {
        const candidateRepo =
          detail.workspaces.find(
            (w) => !w.is_worktree && matchesProjectDirectory([w.cwd], cleanCwd),
          )?.cwd ??
          detail.discovered_projects.find((dp) => matchesProjectDirectory([dp.path], cleanCwd))
            ?.path ??
          null;

        if (candidateRepo) {
          const cleanRepo = normalizeProjectPath(candidateRepo);
          const id = `${host.host_id}::${cleanRepo}`;
          matched = entriesByKey.get(id);
          if (!matched) {
            matched = {
              id,
              name: basenameOfProjectPath(cleanRepo),
              primaryCwd: cleanRepo,
              directories: [cleanRepo],
              gitBranch: resolveGitBranchForPath(detail, cleanRepo),
              host,
              pinnedProject: null,
              conversations: [],
              sortWeight: 0,
            };
            entries.push(matched);
            entriesByKey.set(id, matched);
          }
        }
      }

      if (!matched) {
        const id = `${host.host_id}::${cleanCwd}`;
        matched = entriesByKey.get(id);
        if (!matched) {
          matched = {
            id,
            name: basenameOfProjectPath(cleanCwd),
            primaryCwd: cleanCwd,
            directories: [cleanCwd],
            gitBranch: resolveGitBranchForPath(detail, cleanCwd),
            host,
            pinnedProject: null,
            conversations: [],
            sortWeight: 0,
          };
          entries.push(matched);
          entriesByKey.set(id, matched);
        }
      }

      matched.conversations.push({ agent: ag, isWorktree: isWt });
      const ts = Date.parse(ag.updated_at_rfc3339);
      if (!Number.isNaN(ts) && ts > matched.sortWeight) {
        matched.sortWeight = ts;
      }
    }

    // 3. When a host has neither pinned projects nor active sessions yet, surface top discovered repos
    const hostHasEntries = entries.some((e) => e.host.host_id === host.host_id);
    if (!hostHasEntries) {
      for (const dp of detail.discovered_projects.slice(0, 4)) {
        const cleanPath = normalizeProjectPath(dp.path);
        if (isBareHomeDir(cleanPath) && detail.discovered_projects.length > 1) continue;
        if (hiddenProjectKeys?.[projectPathScopeKey(host.host_id, cleanPath)]) continue;
        const id = `${host.host_id}::${cleanPath}`;
        if (entriesByKey.has(id)) continue;
        const entry: UnifiedProjectEntry = {
          id,
          name: dp.name || basenameOfProjectPath(cleanPath),
          primaryCwd: cleanPath,
          directories: [cleanPath],
          gitBranch: dp.git_branch,
          host,
          pinnedProject: null,
          conversations: [],
          sortWeight: dp.last_modified_unix * 1000,
        };
        entries.push(entry);
        entriesByKey.set(id, entry);
      }
    }
  }

  entries.sort((a, b) => {
    const aHome = isBareHomeDir(a.primaryCwd) ? 1 : 0;
    const bHome = isBareHomeDir(b.primaryCwd) ? 1 : 0;
    if (aHome !== bHome) return aHome - bHome;
    if (a.conversations.length > 0 && b.conversations.length === 0) return -1;
    if (a.conversations.length === 0 && b.conversations.length > 0) return 1;
    return b.sortWeight - a.sortWeight;
  });

  return entries;
}
