export interface XtermBufferLineLike {
  readonly isWrapped?: boolean;
  translateToString(trimRight?: boolean, startColumn?: number, endColumn?: number): string;
}

export interface XtermBufferLike {
  readonly length: number;
  readonly baseY?: number;
  readonly viewportY?: number;
  getLine(y: number): XtermBufferLineLike | undefined;
}

export interface XtermSearchTarget {
  readonly cols: number;
  readonly rows: number;
  readonly buffer: { readonly active: XtermBufferLike };
  select(column: number, row: number, length: number): void;
  clearSelection(): void;
  scrollToLine(line: number): void;
}

export interface XtermSearchOptions {
  caseSensitive?: boolean;
  regex?: boolean;
  maxMatches?: number;
}

export interface XtermSearchMatch {
  /** 0-based buffer row index */
  row: number;
  /** 0-based character column on `row` */
  col: number;
  /** Match length in characters */
  length: number;
}

export interface XtermSearchResult {
  matches: XtermSearchMatch[];
  error: string | null;
  truncated: boolean;
}

export type XtermSearchShortcutAction = "open" | "next" | "prev" | "close" | null;

export interface XtermSearchShortcutEventLike {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  isComposing?: boolean;
  defaultPrevented?: boolean;
}

export interface XtermSearchShortcutContext {
  isMac: boolean;
  terminalHasFocus: boolean;
  findOpen: boolean;
  inSearchInput: boolean;
}

const DEFAULT_MAX_MATCHES = 2000;

function escapeRegExp(literal: string): string {
  return literal.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Pure, zero-dependency xterm buffer search engine.
 * Scans `buffer.active` lines incrementally with support for case sensitivity,
 * safe regex evaluation (never throws or infinite-loops on zero-width matches),
 * multiple matches per line, and bounded result count.
 */
export function searchXtermBuffer(
  buffer: XtermBufferLike,
  query: string,
  options: XtermSearchOptions = {},
): XtermSearchResult {
  if (!query) {
    return { matches: [], error: null, truncated: false };
  }

  const caseSensitive = Boolean(options.caseSensitive);
  const useRegex = Boolean(options.regex);
  const maxMatches = Math.max(1, options.maxMatches ?? DEFAULT_MAX_MATCHES);
  const matches: XtermSearchMatch[] = [];
  let truncated = false;

  if (useRegex) {
    let re: RegExp;
    try {
      re = new RegExp(query, caseSensitive ? "g" : "gi");
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      return { matches: [], error: message, truncated: false };
    }

    for (let y = 0; y < buffer.length; y += 1) {
      const line = buffer.getLine(y)?.translateToString(true);
      if (!line) continue;
      re.lastIndex = 0;
      let m: RegExpExecArray | null;
      while ((m = re.exec(line)) !== null) {
        if (m[0].length === 0) {
          if (re.lastIndex >= line.length) break;
          re.lastIndex += 1;
          continue;
        }
        matches.push({ row: y, col: m.index, length: m[0].length });
        if (matches.length >= maxMatches) {
          truncated = true;
          return { matches, error: null, truncated };
        }
      }
    }

    return { matches, error: null, truncated };
  }

  const needle = caseSensitive ? query : query.toLowerCase();
  let unicodeFallbackRe: RegExp | null = null;

  for (let y = 0; y < buffer.length; y += 1) {
    const line = buffer.getLine(y)?.translateToString(true);
    if (!line) continue;

    if (caseSensitive) {
      let from = 0;
      while (from <= line.length - needle.length) {
        const idx = line.indexOf(needle, from);
        if (idx === -1) break;
        matches.push({ row: y, col: idx, length: needle.length });
        if (matches.length >= maxMatches) {
          truncated = true;
          return { matches, error: null, truncated };
        }
        from = idx + Math.max(1, needle.length);
      }
      continue;
    }

    const lowered = line.toLowerCase();
    if (lowered.length === line.length) {
      let from = 0;
      while (from <= lowered.length - needle.length) {
        const idx = lowered.indexOf(needle, from);
        if (idx === -1) break;
        matches.push({ row: y, col: idx, length: needle.length });
        if (matches.length >= maxMatches) {
          truncated = true;
          return { matches, error: null, truncated };
        }
        from = idx + Math.max(1, needle.length);
      }
    } else {
      unicodeFallbackRe ??= new RegExp(escapeRegExp(query), "gi");
      unicodeFallbackRe.lastIndex = 0;
      let m: RegExpExecArray | null;
      while ((m = unicodeFallbackRe.exec(line)) !== null) {
        if (m[0].length === 0) break;
        matches.push({ row: y, col: m.index, length: m[0].length });
        if (matches.length >= maxMatches) {
          truncated = true;
          return { matches, error: null, truncated };
        }
      }
    }
  }

  return { matches, error: null, truncated };
}

/**
 * Wrap-around index navigation for search matches. Returns `-1` when there are no matches.
 */
export function stepXtermMatchIndex(
  currentIndex: number,
  matchCount: number,
  delta: number,
): number {
  if (matchCount <= 0) return -1;
  const base = currentIndex < 0 || currentIndex >= matchCount ? 0 : currentIndex;
  return (((base + delta) % matchCount) + matchCount) % matchCount;
}

/**
 * Scroll the terminal viewport to reveal `match` and visually highlight its exact span.
 */
export function revealXtermMatch(
  term: XtermSearchTarget,
  match: XtermSearchMatch | null,
): void {
  if (!match) {
    term.clearSelection();
    return;
  }
  const lead = Math.max(2, Math.floor(term.rows / 3));
  term.scrollToLine(Math.max(0, match.row - lead));
  term.select(match.col, match.row, match.length);
}

/**
 * Pure shortcut scope predicate for in-terminal search (`Cmd/Ctrl+F`, `Cmd/Ctrl+G`,
 * `Shift+Cmd/Ctrl+G`, `Enter`/`Shift+Enter`, `Escape`). Strictly requires focus inside
 * the terminal pane so it never hijacks global search or file viewer shortcuts.
 */
export function resolveXtermSearchShortcut(
  event: XtermSearchShortcutEventLike,
  ctx: XtermSearchShortcutContext,
): XtermSearchShortcutAction {
  if (event.defaultPrevented || event.isComposing || !ctx.terminalHasFocus) {
    return null;
  }

  const mod = ctx.isMac ? event.metaKey : event.ctrlKey;
  const otherMod = ctx.isMac ? event.ctrlKey : event.metaKey;
  const keyLower = event.key.toLowerCase();

  if (mod && !otherMod && !event.altKey) {
    if (!event.shiftKey && keyLower === "f") {
      return "open";
    }
    if (ctx.findOpen && keyLower === "g") {
      return event.shiftKey ? "prev" : "next";
    }
    return null;
  }

  if (ctx.findOpen && !mod && !otherMod && !event.altKey) {
    if (event.key === "Escape" && !event.shiftKey) {
      return "close";
    }
    if (ctx.inSearchInput && event.key === "Enter") {
      return event.shiftKey ? "prev" : "next";
    }
  }

  return null;
}
