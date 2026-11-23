import type { DiffFileView, DiffLineView } from "./orbit-ipc.ts";

export const NO_NEWLINE_MARKER = "\\ No newline at end of file";

const MAX_REL_PATH_BYTES = 4096;
const HUNK_HEADER_RE = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/;

export interface ParsedHunkHeader {
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  sectionHeading: string;
}

export interface DiffHunkView {
  /** Original `@@ -oldStart,oldCount +newStart,newCount @@ [heading]` line, if available. */
  header?: string;
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  /** Optional section heading text following the closing `@@`. */
  sectionHeading?: string;
  /** Hunk body lines (`context` | `added` | `removed`, plus optional no-newline markers). */
  lines: DiffLineView[];
  /** True when the hunk represents a newly added file (`--- /dev/null`). */
  isNewFile?: boolean;
  /** True when the hunk represents a deleted file (`+++ /dev/null`). */
  isDeletedFile?: boolean;
  /** True when the diff preview capped this hunk before all lines were rendered. */
  truncated?: boolean;
}

export interface IndexedDiffHunk extends DiffHunkView {
  /** Zero-based hunk index within the file. */
  hunkIndex: number;
  /** Line index of this hunk's `hunk_header` entry inside `DiffFileView.lines`. */
  headerLineIndex: number;
}

function utf8ByteLength(value: string): number {
  let bytes = 0;
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code <= 0x7f) {
      bytes += 1;
    } else if (code <= 0x7ff) {
      bytes += 2;
    } else if (code >= 0xd800 && code <= 0xdbff) {
      // Surrogate pair (4 UTF-8 bytes)
      bytes += 4;
      i += 1;
    } else {
      bytes += 3;
    }
  }
  return bytes;
}

function hasControlChar(value: string): boolean {
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code <= 0x1f || code === 0x7f) {
      return true;
    }
  }
  return false;
}

/**
 * Parse a standard unified diff hunk header (`@@ -oldStart[,oldCount] +newStart[,newCount] @@[ heading]`).
 */
export function parseHunkHeader(header: string): ParsedHunkHeader | null {
  const clean = header.endsWith("\r") ? header.slice(0, -1) : header;
  const match = HUNK_HEADER_RE.exec(clean);
  if (!match) return null;

  const oldStart = Number.parseInt(match[1], 10);
  const oldCount = match[2] !== undefined ? Number.parseInt(match[2], 10) : 1;
  const newStart = Number.parseInt(match[3], 10);
  const newCount = match[4] !== undefined ? Number.parseInt(match[4], 10) : 1;
  const sectionHeading = match[5] ?? "";

  if (
    !Number.isSafeInteger(oldStart) ||
    !Number.isSafeInteger(oldCount) ||
    !Number.isSafeInteger(newStart) ||
    !Number.isSafeInteger(newCount) ||
    oldStart < 0 ||
    oldCount < 0 ||
    newStart < 0 ||
    newCount < 0
  ) {
    return null;
  }

  return { oldStart, oldCount, newStart, newCount, sectionHeading };
}

/**
 * Validate and normalize a repository-relative file path to match Orbit daemon's
 * strict per-section unified diff validator (`validate_git_rel_path` / `extract_patch_header_path`).
 *
 * Preserves literal spaces inside filenames (never calls `.trim()`).
 */
export function normalizeAndValidatePatchPath(filePath: string): string {
  if (typeof filePath !== "string" || filePath.length === 0) {
    throw new Error("Invalid patch path: path cannot be empty");
  }
  if (utf8ByteLength(filePath) > MAX_REL_PATH_BYTES) {
    throw new Error("Invalid patch path: path exceeds maximum length");
  }
  if (hasControlChar(filePath)) {
    throw new Error("Invalid patch path: path contains NUL or control characters");
  }
  if (filePath.includes("\\")) {
    throw new Error("Invalid patch path: path must use forward slashes (`/`)");
  }
  if (filePath.startsWith('"') || filePath.endsWith('"')) {
    throw new Error("Invalid patch path: quoted diff paths are not supported");
  }
  if (filePath.startsWith("-")) {
    throw new Error("Invalid patch path: path must not start with `-`");
  }
  if (
    filePath.startsWith("/") ||
    filePath === "~" ||
    filePath.startsWith("~/") ||
    /^[A-Za-z]:(?:\/|$)/.test(filePath)
  ) {
    throw new Error("Invalid patch path: absolute or home-prefixed paths are not allowed");
  }

  const rawSegments = filePath.split("/");
  const segments: string[] = [];
  for (const seg of rawSegments) {
    if (seg === "" || seg === ".") {
      continue;
    }
    if (seg === "..") {
      throw new Error("Invalid patch path: path traversal (`..`) is not allowed");
    }
    if (seg.toLowerCase() === ".git") {
      throw new Error("Invalid patch path: modifying `.git` internals is forbidden");
    }
    segments.push(seg);
  }

  if (segments.length === 0) {
    throw new Error("Invalid patch path: path must target a file inside the repository");
  }

  const normalized = segments.join("/");
  if (normalized.startsWith("-")) {
    throw new Error("Invalid patch path: normalized path must not start with `-`");
  }
  if (normalized.startsWith('"') || normalized.endsWith('"')) {
    throw new Error("Invalid patch path: quoted diff paths are not supported");
  }

  return normalized;
}

function isNoNewlineLine(line: DiffLineView): boolean {
  if (line.kind !== "context" && line.kind !== "meta") {
    return false;
  }
  const clean = line.content.endsWith("\r") ? line.content.slice(0, -1) : line.content;
  return clean === NO_NEWLINE_MARKER;
}

function sanitizeLineContent(raw: string): string {
  const clean = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
  if (clean.includes("\0") || clean.includes("\n") || clean.includes("\r")) {
    throw new Error("Invalid hunk line: embedded newline or NUL character is not allowed");
  }
  return clean;
}

function isValidLineNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

/**
 * Build a strict, self-contained single-hunk unified diff patch compatible with Orbit B2-01's
 * per-section validator (`validate_hunk_patch`) and `git apply --unidiff-zero`.
 */
export function buildSingleHunkPatch(filePath: string, hunk: DiffHunkView): string {
  const normalizedPath = normalizeAndValidatePatchPath(filePath);

  if (!hunk || typeof hunk !== "object") {
    throw new Error("Cannot build patch from an empty hunk");
  }
  if (hunk.truncated) {
    throw new Error("Cannot build patch for a truncated hunk preview");
  }
  if (hunk.isNewFile && hunk.isDeletedFile) {
    throw new Error("Invalid hunk: file cannot be both newly added and deleted");
  }
  if (!Array.isArray(hunk.lines) || hunk.lines.length === 0) {
    throw new Error("Cannot build patch from an empty hunk");
  }

  const parsedHeader = hunk.header ? parseHunkHeader(hunk.header) : null;

  const bodyLines: string[] = [];
  let computedOldCount = 0;
  let computedNewCount = 0;
  let changeCount = 0;
  let hasBodyLine = false;
  let firstOldLine: number | null = null;
  let firstNewLine: number | null = null;

  for (const line of hunk.lines) {
    if (!line) continue;

    if (isNoNewlineLine(line)) {
      if (hasBodyLine) {
        bodyLines.push(NO_NEWLINE_MARKER);
      }
      continue;
    }

    if (line.kind === "meta" || line.kind === "hunk_header") {
      continue;
    }

    const content = sanitizeLineContent(line.content);

    if (line.kind === "context") {
      if (firstOldLine === null && isValidLineNumber(line.old_line)) {
        firstOldLine = line.old_line;
      }
      if (firstNewLine === null && isValidLineNumber(line.new_line)) {
        firstNewLine = line.new_line;
      }
      bodyLines.push(` ${content}`);
      computedOldCount += 1;
      computedNewCount += 1;
      hasBodyLine = true;
    } else if (line.kind === "removed") {
      if (firstOldLine === null && isValidLineNumber(line.old_line)) {
        firstOldLine = line.old_line;
      }
      bodyLines.push(`-${content}`);
      computedOldCount += 1;
      changeCount += 1;
      hasBodyLine = true;
    } else if (line.kind === "added") {
      if (firstNewLine === null && isValidLineNumber(line.new_line)) {
        firstNewLine = line.new_line;
      }
      bodyLines.push(`+${content}`);
      computedNewCount += 1;
      changeCount += 1;
      hasBodyLine = true;
    }
  }

  if (computedOldCount === 0 && computedNewCount === 0) {
    throw new Error("Cannot build patch from an empty hunk");
  }
  if (changeCount === 0) {
    throw new Error("Cannot build patch from a hunk with no additions or deletions");
  }

  const resolvedOldStart = isValidLineNumber(hunk.oldStart)
    ? hunk.oldStart
    : parsedHeader
      ? parsedHeader.oldStart
      : firstOldLine ?? (computedOldCount === 0 ? 0 : 1);

  const resolvedNewStart = isValidLineNumber(hunk.newStart)
    ? hunk.newStart
    : parsedHeader
      ? parsedHeader.newStart
      : firstNewLine ?? (computedNewCount === 0 ? 0 : 1);

  const oldStart = hunk.isNewFile && computedOldCount === 0 ? 0 : resolvedOldStart;
  const newStart = hunk.isDeletedFile && computedNewCount === 0 ? 0 : resolvedNewStart;

  const rawHeading = hunk.sectionHeading ?? parsedHeader?.sectionHeading ?? "";
  const cleanHeading = rawHeading.replace(/[\0\r\n]+/g, " ").trimEnd();
  const headingSuffix =
    cleanHeading.length === 0
      ? ""
      : cleanHeading.startsWith(" ")
        ? cleanHeading
        : ` ${cleanHeading}`;

  const diffGitHeader = `diff --git a/${normalizedPath} b/${normalizedPath}`;
  const minusHeader = hunk.isNewFile ? "--- /dev/null" : `--- a/${normalizedPath}`;
  const plusHeader = hunk.isDeletedFile ? "+++ /dev/null" : `+++ b/${normalizedPath}`;
  const hunkHeader = `@@ -${oldStart},${computedOldCount} +${newStart},${computedNewCount} @@${headingSuffix}`;

  return `${diffGitHeader}\n${minusHeader}\n${plusHeader}\n${hunkHeader}\n${bodyLines.join("\n")}\n`;
}

/**
 * Extract structured, indexed hunks from a `DiffFileView` produced by `parse_unified_diff`.
 *
 * Detects new/deleted file markers and marks any hunk whose rendered line count does not match
 * its `@@ ... @@` header (e.g. when capped by the 400-line preview limit) as `truncated: true`.
 */
export function extractFileHunks(file: DiffFileView): IndexedDiffHunk[] {
  const hunks: IndexedDiffHunk[] = [];
  let isNewFile = false;
  let isDeletedFile = false;
  let seenFirstHunk = false;

  let activeHunk: {
    hunkIndex: number;
    headerLineIndex: number;
    header: string;
    parsed: ParsedHunkHeader;
    lines: DiffLineView[];
    bodyOldCount: number;
    bodyNewCount: number;
  } | null = null;

  const flushActive = () => {
    if (!activeHunk) return;
    const { hunkIndex, headerLineIndex, header, parsed, lines, bodyOldCount, bodyNewCount } =
      activeHunk;
    const truncated = bodyOldCount !== parsed.oldCount || bodyNewCount !== parsed.newCount;
    hunks.push({
      hunkIndex,
      headerLineIndex,
      header,
      oldStart: parsed.oldStart,
      oldCount: parsed.oldCount,
      newStart: parsed.newStart,
      newCount: parsed.newCount,
      sectionHeading: parsed.sectionHeading || undefined,
      lines,
      isNewFile: isNewFile || undefined,
      isDeletedFile: isDeletedFile || undefined,
      truncated: truncated || undefined,
    });
    activeHunk = null;
  };

  for (let idx = 0; idx < file.lines.length; idx += 1) {
    const line = file.lines[idx];

    if (!seenFirstHunk && line.kind !== "hunk_header") {
      if (line.kind === "meta") {
        if (line.content === "--- /dev/null" || line.content.startsWith("new file mode ")) {
          isNewFile = true;
        } else if (line.content.startsWith("deleted file mode ")) {
          isDeletedFile = true;
        }
      } else if (line.kind === "added" && line.content === "++ /dev/null") {
        // `parse_unified_diff` classifies `+++ /dev/null` as an added line prior to the first hunk
        isDeletedFile = true;
      }
      continue;
    }

    if (line.kind === "hunk_header") {
      seenFirstHunk = true;
      flushActive();
      const parsed = parseHunkHeader(line.content);
      if (!parsed) continue;
      activeHunk = {
        hunkIndex: hunks.length,
        headerLineIndex: idx,
        header: line.content,
        parsed,
        lines: [],
        bodyOldCount: 0,
        bodyNewCount: 0,
      };
      continue;
    }

    if (!activeHunk) continue;

    if (isNoNewlineLine(line)) {
      activeHunk.lines.push(line);
      continue;
    }

    if (line.kind === "meta") {
      // Trailing omission notice (`… N more diff lines omitted`) is not part of hunk body
      continue;
    }

    activeHunk.lines.push(line);
    if (line.kind === "context") {
      activeHunk.bodyOldCount += 1;
      activeHunk.bodyNewCount += 1;
    } else if (line.kind === "removed") {
      activeHunk.bodyOldCount += 1;
    } else if (line.kind === "added") {
      activeHunk.bodyNewCount += 1;
    }
  }

  flushActive();
  return hunks;
}

/**
 * Derive a clean, human-readable default Pull Request title from either the latest commit
 * summary or the active Git branch name.
 */
export function deriveDefaultPrTitle(
  branch: string | null | undefined,
  lastCommitSummary?: string | null,
): string {
  const summary = lastCommitSummary?.trim();
  if (summary) {
    return summary.split(/\r?\n/)[0].trim();
  }
  const rawBranch = branch?.trim();
  if (!rawBranch) return "";
  const lower = rawBranch.toLowerCase();
  if (lower === "main" || lower === "master" || lower === "head" || lower === "develop") {
    return "";
  }
  // Use the trailing segment after common namespace prefixes (e.g. `feat/foo-bar` -> `foo-bar`)
  const leaf = rawBranch.includes("/") ? rawBranch.slice(rawBranch.lastIndexOf("/") + 1) : rawBranch;
  const cleaned = leaf.replace(/[-_]+/g, " ").replace(/\s+/g, " ").trim();
  if (!cleaned) return "";
  return cleaned.charAt(0).toUpperCase() + cleaned.slice(1);
}
