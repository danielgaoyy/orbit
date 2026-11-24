import type {
  LspCompletionItem,
  LspDiagnostic,
  LspTextEdit,
} from "./orbit-ipc.ts";

export type CodeMirrorLanguageKey =
  | "javascript"
  | "jsx"
  | "typescript"
  | "tsx"
  | "rust"
  | "python"
  | "json"
  | "css"
  | "html"
  | "markdown"
  | "go"
  | "cpp"
  | "yaml"
  | "plain";

export const CODE_MIRROR_LANGUAGE_LABELS: Record<string, string> = {
  rs: "Rust",
  rust: "Rust",
  ts: "TypeScript",
  typescript: "TypeScript",
  tsx: "TSX",
  js: "JavaScript",
  javascript: "JavaScript",
  jsx: "JSX",
  json: "JSON",
  toml: "TOML",
  yaml: "YAML",
  yml: "YAML",
  markdown: "Markdown",
  md: "Markdown",
  py: "Python",
  python: "Python",
  go: "Go",
  swift: "Swift",
  kotlin: "Kotlin",
  java: "Java",
  c: "C",
  cpp: "C++",
  sh: "Shell",
  css: "CSS",
  scss: "CSS",
  html: "HTML",
  xml: "HTML",
  sql: "SQL",
  dockerfile: "Dockerfile",
  makefile: "Makefile",
  text: "Plain Text",
  plain: "Plain Text",
};

export function formatEditorLanguageLabel(language: string): string {
  const key = language.trim().toLowerCase();
  return CODE_MIRROR_LANGUAGE_LABELS[key] ?? (key ? key.toUpperCase() : "Plain Text");
}

export function formatEditorLineEndingLabel(lineEnding?: string | null): "LF" | "CRLF" | "Mixed" {
  if (lineEnding === "crlf") return "CRLF";
  if (lineEnding === "mixed") return "Mixed";
  return "LF";
}

/**
 * Resolve a normalized CodeMirror 6 language pack key from Orbit's `WorkspaceFileView.language`
 * and/or file path extension.
 */
export function resolveCodeMirrorLanguageKey(
  language?: string | null,
  filePath?: string | null,
): CodeMirrorLanguageKey {
  const extFromPath = (() => {
    if (!filePath) return "";
    const normalized = filePath.replace(/\\/g, "/");
    const base = normalized.slice(normalized.lastIndexOf("/") + 1).toLowerCase();
    const dot = base.lastIndexOf(".");
    return dot > 0 ? base.slice(dot + 1) : "";
  })();

  const candidates = [language?.trim().toLowerCase() ?? "", extFromPath].filter(Boolean);
  for (const key of candidates) {
    switch (key) {
      case "tsx":
      case "typescriptreact":
        return "tsx";
      case "ts":
      case "mts":
      case "cts":
      case "typescript":
        return "typescript";
      case "jsx":
      case "javascriptreact":
        return "jsx";
      case "js":
      case "mjs":
      case "cjs":
      case "javascript":
        return "javascript";
      case "rs":
      case "rust":
        return "rust";
      case "py":
      case "pyi":
      case "python":
        return "python";
      case "json":
      case "jsonc":
        return "json";
      case "css":
      case "scss":
      case "less":
        return "css";
      case "html":
      case "htm":
      case "xml":
      case "svg":
      case "vue":
      case "svelte":
        return "html";
      case "md":
      case "mdx":
      case "markdown":
        return "markdown";
      case "go":
      case "golang":
        return "go";
      case "c":
      case "h":
      case "cc":
      case "cpp":
      case "cxx":
      case "hpp":
      case "hh":
      case "hxx":
      case "c++":
        return "cpp";
      case "yaml":
      case "yml":
        return "yaml";
      default:
        break;
    }
  }
  return "plain";
}

/**
 * Convert a 1-based `(line, utf16_col)` coordinate into a clamped UTF-16 string offset
 * compatible with CodeMirror 6 (`Text` uses UTF-16 code unit indexing just like JS strings and LSP).
 */
export function lineColToOffset(text: string, line1Based: number, col1Based: number): number {
  if (text.length === 0) return 0;
  const targetLine = Math.max(1, Math.floor(line1Based));
  const targetCol = Math.max(1, Math.floor(col1Based));

  let curLine = 1;
  let lineStart = 0;
  while (curLine < targetLine) {
    const nl = text.indexOf("\n", lineStart);
    if (nl === -1) {
      return text.length;
    }
    lineStart = nl + 1;
    curLine += 1;
  }

  const nextNl = text.indexOf("\n", lineStart);
  const rawLineEnd = nextNl === -1 ? text.length : nextNl;
  const lineEnd =
    rawLineEnd > lineStart && text.charCodeAt(rawLineEnd - 1) === 13 ? rawLineEnd - 1 : rawLineEnd;
  return Math.min(lineStart + (targetCol - 1), lineEnd);
}

/**
 * Convert a JS/CodeMirror UTF-16 offset into a 1-based `(line, col)` coordinate.
 */
export function offsetToLineCol(text: string, offset: number): { line: number; col: number } {
  const clamped = Math.max(0, Math.min(Math.floor(offset), text.length));
  let line = 1;
  let lastNewline = -1;
  for (let i = 0; i < clamped; i += 1) {
    if (text.charCodeAt(i) === 10) {
      line += 1;
      lastNewline = i;
    }
  }
  return { line, col: clamped - lastNewline };
}

export interface MappedCodeMirrorDiagnostic {
  from: number;
  to: number;
  severity: "error" | "warning" | "info" | "hint";
  message: string;
  source?: string;
  original: LspDiagnostic;
}

/**
 * Map Orbit's 1-based `LspDiagnostic[]` into clamped CodeMirror 6 lint diagnostic ranges.
 */
export function mapLspDiagnosticsToCodeMirror(
  docText: string,
  diagnostics: readonly LspDiagnostic[],
): MappedCodeMirrorDiagnostic[] {
  if (!diagnostics.length) return [];
  const docLen = docText.length;
  const mapped: MappedCodeMirrorDiagnostic[] = [];

  for (const d of diagnostics) {
    const rawFrom = lineColToOffset(docText, d.line, d.col);
    const rawTo = lineColToOffset(docText, Math.max(d.line, d.end_line), d.end_col);
    let from = Math.max(0, Math.min(rawFrom, docLen));
    let to = Math.max(from, Math.min(rawTo, docLen));

    // Ensure zero-width diagnostics still draw a visible underline squiggle on at least 1 char when possible
    if (to === from && docLen > 0) {
      if (from < docLen && docText.charCodeAt(from) !== 10) {
        to = from + 1;
      } else if (from > 0 && docText.charCodeAt(from - 1) !== 10) {
        from -= 1;
      }
    }

    const sourceLabel = [d.source, d.code].filter(Boolean).join(" ").trim();
    mapped.push({
      from,
      to,
      severity: d.severity,
      message: d.message,
      ...(sourceLabel ? { source: sourceLabel } : {}),
      original: d,
    });
  }
  return mapped;
}

export interface MappedCodeMirrorCompletion {
  label: string;
  type: string;
  detail?: string;
  info?: string;
  apply?: string;
  boost?: number;
}

const LSP_KIND_TO_CM_TYPE: Record<string, string> = {
  method: "method",
  function: "function",
  constructor: "class",
  field: "property",
  variable: "variable",
  class: "class",
  interface: "interface",
  module: "namespace",
  property: "property",
  enum: "enum",
  keyword: "keyword",
  snippet: "text",
  constant: "constant",
  struct: "type",
  type_parameter: "type",
  enum_member: "constant",
  text: "text",
  file: "text",
  folder: "namespace",
};

/**
 * Map normalized LSP completion items to CodeMirror `@codemirror/autocomplete` Completion entries.
 */
export function mapLspCompletionItems(
  items: readonly LspCompletionItem[],
): MappedCodeMirrorCompletion[] {
  return items.map((item, idx) => {
    const type = LSP_KIND_TO_CM_TYPE[item.kind] ?? "text";
    const apply =
      item.insert_text && item.insert_text !== item.label ? item.insert_text : undefined;
    // Preserve server `sort_text` order via gentle descending boost
    const boost = Math.max(-99, Math.min(99, 80 - idx));
    return {
      label: item.label,
      type,
      ...(item.detail ? { detail: item.detail } : {}),
      ...(item.documentation ? { info: item.documentation } : {}),
      ...(apply ? { apply } : {}),
      boost,
    };
  });
}

export interface CodeMirrorChangeSpec {
  from: number;
  to: number;
  insert: string;
}

/**
 * Convert non-overlapping 1-based `LspTextEdit[]` targeting the active document into
 * CodeMirror 6 transaction `ChangeSpec[]`.
 */
export function convertLspTextEditsToChanges(
  docText: string,
  edits: readonly LspTextEdit[],
): CodeMirrorChangeSpec[] {
  return edits
    .map((edit) => {
      const from = lineColToOffset(docText, edit.line, edit.col);
      const to = Math.max(from, lineColToOffset(docText, edit.end_line, edit.end_col));
      return { from, to, insert: edit.new_text };
    })
    .sort((a, b) => a.from - b.from || a.to - b.to);
}

/**
 * Extract the identifier token under or immediately touching `offset` (used to prefill `F2` Rename).
 */
export function extractIdentifierAtOffset(
  text: string,
  offset: number,
): { word: string; from: number; to: number } | null {
  if (!text) return null;
  const clamped = Math.max(0, Math.min(offset, text.length));
  const isIdent = (ch: string) => /[\p{L}\p{N}_$]/u.test(ch);

  let anchor = clamped;
  if (anchor < text.length && !isIdent(text[anchor])) {
    if (anchor > 0 && isIdent(text[anchor - 1])) {
      anchor -= 1;
    } else {
      return null;
    }
  } else if (anchor === text.length) {
    if (anchor > 0 && isIdent(text[anchor - 1])) {
      anchor -= 1;
    } else {
      return null;
    }
  }

  let from = anchor;
  while (from > 0 && isIdent(text[from - 1])) {
    from -= 1;
  }
  let to = anchor + 1;
  while (to < text.length && isIdent(text[to])) {
    to += 1;
  }
  const word = text.slice(from, to);
  return word ? { word, from, to } : null;
}

export interface InlineAiEditPromptParams {
  filePath: string;
  language: string;
  startLine: number;
  endLine: number;
  selectedCode: string;
  surroundingContext?: string;
  instruction: string;
}

export interface BuiltInlineAiEditPrompt {
  wirePrompt: string;
  displayPrompt: string;
  rangeLabel: string;
}

/**
 * Build a focused, unambiguous inline `Cmd+K` AI edit prompt carrying exact file path,
 * 1-based line range, fenced code excerpt, and user instruction.
 */
export function buildInlineAiEditPrompt(
  params: InlineAiEditPromptParams,
): BuiltInlineAiEditPrompt {
  const cleanFile = params.filePath.trim();
  const cleanInstruction = params.instruction.trim();
  const start = Math.max(1, Math.floor(params.startLine));
  const end = Math.max(start, Math.floor(params.endLine));
  const rangeLabel = start === end ? `L${start}` : `L${start}-L${end}`;
  const fenceLang = params.language.trim().toLowerCase().replace(/[^a-z0-9_+-]/g, "");
  const codeBody = params.selectedCode.length > 0 ? params.selectedCode : (params.surroundingContext ?? "");
  const hasSelection = params.selectedCode.length > 0;

  const lines: string[] = [
    `Edit \`${cleanFile}\` (${rangeLabel}): ${cleanInstruction}`,
    "",
    hasSelection
      ? `Target selection (\`${cleanFile}:${rangeLabel}\`):`
      : `Cursor context (\`${cleanFile}:${rangeLabel}\`):`,
    `\`\`\`${fenceLang}`,
    codeBody,
    "```",
    "",
    "Apply this focused edit directly in the file, keeping unrelated code unchanged.",
  ];

  const displayPrompt = `Inline edit \`${cleanFile}:${rangeLabel}\`: ${cleanInstruction}`;

  return {
    wirePrompt: lines.join("\n"),
    displayPrompt,
    rangeLabel,
  };
}

export type HoverContentBlock =
  | { kind: "code"; language: string; code: string }
  | { kind: "divider" }
  | { kind: "paragraph"; text: string };

/**
 * Parse normalized LSP hover Markdown/plaintext into safe structured blocks for XSS-free DOM rendering.
 */
export function parseHoverContentBlocks(raw: string): HoverContentBlock[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];

  const blocks: HoverContentBlock[] = [];
  const lines = trimmed.split(/\r?\n/);
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    const fenceMatch = line.match(/^```([a-zA-Z0-9_+-]*)\s*$/);
    if (fenceMatch) {
      const lang = fenceMatch[1] ?? "";
      const codeLines: string[] = [];
      i += 1;
      while (i < lines.length && !/^```\s*$/.test(lines[i])) {
        codeLines.push(lines[i]);
        i += 1;
      }
      if (i < lines.length) i += 1;
      const code = codeLines.join("\n").trimEnd();
      if (code) {
        blocks.push({ kind: "code", language: lang, code });
      }
      continue;
    }

    if (/^\s*---+\s*$/.test(line)) {
      if (blocks.length > 0 && blocks[blocks.length - 1]?.kind !== "divider") {
        blocks.push({ kind: "divider" });
      }
      i += 1;
      continue;
    }

    if (!line.trim()) {
      i += 1;
      continue;
    }

    const paraLines: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() !== "" &&
      !/^```/.test(lines[i]) &&
      !/^\s*---+\s*$/.test(lines[i])
    ) {
      paraLines.push(lines[i]);
      i += 1;
    }
    const text = paraLines.join("\n").trim();
    if (text) {
      blocks.push({ kind: "paragraph", text });
    }
  }

  while (blocks.length > 0 && blocks[blocks.length - 1]?.kind === "divider") {
    blocks.pop();
  }
  return blocks;
}
