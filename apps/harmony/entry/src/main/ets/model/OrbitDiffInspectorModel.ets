import { DiffFileDto } from './OrbitTypes';

export interface OrbitDiffInspectorScope {
  readonly hostId: string;
  readonly cwd: string;
  readonly isPinned: boolean;
}

function isAsciiLetterCode(ch: number): boolean {
  return (ch >= 65 && ch <= 90) || (ch >= 97 && ch <= 122);
}

function isWindowsUncPath(raw: string): boolean {
  if (raw.length < 3) {
    return false;
  }
  const hasUncPrefix = raw.startsWith('\\\\') || raw.startsWith('//');
  if (!hasUncPrefix) {
    return false;
  }
  const third = raw.charAt(2);
  return third !== '/' && third !== '\\';
}

function isWindowsDrivePath(raw: string): boolean {
  if (raw.length < 2 || raw.charAt(1) !== ':') {
    return false;
  }
  if (!isAsciiLetterCode(raw.charCodeAt(0))) {
    return false;
  }
  return raw.length === 2 || raw.charAt(2) === '\\' || raw.charAt(2) === '/';
}

function normalizeSingleGitPath(rawPath: string): string {
  if (rawPath.length === 0) {
    return '';
  }
  if (isWindowsUncPath(rawPath)) {
    const slashed = rawPath.replace(/\\/g, '/');
    let rest = slashed.substring(2).replace(/\/{2,}/g, '/');
    while (rest.length > 0 && rest.endsWith('/')) {
      rest = rest.substring(0, rest.length - 1);
    }
    return `//${rest}`;
  }
  if (isWindowsDrivePath(rawPath)) {
    const slashed = rawPath.replace(/\\/g, '/');
    const drivePrefix = slashed.substring(0, 2);
    if (slashed.length === 2) {
      // Drive-relative `C:` MUST stay distinct from drive-root `C:/`.
      return drivePrefix;
    }
    let rest = slashed.substring(2).replace(/\/{2,}/g, '/');
    if (rest === '/') {
      return `${drivePrefix}/`;
    }
    while (rest.length > 1 && rest.endsWith('/')) {
      rest = rest.substring(0, rest.length - 1);
    }
    return `${drivePrefix}${rest}`;
  }
  // POSIX path: preserve significant spaces and literal backslashes verbatim.
  let clean = rawPath.replace(/\/{2,}/g, '/');
  while (clean.length > 1 && clean.endsWith('/')) {
    clean = clean.substring(0, clean.length - 1);
  }
  return clean;
}

/**
 * Normalizes a filesystem path for Git inspector target comparison WITHOUT ever stripping
 * `/home/<user>`, `/Users/<user>`, `/root` prefixes, or significant leading/trailing spaces
 * in POSIX directory names (e.g., `/tmp/project` vs `/tmp/project ` remain distinct).
 *
 * - Converts `\` to `/` ONLY for explicit Windows drive/UNC paths; POSIX literal backslashes
 *   (e.g., `/tmp/team\project` vs `/tmp/team/project`) are preserved verbatim.
 * - Normalizes Windows UNC paths (`\\server\share\p` and `//server/share/p` -> `//server/share/p`)
 *   idempotently and keeps Windows drive roots (`C:\` / `C:/` -> `C:/`) distinct from `C:`.
 * - Expands a leading `~` ONLY when that host's authoritative `hostHomeDir` is explicitly provided.
 */
export function normalizeGitTargetPath(rawPath: string, hostHomeDir: string = ''): string {
  if (rawPath.length === 0) {
    return '';
  }
  let clean = normalizeSingleGitPath(rawPath);
  if (hostHomeDir.length > 0 && hostHomeDir !== '~') {
    const cleanHome = normalizeSingleGitPath(hostHomeDir);
    if (cleanHome.length > 0 && cleanHome !== '~') {
      if (clean === '~') {
        clean = cleanHome;
      } else if (clean.startsWith('~/')) {
        const joined = cleanHome === '/'
          ? `/${clean.substring(2)}`
          : `${cleanHome}/${clean.substring(2)}`;
        clean = normalizeSingleGitPath(joined);
      }
    }
  }
  return clean;
}

/**
 * Strict full-path equality check for Git Diff & collaboration targets.
 * Never strips user home prefixes (`/home/<user>` or `/Users/<user>`).
 */
export function gitTargetPathsEqual(
  pathA: string,
  pathB: string,
  hostHomeDir: string = ''
): boolean {
  const normA = normalizeGitTargetPath(pathA, hostHomeDir);
  const normB = normalizeGitTargetPath(pathB, hostHomeDir);
  if (normA.length === 0 || normB.length === 0) {
    return false;
  }
  return normA === normB;
}

/**
 * Selects the displayed diff file list for `GitDiffSheet`:
 * - When the inspector is pinned to a specific decision worker `(hostId, cwd)`, ALWAYS returns
 *   that target's own `gitDiffFiles` (even when empty) and NEVER falls back to the active PA
 *   conversation's `sessionDiffs`.
 * - When unpinned (following the active conversation), falls back to `sessionDiffs` when working
 *   tree diff is empty.
 */
export function selectDisplayedDiffFiles(
  isDiffTargetPinned: boolean,
  gitDiffFiles: DiffFileDto[],
  sessionDiffs: DiffFileDto[]
): DiffFileDto[] {
  if (isDiffTargetPinned) {
    return gitDiffFiles;
  }
  if (gitDiffFiles.length > 0) {
    return gitDiffFiles;
  }
  return sessionDiffs;
}

/**
 * Validates that an asynchronous Git collaboration response (PR status, stashes, conflicts, rebase)
 * still matches the inspector's current `(requestSeq, hostId, cwd)` before committing to store state,
 * preventing late responses from repo A from overwriting an inspector pinned to repo B.
 */
export function isGitCollabResponseCurrent(
  requestSeq: number,
  currentSeq: number,
  requestHostId: string,
  currentHostId: string,
  requestCwd: string,
  currentCwd: string,
  hostHomeDir: string = ''
): boolean {
  if (requestSeq !== currentSeq) {
    return false;
  }
  if (requestHostId.trim().length === 0 || requestHostId.trim() !== currentHostId.trim()) {
    return false;
  }
  return gitTargetPathsEqual(requestCwd, currentCwd, hostHomeDir);
}

/**
 * Guards `gitStatus` / `gitDiffFiles` mutations so that when the Diff inspector is pinned to a
 * specific decision worker `(pinnedHostId, pinnedCwd)`, opening another repository in the File
 * Explorer or changing `activeCwd` NEVER overwrites the pinned inspector's file list — including
 * distinct user home directories on the same host (e.g. `/home/worker/project` vs `/home/pa/project`).
 */
export function resolveDirectoryBrowserCurrentPath(
  listingCurrentPath: string | null | undefined,
  activeCwd: string
): string {
  if (typeof listingCurrentPath === 'string' && listingCurrentPath.trim().length > 0) {
    return listingCurrentPath;
  }
  return activeCwd.trim().length > 0 ? activeCwd : '~';
}

export function resolveDirectoryBrowserParentPath(
  listingParentPath: string | null | undefined
): string | null {
  if (typeof listingParentPath === 'string' && listingParentPath.trim().length > 0) {
    return listingParentPath;
  }
  return null;
}

export function shouldApplyGitDiffForTarget(
  isDiffTargetPinned: boolean,
  pinnedHostId: string,
  pinnedCwd: string,
  incomingHostId: string,
  incomingCwd: string,
  hostHomeDir: string = ''
): boolean {
  if (!isDiffTargetPinned) {
    return true;
  }
  const cleanPinnedHost = pinnedHostId.trim();
  if (cleanPinnedHost.length === 0 || pinnedCwd.length === 0) {
    return false;
  }
  return incomingHostId.trim() === cleanPinnedHost &&
    gitTargetPathsEqual(incomingCwd, pinnedCwd, hostHomeDir);
}
