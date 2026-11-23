import type {
  AgentCardView,
  FleetSidebarView,
  HostCardView,
  HostDetailView,
} from "@/lib/orbit-ipc";
import { stripWindowsVerbatimPrefix } from "@/lib/stage-cache";

export function basenameOfProjectPath(path: string): string {
  if (!path || path === "~") return "~";
  const clean = normalizeProjectPath(path);
  if (clean === "/" || clean === "\\" || /^[A-Za-z]:[\\/]?$/.test(clean)) {
    return clean;
  }
  const normalized = clean.replace(/\\/g, "/").replace(/\/+$/, "");
  const i = normalized.lastIndexOf("/");
  return i >= 0 ? normalized.slice(i + 1) || "/" : normalized;
}

export function normalizeProjectPath(path: string): string {
  if (!path || path === "~") return "~";
  const clean = stripWindowsVerbatimPrefix(path.trim());
  if (!clean) return "~";
  if (clean === "/" || clean === "\\") return clean;
  if (/^[A-Za-z]:[\\/]+$/.test(clean)) {
    return `${clean.slice(0, 2)}${clean[2]}`;
  }
  return clean.replace(/[\\/]+$/, "") || "/";
}

export function formatTildeProjectPath(path: string): string {
  const normalized = normalizeProjectPath(path);
  if (/^[A-Za-z]:[\\/]Users[\\/][^\\/]+/i.test(normalized)) {
    return normalized.replace(/^[A-Za-z]:[\\/]Users[\\/][^\\/]+/i, "~");
  }
  return normalized.replace(/^\/(?:Users|home)\/[^/]+/, "~");
}

/**
 * Unified git-repo check shared across sidebar context menus and stage fork actions.
 */
export function isAgentInGitRepo(
  hostId: string | null | undefined,
  agent: AgentCardView | null | undefined,
  hostDetail: HostDetailView | null | undefined,
  sidebarView?: FleetSidebarView | null,
): boolean {
  if (!hostId || !agent) return false;
  const activeWs =
    hostDetail?.workspaces.find((w) => w.workspace_id === agent.workspace_id) ?? null;
  if (activeWs?.git_branch || activeWs?.is_worktree) return true;

  if (sidebarView) {
    const proj = sidebarView.projects.find(
      (p) =>
        p.host_id === hostId &&
        p.conversations.some((c) => c.agent.agent_id === agent.agent_id),
    );
    if (proj?.is_git_repo) return true;
  }

  const rawRoot = activeWs?.project_root || activeWs?.source_cwd || activeWs?.cwd || agent.cwd || "";
  if (!rawRoot.trim()) return false;
  const normRoot = normalizeProjectPath(rawRoot);
  return Boolean(
    hostDetail?.discovered_projects.some((p) => {
      if (!p.is_git_repo) return false;
      const normProj = normalizeProjectPath(p.path);
      return (
        normProj === normRoot ||
        (normProj !== "/" && normProj !== "~" && normRoot.startsWith(`${normProj}/`))
      );
    }),
  );
}

export interface FilterableProject {
  name: string;
  path: string;
}

export function isPathProjectQuery(query: string): boolean {
  return query.trim().includes("/");
}

function isSegmentBoundary(originalText: string, index: number): boolean {
  if (index <= 0) return true;
  const prev = originalText[index - 1];
  if (prev === "-" || prev === "_" || prev === "." || prev === "/" || prev === " ") {
    return true;
  }
  const curr = originalText[index];
  return prev >= "a" && prev <= "z" && curr >= "A" && curr <= "Z";
}

function scoreTokenInText(originalText: string, tokenLower: string): number | null {
  if (!tokenLower) return null;
  const lower = originalText.toLowerCase();
  if (lower === tokenLower) return 0;
  if (lower.startsWith(tokenLower)) return 1;

  let searchFrom = 0;
  let firstMatchIdx = -1;
  while (searchFrom <= lower.length - tokenLower.length) {
    const idx = lower.indexOf(tokenLower, searchFrom);
    if (idx === -1) break;
    if (firstMatchIdx === -1) firstMatchIdx = idx;
    if (isSegmentBoundary(originalText, idx)) return 2;
    searchFrom = idx + 1;
  }

  return firstMatchIdx !== -1 ? 3 : null;
}

/**
 * Score how well a project matches a search query (lower is better, `null` means no match).
 *
 * - Default mode (no `/`): strictly matches `project.name` (supports space-separated AND tokens).
 * - Path mode (contains `/`): matches normalized full path and `~`-abbreviated path.
 */
export function scoreProjectMatch(project: FilterableProject, rawQuery: string): number | null {
  const q = rawQuery.trim().toLowerCase();
  if (!q) return 0;

  const tokens = q.split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return 0;

  if (q.includes("/")) {
    const normPath = normalizeProjectPath(project.path);
    const tildePath = formatTildeProjectPath(project.path);
    let worstRank = 0;
    for (const token of tokens) {
      const tildeRank = scoreTokenInText(tildePath, token);
      const normRank = scoreTokenInText(normPath, token);
      const best =
        tildeRank !== null && normRank !== null
          ? Math.min(tildeRank, normRank)
          : (tildeRank ?? normRank);
      if (best === null) return null;
      if (best > worstRank) worstRank = best;
    }
    return worstRank;
  }

  if (tokens.length === 1) {
    return scoreTokenInText(project.name, tokens[0]);
  }

  let worstRank = 0;
  for (const token of tokens) {
    const rank = scoreTokenInText(project.name, token);
    if (rank === null) return null;
    if (rank > worstRank) worstRank = rank;
  }
  return worstRank + 1;
}

/**
 * Filter projects by query and sort by match relevance while preserving original order on ties.
 */
export function filterAndRankProjects<T extends FilterableProject>(
  projects: readonly T[],
  rawQuery: string,
): T[] {
  const q = rawQuery.trim();
  if (!q) return [...projects];

  const scored: Array<{ item: T; rank: number; index: number }> = [];
  for (let i = 0; i < projects.length; i++) {
    const item = projects[i];
    const rank = scoreProjectMatch(item, q);
    if (rank !== null) {
      scored.push({ item, rank, index: i });
    }
  }

  scored.sort((a, b) => a.rank - b.rank || a.index - b.index);
  return scored.map((entry) => entry.item);
}

export function projectPathScopeKey(hostId: string, dirPath: string): string {
  return `${hostId}::${normalizeProjectPath(dirPath)}`;
}

export function isBareHomeDir(path: string): boolean {
  const clean = normalizeProjectPath(path);
  if (clean === "~" || clean === "/" || clean === "\\" || /^[A-Za-z]:[\\/]?$/.test(clean)) {
    return true;
  }
  if (/^[A-Za-z]:[\\/]Users[\\/][^\\/]+$/i.test(clean)) {
    return true;
  }
  const parts = clean.split("/");
  return parts.length === 3 && (parts[1] === "Users" || parts[1] === "home");
}

export function matchesProjectDirectory(directories: string[], cwd: string): boolean {
  const cleanCwd = normalizeProjectPath(cwd);
  const slashCwd = cleanCwd.replace(/\\/g, "/");
  for (const dir of directories) {
    const cleanDir = normalizeProjectPath(dir);
    if (cleanDir === cleanCwd || cleanDir.replace(/\\/g, "/") === slashCwd) return true;
    const repoBase = basenameOfProjectPath(cleanDir);
    if (repoBase && repoBase !== "~" && slashCwd.includes(`/worktrees/${repoBase}-`)) {
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
