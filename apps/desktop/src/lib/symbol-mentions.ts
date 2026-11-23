import type { MentionCandidateView, WorkspaceSymbolEntry } from "@/lib/orbit-ipc";

export interface ParsedSymbolMentionQuery {
  /** `true` when the user explicitly typed `@sym:` or `@#` to restrict results to code symbols. */
  symbolOnly: boolean;
  /** Stripped search query forwarded to the workspace symbol indexer. */
  query: string;
}

export interface SymbolKindGlyphMeta {
  glyph: string;
  label: string;
}

/**
 * Parse the raw query text after `@` in the composer.
 * Supports explicit `@sym:<query>` / `@#<query>` symbol-only triggers as well as
 * unified `@<query>` triggers that combine files and code symbols.
 */
export function parseSymbolMentionQuery(rawQuery: string): ParsedSymbolMentionQuery {
  const trimmed = rawQuery.trim();
  const lower = trimmed.toLowerCase();
  if (lower.startsWith("sym:")) {
    return { symbolOnly: true, query: trimmed.slice(4).trim() };
  }
  if (trimmed.startsWith("#")) {
    return { symbolOnly: true, query: trimmed.slice(1).trim() };
  }
  return { symbolOnly: false, query: trimmed };
}

/** Map normalized daemon symbol kind to a compact monochrome glyph. */
export function symbolKindGlyph(kind: string): SymbolKindGlyphMeta {
  switch (kind.toLowerCase()) {
    case "fn":
    case "function":
      return { glyph: "ƒ", label: "fn" };
    case "method":
      return { glyph: "ƒ", label: "method" };
    case "struct":
      return { glyph: "S", label: "struct" };
    case "class":
      return { glyph: "C", label: "class" };
    case "enum":
      return { glyph: "E", label: "enum" };
    case "trait":
    case "interface":
    case "protocol":
      return { glyph: "T", label: kind.toLowerCase() };
    case "type":
      return { glyph: "T", label: "type" };
    case "const":
    case "static":
      return { glyph: "K", label: "const" };
    case "module":
    case "namespace":
      return { glyph: "M", label: "mod" };
    default:
      return { glyph: "#", label: kind || "symbol" };
  }
}

/** Format a clean inline `@SymbolName` token for insertion into the composer draft. */
export function formatSymbolMentionToken(name: string): string {
  const clean = name.trim().replace(/\s+/g, "");
  return `@${clean}`;
}

function isMentionIdentChar(ch: string): boolean {
  return /[A-Za-z0-9_$]/.test(ch);
}

/**
 * Returns `true` when `token` (e.g. `@run`) appears in `draft` on clean token boundaries
 * (not followed by `[A-Za-z0-9_$]`, `::`, or `:L<digit>`), so deleting `@run` does not
 * falsely match `@run_turn` or `@run:L12`.
 */
export function containsInlineMentionToken(draft: string, token: string): boolean {
  if (!draft || !token) return false;
  let searchFrom = 0;
  while (searchFrom <= draft.length - token.length) {
    const idx = draft.indexOf(token, searchFrom);
    if (idx < 0) return false;
    const prevCh = idx > 0 ? draft[idx - 1] : "";
    const leftOk = !prevCh || !isMentionIdentChar(prevCh);
    const rest = draft.slice(idx + token.length);
    const nextCh = rest[0] ?? "";
    let rightOk = true;
    if (nextCh && isMentionIdentChar(nextCh)) {
      rightOk = false;
    } else if (nextCh === ":") {
      if (rest.startsWith("::") || /^:L\d/.test(rest)) {
        rightOk = false;
      }
    }
    if (leftOk && rightOk) {
      return true;
    }
    searchFrom = idx + 1;
  }
  return false;
}

export interface ExistingSymbolMentionRef {
  token: string;
  kind: string;
  path?: string;
  symbolName?: string;
  startLine?: number;
}

/**
 * Compute an inline token for a code symbol, keeping `@SymbolName` when unambiguous
 * and disambiguating colliding same-named symbols with `@Container::name` or `@name:L<startLine>`.
 */
export function resolveDisambiguatedSymbolToken(
  target: {
    symbolName: string;
    path: string;
    startLine: number;
    container?: string | null;
  },
  existingInline: readonly ExistingSymbolMentionRef[],
  currentDraft: string,
): string {
  const isSameSymbol = (m: ExistingSymbolMentionRef) =>
    m.kind === "code_symbol" &&
    m.path === target.path &&
    m.symbolName === target.symbolName &&
    m.startLine === target.startLine;

  const existingSame = existingInline.find(isSameSymbol);
  if (existingSame && containsInlineMentionToken(currentDraft, existingSame.token)) {
    return existingSame.token;
  }

  const collides = (candidate: string) =>
    existingInline.some(
      (m) =>
        !isSameSymbol(m) &&
        m.token === candidate &&
        containsInlineMentionToken(currentDraft, m.token),
    );

  const base = formatSymbolMentionToken(target.symbolName);
  if (!collides(base)) {
    return base;
  }

  const cleanContainer = target.container?.trim().replace(/\s+/g, "");
  if (cleanContainer) {
    const qualified = `@${cleanContainer}::${target.symbolName.trim().replace(/\s+/g, "")}`;
    if (!collides(qualified)) {
      return qualified;
    }
  }

  const lineQualified = `${base}:L${Math.max(1, Math.floor(target.startLine))}`;
  if (!collides(lineQualified)) {
    return lineQualified;
  }

  const slashIdx = target.path.lastIndexOf("/");
  const basename = (slashIdx >= 0 ? target.path.slice(slashIdx + 1) : target.path).replace(
    /\s+/g,
    "",
  );
  return `@${basename}:${target.symbolName.trim().replace(/\s+/g, "")}:L${Math.max(1, Math.floor(target.startLine))}`;
}

/** Convert a daemon `WorkspaceSymbolEntry` into a unified `MentionCandidateView` row. */
export function workspaceSymbolToMentionCandidate(
  entry: WorkspaceSymbolEntry,
  rankIndex: number,
): MentionCandidateView {
  const token = formatSymbolMentionToken(entry.name);
  const location = `${entry.path}:L${entry.start_line}`;
  return {
    key: `symbol::${entry.path}::${entry.name}::${entry.start_line}`,
    kind: "code_symbol",
    token_label: token,
    title: entry.name,
    subtitle: location,
    file_path: entry.path,
    git_status: "",
    score: Math.max(1, 500 - rankIndex),
    symbol_kind: entry.kind,
    start_line: entry.start_line,
    end_line: entry.end_line,
    signature: entry.signature ?? null,
    container: entry.container ?? null,
    language: entry.language,
  };
}

/**
 * Merge base `@` candidates (peers, git diffs, terminal, files) with fuzzy symbol hits.
 * - When `symbolOnly` (`@sym:` or `@#`), returns strictly code symbols.
 * - Otherwise keeps top base candidates followed by a crisp `Symbols` group.
 */
export function mergeMentionCandidates(
  baseCandidates: readonly MentionCandidateView[],
  symbolCandidates: readonly MentionCandidateView[],
  symbolOnly: boolean,
  maxTotal = 16,
): MentionCandidateView[] {
  const seen = new Set<string>();
  const out: MentionCandidateView[] = [];

  if (symbolOnly) {
    for (const sym of symbolCandidates) {
      if (out.length >= maxTotal) break;
      if (seen.has(sym.key)) continue;
      seen.add(sym.key);
      out.push(sym);
    }
    return out;
  }

  const maxBase = symbolCandidates.length > 0 ? Math.min(8, maxTotal) : maxTotal;
  for (const item of baseCandidates) {
    if (out.length >= maxBase) break;
    if (seen.has(item.key)) continue;
    seen.add(item.key);
    out.push(item);
  }

  for (const sym of symbolCandidates) {
    if (out.length >= maxTotal) break;
    if (seen.has(sym.key)) continue;
    seen.add(sym.key);
    out.push(sym);
  }

  return out;
}

/**
 * Extract an exact 1-based line-numbered code snippet from a workspace file's text,
 * bounded safely to `maxChars` so cited symbols attach crisp context to any provider.
 */
export function extractLineSnippet(
  fileContent: string,
  startLine: number,
  endLine: number,
  maxChars = 6000,
): string {
  if (!fileContent) return "";
  const lines = fileContent.split(/\r?\n/);
  const safeStart = Math.max(1, Math.floor(startLine));
  if (safeStart > lines.length) return "";
  const safeEnd = Math.min(
    lines.length,
    Math.max(safeStart, Math.floor(endLine)),
    safeStart + 180,
  );
  const formatted: string[] = [];
  let charCount = 0;
  for (let lineNo = safeStart; lineNo <= safeEnd; lineNo += 1) {
    const row = `${lineNo} | ${lines[lineNo - 1] ?? ""}`;
    if (charCount + row.length + 1 > maxChars) {
      if (formatted.length === 0) {
        formatted.push(row.slice(0, Math.max(0, maxChars)));
      }
      formatted.push("… (truncated)");
      break;
    }
    formatted.push(row);
    charCount += row.length + 1;
  }
  return formatted.join("\n");
}
