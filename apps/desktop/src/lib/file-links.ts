/**
 * File-link parsing, normalization, and side-panel navigation helpers for Orbit Desktop.
 */

export type OrbitLinkTarget =
  | { kind: "anchor"; fragment: string }
  | { kind: "external"; url: string }
  | { kind: "browser"; url: string }
  | {
      kind: "file";
      rawPath: string;
      startLine: number | null;
      endLine: number | null;
      column: number | null;
    };

export interface NormalizedLineRange {
  startLine: number | null;
  endLine: number | null;
}

export interface OpenOrbitLinkOptions {
  hostId?: string;
  cwd?: string;
  baseFilePath?: string;
  onAnchor?: (fragment: string) => void;
}

export interface OrbitLinkOpener {
  openFile: (
    rawPath: string,
    opts?: {
      focusLine?: number | null;
      focusEndLine?: number | null;
      hostId?: string;
      cwd?: string;
      fallbackPath?: string;
    },
  ) => void;
  openBrowser: (url: string) => void;
  openExternal: (url: string) => void;
}

let registeredOpener: OrbitLinkOpener | null = null;

/**
 * Registers the runtime handlers used by `openOrbitLink`.
 * Called once by `orbit-store` so `file-links.ts` stays dependency-free and testable.
 */
export function registerOrbitLinkOpener(opener: OrbitLinkOpener): void {
  registeredOpener = opener;
}

function safeDecodeURIComponent(value: string): string {
  if (!/%[0-9A-Fa-f]{2}/.test(value)) return value;
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function parsePositiveInt(raw: string | undefined): number | null {
  if (!raw) return null;
  const n = Number.parseInt(raw, 10);
  if (!Number.isFinite(n) || n <= 0) return null;
  return n;
}

export function normalizeLineRange(
  startLine?: number | null,
  endLine?: number | null,
): NormalizedLineRange {
  const start =
    typeof startLine === "number" && Number.isFinite(startLine) && startLine > 0
      ? Math.trunc(startLine)
      : null;
  const end =
    typeof endLine === "number" && Number.isFinite(endLine) && endLine > 0
      ? Math.trunc(endLine)
      : null;

  if (start == null) {
    return end != null ? { startLine: end, endLine: null } : { startLine: null, endLine: null };
  }
  if (end == null || end === start) {
    return { startLine: start, endLine: null };
  }
  return end > start ? { startLine: start, endLine: end } : { startLine: end, endLine: start };
}

export function formatLineRangeLabel(
  startLine?: number | null,
  endLine?: number | null,
): string | null {
  const range = normalizeLineRange(startLine, endLine);
  if (range.startLine == null) return null;
  if (range.endLine != null && range.endLine > range.startLine) {
    return `L${range.startLine}–${range.endLine}`;
  }
  return `L${range.startLine}`;
}

interface ParsedLineSuffix {
  startLine: number | null;
  endLine: number | null;
  column: number | null;
}

function parseLineHash(rawHash: string): ParsedLineSuffix | null {
  const hash = rawHash.startsWith("#") ? rawHash.slice(1) : rawHash;
  if (!hash) return null;

  // `#L10`, `#L10-L25`, `#L10-25`, `#L10C5`, `#L10C5-L25C12`, `#10`, `#10-25`
  const match =
    /^L?(\d+)(?:[C:](\d+))?(?:\s*[-–—]\s*L?(\d+)(?:[C:](\d+))?)?$/i.exec(hash.trim());
  if (!match) return null;

  const start = parsePositiveInt(match[1]);
  if (start == null) return null;
  const col = parsePositiveInt(match[2]);
  const end = parsePositiveInt(match[3]);
  const normalized = normalizeLineRange(start, end);
  return {
    startLine: normalized.startLine,
    endLine: normalized.endLine,
    column: col,
  };
}

function stripPathLineSuffix(rawPath: string): {
  path: string;
  startLine: number | null;
  endLine: number | null;
  column: number | null;
} {
  // Handle `#L10-L25` fragment if still attached (e.g. non-URL relative links)
  const hashIdx = rawPath.indexOf("#");
  if (hashIdx > 0) {
    const base = rawPath.slice(0, hashIdx);
    const fragment = rawPath.slice(hashIdx + 1);
    const hashRange = parseLineHash(fragment);
    const stripped = stripPathLineSuffix(base);
    if (hashRange) {
      return {
        path: stripped.path,
        startLine: hashRange.startLine,
        endLine: hashRange.endLine,
        column: hashRange.column,
      };
    }
    return stripped;
  }

  // Handle `:L?10:5-L?25:12`, `:L?10-L?25`, `:L?10:5`, or `:L?10` at end of path
  const rangeColonMatch = /^(.*?):L?(\d+)(?::(\d+))?\s*[-–—]\s*L?(\d+)(?::(\d+))?$/i.exec(
    rawPath,
  );
  if (rangeColonMatch && rangeColonMatch[1].length > 0) {
    const start = parsePositiveInt(rangeColonMatch[2]);
    const col = parsePositiveInt(rangeColonMatch[3]);
    const end = parsePositiveInt(rangeColonMatch[4]);
    if (start != null) {
      const normalized = normalizeLineRange(start, end);
      return {
        path: rangeColonMatch[1],
        startLine: normalized.startLine,
        endLine: normalized.endLine,
        column: col,
      };
    }
  }

  const lineColMatch = /^(.*?):L?(\d+):(\d+)$/i.exec(rawPath);
  if (lineColMatch && lineColMatch[1].length > 0) {
    const start = parsePositiveInt(lineColMatch[2]);
    const col = parsePositiveInt(lineColMatch[3]);
    if (start != null) {
      return {
        path: lineColMatch[1],
        startLine: start,
        endLine: null,
        column: col,
      };
    }
  }

  const singleLineMatch = /^(.*?):L?(\d+)$/i.exec(rawPath);
  if (singleLineMatch && singleLineMatch[1].length > 0) {
    const start = parsePositiveInt(singleLineMatch[2]);
    if (start != null) {
      return {
        path: singleLineMatch[1],
        startLine: start,
        endLine: null,
        column: null,
      };
    }
  }

  return {
    path: rawPath,
    startLine: null,
    endLine: null,
    column: null,
  };
}

function isLocalHostname(hostname: string): boolean {
  const host = hostname.toLowerCase();
  return (
    host === "localhost" ||
    host === "127.0.0.1" ||
    host === "0.0.0.0" ||
    host === "::1" ||
    host === "[::1]" ||
    host.endsWith(".localhost")
  );
}

const DISALLOWED_SCHEMES = new Set([
  "javascript:",
  "data:",
  "vbscript:",
  "blob:",
  "about:",
]);

const EDITOR_FILE_SCHEMES = new Set(["vscode:", "vscode-insiders:", "zed:", "cursor:"]);

export function parseOrbitLinkTarget(rawHref: string): OrbitLinkTarget | null {
  const trimmed = rawHref.trim();
  if (!trimmed) return null;

  if (trimmed.startsWith("#")) {
    return { kind: "anchor", fragment: trimmed.slice(1) };
  }

  // Check explicit URL schemes (ignoring Windows drive letters like C:\ or C:/)
  const schemeMatch = /^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(trimmed);
  const hasWindowsDrivePrefix = /^[a-zA-Z]:[\\/]/.test(trimmed);

  if (schemeMatch && !hasWindowsDrivePrefix) {
    const scheme = `${schemeMatch[1].toLowerCase()}:`;
    if (DISALLOWED_SCHEMES.has(scheme)) {
      return null;
    }

    if (scheme === "mailto:") {
      return { kind: "external", url: trimmed };
    }

    if (scheme === "http:" || scheme === "https:") {
      try {
        const parsed = new URL(trimmed);
        return isLocalHostname(parsed.hostname)
          ? { kind: "browser", url: trimmed }
          : { kind: "external", url: trimmed };
      } catch {
        return null;
      }
    }

    if (scheme === "file:") {
      try {
        const parsed = new URL(trimmed);
        const hashInfo = parsed.hash ? parseLineHash(parsed.hash) : null;
        let pathname = safeDecodeURIComponent(parsed.pathname);
        // Handle `file://relative/path` where URL put first segment in hostname
        if (parsed.hostname && parsed.hostname.toLowerCase() !== "localhost") {
          pathname = `${parsed.hostname}${pathname}`;
        }
        // Normalize Windows `file:///C:/...` -> `C:/...`
        if (/^\/[a-zA-Z]:[\\/]/.test(pathname)) {
          pathname = pathname.slice(1);
        }
        const stripped = stripPathLineSuffix(pathname);
        const range = normalizeLineRange(
          hashInfo?.startLine ?? stripped.startLine,
          hashInfo?.endLine ?? stripped.endLine,
        );
        if (!stripped.path) return null;
        return {
          kind: "file",
          rawPath: stripped.path,
          startLine: range.startLine,
          endLine: range.endLine,
          column: hashInfo?.column ?? stripped.column,
        };
      } catch {
        // Fallback string strip for malformed file:// URIs
        const withoutPrefix = safeDecodeURIComponent(trimmed.replace(/^file:\/\//i, ""));
        const stripped = stripPathLineSuffix(withoutPrefix);
        if (!stripped.path) return null;
        return {
          kind: "file",
          rawPath: stripped.path,
          startLine: stripped.startLine,
          endLine: stripped.endLine,
          column: stripped.column,
        };
      }
    }

    if (EDITOR_FILE_SCHEMES.has(scheme)) {
      try {
        const parsed = new URL(trimmed);
        if (parsed.hostname.toLowerCase() !== "file") {
          return null;
        }
        const hashInfo = parsed.hash ? parseLineHash(parsed.hash) : null;
        let pathname = safeDecodeURIComponent(parsed.pathname);
        // Collapse `vscode://file//Users/...` -> `/Users/...`, keep `vscode://file/rel/...` -> `rel/...`?
        // Standard `vscode://file//abs/path` gives pathname `//abs/path` or `/abs/path`.
        if (pathname.startsWith("//")) {
          pathname = pathname.slice(1);
        }
        if (/^\/[a-zA-Z]:[\\/]/.test(pathname)) {
          pathname = pathname.slice(1);
        }
        const stripped = stripPathLineSuffix(pathname);
        const range = normalizeLineRange(
          hashInfo?.startLine ?? stripped.startLine,
          hashInfo?.endLine ?? stripped.endLine,
        );
        if (!stripped.path) return null;
        return {
          kind: "file",
          rawPath: stripped.path,
          startLine: range.startLine,
          endLine: range.endLine,
          column: hashInfo?.column ?? stripped.column,
        };
      } catch {
        return null;
      }
    }

    // Any other unrecognized URL scheme (not a bare path with `:line`)
    if (trimmed.includes("://")) {
      return null;
    }
  }

  // Bare file path (absolute, `~/...`, relative, `./...`, or `path/file.rs#L10-L20` / `path/file.rs:10-20`)
  const stripped = stripPathLineSuffix(trimmed);
  const decodedPath = safeDecodeURIComponent(stripped.path).trim();
  if (!decodedPath) return null;

  return {
    kind: "file",
    rawPath: decodedPath,
    startLine: stripped.startLine,
    endLine: stripped.endLine,
    column: stripped.column,
  };
}

/**
 * Resolves a relative link inside a Markdown file against `baseFilePath` directory
 * when `linkPath` is explicitly relative (`./` or `../`) or has no leading slash.
 */
export function resolvePathAgainstBase(
  linkPath: string,
  baseFilePath?: string,
): { direct: string; fallback: string } {
  const clean = linkPath.trim().replace(/\\/g, "/");
  if (
    !baseFilePath ||
    clean.startsWith("/") ||
    clean.startsWith("~/") ||
    /^[a-zA-Z]:\//.test(clean)
  ) {
    return { direct: clean, fallback: clean };
  }

  const normalizedBase = baseFilePath.trim().replace(/\\/g, "/");
  const lastSlash = normalizedBase.lastIndexOf("/");
  if (lastSlash < 0) {
    return { direct: clean, fallback: clean };
  }

  const baseDir = normalizedBase.slice(0, lastSlash);
  const parts = [...baseDir.split("/").filter(Boolean), ...clean.split("/")];
  const resolved: string[] = [];
  for (const seg of parts) {
    if (!seg || seg === ".") continue;
    if (seg === "..") {
      if (resolved.length > 0 && resolved[resolved.length - 1] !== "..") {
        resolved.pop();
      } else if (!normalizedBase.startsWith("/")) {
        resolved.push("..");
      }
    } else {
      resolved.push(seg);
    }
  }
  const joined = (normalizedBase.startsWith("/") ? "/" : "") + resolved.join("/");
  return {
    direct: joined || clean,
    fallback: clean,
  };
}

/**
 * Strips leading `cwd/` or `./` prefixes to produce a clean workspace-relative path when possible.
 */
export function normalizeWorkspacePathCandidate(rawPath: string, cwd?: string): string {
  const clean = rawPath.trim().replace(/\\/g, "/");
  if (!clean) return clean;

  if (clean.startsWith("./")) {
    return clean.slice(2);
  }

  if (cwd && cwd !== "~") {
    const cleanCwd = cwd.trim().replace(/\\/g, "/").replace(/\/+$/, "");
    if (cleanCwd && clean.startsWith(`${cleanCwd}/`)) {
      return clean.slice(cleanCwd.length + 1);
    }
  }

  return clean;
}

/**
 * Finds a unique workspace file matching `candidate` either exactly or by `/<candidate>` suffix
 * (for shortened agent references such as `orbit-ffi-ohos/src/lib.rs` -> `crates/orbit-ffi-ohos/src/lib.rs`).
 */
export function resolveUniqueWorkspaceFileSuffix(
  candidate: string,
  workspaceFiles?: readonly string[] | null,
): string | null {
  if (!workspaceFiles || workspaceFiles.length === 0) return null;
  const clean = candidate.replace(/^\.\//, "").replace(/^\/+/, "");
  if (!clean) return null;

  if (workspaceFiles.includes(clean)) {
    return clean;
  }

  const suffix = `/${clean}`;
  let matched: string | null = null;
  for (const file of workspaceFiles) {
    if (file === clean || file.endsWith(suffix)) {
      if (matched !== null && matched !== file) {
        return null; // Ambiguous match
      }
      matched = file;
    }
  }
  return matched;
}

/**
 * Opens any supported Markdown/UI target:
 * - external `https://` -> system default browser
 * - `http://localhost` / `127.0.0.1` -> Orbit right-panel Browser tab
 * - file path / `file://` / `vscode://file/` / `zed://file/` -> Orbit right-panel FileViewerTab
 * - `#anchor` -> optional `onAnchor` callback
 */
export function openOrbitLink(rawHref: string, opts?: OpenOrbitLinkOptions): boolean {
  const target = parseOrbitLinkTarget(rawHref);
  if (!target) return false;

  if (target.kind === "anchor") {
    opts?.onAnchor?.(target.fragment);
    return true;
  }

  if (!registeredOpener) {
    return false;
  }

  if (target.kind === "browser") {
    registeredOpener.openBrowser(target.url);
    return true;
  }

  if (target.kind === "external") {
    registeredOpener.openExternal(target.url);
    return true;
  }

  const isExplicitRelative =
    target.rawPath.startsWith("./") || target.rawPath.startsWith("../");
  const baseResolved = resolvePathAgainstBase(target.rawPath, opts?.baseFilePath);
  const pathToOpen = opts?.baseFilePath ? baseResolved.direct : target.rawPath;
  const fallbackPath =
    opts?.baseFilePath &&
    !isExplicitRelative &&
    baseResolved.fallback !== baseResolved.direct
      ? baseResolved.fallback
      : undefined;

  registeredOpener.openFile(pathToOpen, {
    focusLine: target.startLine,
    focusEndLine: target.endLine,
    hostId: opts?.hostId,
    cwd: opts?.cwd,
    fallbackPath,
  });
  return true;
}
