import type { HostCardView } from "@/lib/orbit-ipc";

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
