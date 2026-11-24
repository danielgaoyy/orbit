import type { InlineMentionTarget } from "@/state/store-types";

export interface InlineMentionSpan {
  start: number;
  end: number;
  mention: InlineMentionTarget;
}

export interface AtomicMentionEditResult {
  nextText: string;
  nextCursor: number;
  removedToken: string;
}

function isMentionWordChar(ch: string): boolean {
  return /[A-Za-z0-9_$]/.test(ch);
}

function isTokenBoundaryAt(text: string, start: number, token: string): boolean {
  const end = start + token.length;
  const prevCh = start > 0 ? text[start - 1] : "";
  const leftOk = !prevCh || !isMentionWordChar(prevCh);
  if (!leftOk) return false;
  const rest = text.slice(end);
  const nextCh = rest[0] ?? "";
  if (!nextCh) return true;
  if (isMentionWordChar(nextCh)) return false;
  if (nextCh === ":") {
    if (rest.startsWith("::") || /^:L\d/.test(rest)) {
      return false;
    }
  }
  return true;
}

/**
 * Find all non-overlapping, boundary-valid occurrences of registered inline `@` mention tokens
 * in `text`, ordered left-to-right. Longer tokens take precedence when prefixes overlap.
 */
export function findInlineMentionSpans(
  text: string,
  mentions: readonly InlineMentionTarget[],
): InlineMentionSpan[] {
  if (!text || mentions.length === 0) return [];
  const sortedMentions = mentions
    .filter((m) => m.token.length > 0)
    .slice()
    .sort((a, b) => b.token.length - a.token.length);

  const candidates: InlineMentionSpan[] = [];
  for (const mention of sortedMentions) {
    const token = mention.token;
    let searchFrom = 0;
    while (searchFrom <= text.length - token.length) {
      const idx = text.indexOf(token, searchFrom);
      if (idx < 0) break;
      if (isTokenBoundaryAt(text, idx, token)) {
        candidates.push({ start: idx, end: idx + token.length, mention });
      }
      searchFrom = idx + 1;
    }
  }

  candidates.sort((a, b) => a.start - b.start || b.end - a.end);
  const resolved: InlineMentionSpan[] = [];
  let cursorEnd = 0;
  for (const span of candidates) {
    if (span.start < cursorEnd) continue;
    resolved.push(span);
    cursorEnd = span.end;
  }
  return resolved;
}

function spliceSpanCleanly(text: string, start: number, end: number): {
  nextText: string;
  nextCursor: number;
} {
  let left = text.slice(0, start);
  let right = text.slice(end);

  // Avoid leaving double horizontal spaces or stray leading space after removing a chip
  if (left.endsWith(" ") && right.startsWith(" ")) {
    right = right.slice(1);
  } else if (left.length === 0 && right.startsWith(" ")) {
    right = right.slice(1);
  } else if (right.length === 0 && left.endsWith(" ")) {
    left = left.slice(0, -1);
  }
  return {
    nextText: `${left}${right}`,
    nextCursor: left.length,
  };
}

/**
 * Atomic Backspace handling for inline `@` mention chips:
 * when the caret sits at the trailing edge of, or anywhere inside, a bound mention token,
 * remove the entire token atomically instead of leaving half-edited `@file` text.
 */
export function resolveAtomicMentionBackspace(
  text: string,
  selectionStart: number,
  selectionEnd: number,
  mentions: readonly InlineMentionTarget[],
): AtomicMentionEditResult | null {
  if (selectionStart !== selectionEnd || selectionStart <= 0 || mentions.length === 0) {
    return null;
  }
  const spans = findInlineMentionSpans(text, mentions);
  for (const span of spans) {
    if (selectionStart > span.start && selectionStart <= span.end) {
      const { nextText, nextCursor } = spliceSpanCleanly(text, span.start, span.end);
      return { nextText, nextCursor, removedToken: span.mention.token };
    }
  }
  return null;
}

/**
 * Atomic forward-Delete handling when the caret sits at the leading edge of or inside a bound token.
 */
export function resolveAtomicMentionDelete(
  text: string,
  selectionStart: number,
  selectionEnd: number,
  mentions: readonly InlineMentionTarget[],
): AtomicMentionEditResult | null {
  if (selectionStart !== selectionEnd || selectionStart >= text.length || mentions.length === 0) {
    return null;
  }
  const spans = findInlineMentionSpans(text, mentions);
  for (const span of spans) {
    if (selectionStart >= span.start && selectionStart < span.end) {
      const { nextText, nextCursor } = spliceSpanCleanly(text, span.start, span.end);
      return { nextText, nextCursor, removedToken: span.mention.token };
    }
  }
  return null;
}

/**
 * Remove all boundary-matched occurrences of `token` from `text` while preserving newlines and
 * collapsing redundant spaces cleanly.
 */
export function removeInlineMentionFromDraft(text: string, token: string): string {
  if (!text || !token) return text;
  let current = text;
  let guard = 0;
  while (guard < 64) {
    guard += 1;
    let foundIdx = -1;
    let searchFrom = 0;
    while (searchFrom <= current.length - token.length) {
      const idx = current.indexOf(token, searchFrom);
      if (idx < 0) break;
      if (isTokenBoundaryAt(current, idx, token)) {
        foundIdx = idx;
        break;
      }
      searchFrom = idx + 1;
    }
    if (foundIdx < 0) break;
    current = spliceSpanCleanly(current, foundIdx, foundIdx + token.length).nextText;
  }
  return current;
}

/**
 * Strictly validate an explicit `https://...` URL reference without performing any network I/O.
 */
export function isValidHttpsReferenceUrl(raw: string): boolean {
  const trimmed = raw.trim();
  if (trimmed.length < 10 || trimmed.length > 2048) return false;
  if (!/^https:\/\//i.test(trimmed)) return false;
  if (/[\s<>"'\u0000-\u001f\u007f]/.test(trimmed)) return false;
  const rest = trimmed.slice(8);
  const authority = rest.split(/[/?#]/)[0] ?? "";
  if (!authority || authority.includes("@")) return false;
  const host = authority.split(":")[0] ?? "";
  return /[A-Za-z0-9]/.test(host) && (host.includes(".") || host.toLowerCase() === "localhost");
}

export function shortWebUrlLabel(url: string): string {
  const clean = url
    .trim()
    .replace(/^https:\/\//i, "")
    .replace(/\/+$/, "");
  return clean.length <= 44 ? clean : `${clean.slice(0, 43)}…`;
}

/**
 * Build a concise structured directory tree summary for `@folder:<path>` from cached workspace files.
 */
export function buildFolderTreeSummary(
  folderPath: string,
  workspaceFiles: readonly string[],
  maxEntries = 80,
): { fileCount: number; treeSummary: string } {
  const cleanFolder = folderPath.trim().replace(/\\/g, "/").replace(/\/+$/, "");
  if (!cleanFolder) {
    return { fileCount: 0, treeSummary: "" };
  }
  const prefix = `${cleanFolder}/`;
  const matched: string[] = [];
  for (const raw of workspaceFiles) {
    const norm = raw.trim().replace(/\\/g, "/");
    if (norm.startsWith(prefix)) {
      matched.push(norm.slice(prefix.length));
    }
  }
  matched.sort((a, b) => a.localeCompare(b));
  const total = matched.length;
  if (total === 0) {
    return {
      fileCount: 0,
      treeSummary: `Directory \`${cleanFolder}/\` (no indexed files cached yet).`,
    };
  }
  const shown = matched.slice(0, maxEntries);
  const lines = shown.map((rel) => `- ${rel}`);
  if (total > shown.length) {
    lines.push(`- … (+${total - shown.length} more files)`);
  }
  return {
    fileCount: total,
    treeSummary: lines.join("\n"),
  };
}

/**
 * Strip optional YAML frontmatter (`---\n...\n---`) from a `.claude/commands/*.md` template
 * and substitute `$ARGUMENTS` cleanly (matching Rust `orbit_protocol::expand_command_arguments`).
 */
export function expandClaudeCommandArguments(
  templateContent: string,
  argumentsText: string,
): string {
  const normalized = templateContent.startsWith("\ufeff")
    ? templateContent.slice(1)
    : templateContent;
  let body = normalized;
  const openMatch = /^---\r?\n/.exec(normalized);
  if (openMatch) {
    const rest = normalized.slice(openMatch[0].length);
    const closeMatch = /^(?:---|\.\.\.)\r?(?:\n|$)/m.exec(rest);
    if (closeMatch && closeMatch.index !== undefined) {
      body = rest.slice(closeMatch.index + closeMatch[0].length);
    }
  }
  const cleanBody = body.trim();
  const cleanArgs = argumentsText.trim();
  if (cleanBody.includes("$ARGUMENTS")) {
    return cleanBody.split("$ARGUMENTS").join(cleanArgs).trim();
  }
  if (!cleanArgs) return cleanBody;
  if (!cleanBody) return cleanArgs;
  return `${cleanBody}\n\n${cleanArgs}`;
}
