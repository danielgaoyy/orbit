import type { RichBlockView } from "@/lib/orbit-ipc";

export const CODE_BLOCK_COLLAPSE_THRESHOLD = 12;

export const RUNNABLE_SHELL_LANGUAGES: ReadonlySet<string> = new Set([
  "sh",
  "bash",
  "zsh",
  "shell",
  "fish",
  "powershell",
  "pwsh",
  "cmd",
]);

const DESTRUCTIVE_SHELL_RE =
  /(?:^|[;&|]\s*)(?:sudo\s+|rm\s+-[a-zA-Z]*[rf]|mkfs\b|dd\s+if=|shutdown\b|reboot\b|halt\b|poweroff\b|chmod\s+-R\s+777\b|chown\s+-R\b|git\s+reset\s+--hard\b|git\s+clean\s+-[a-zA-Z]*f|git\s+push\s+(?:--force|-f)\b|:\(\)\s*\{)|>\s*\/dev\/(?:sd|nvme|disk|hd)/i;

export interface RunnableShellSnippet {
  runnable: boolean;
  cleanedCommand: string;
  isMultiline: boolean;
  isPotentiallyDestructive: boolean;
  safeToAutoExecute: boolean;
}

/**
 * Classify a Markdown fenced code block to determine whether it can be run in Orbit's Terminal,
 * clean leading single-line `$ ` / `% ` prompt prefixes, and gate multi-line or destructive
 * commands to safe paste-without-Enter mode.
 */
export function classifyRunnableShellSnippet(
  language: string,
  rawCode: string,
): RunnableShellSnippet {
  const normLang = language.trim().toLowerCase().replace(/^\./, "").split(/[\s,]+/)[0] ?? "";
  if (!RUNNABLE_SHELL_LANGUAGES.has(normLang)) {
    return {
      runnable: false,
      cleanedCommand: "",
      isMultiline: false,
      isPotentiallyDestructive: false,
      safeToAutoExecute: false,
    };
  }

  const lines = rawCode
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((l) => l.trimEnd());
  while (lines.length > 0 && lines[0].trim().length === 0) {
    lines.shift();
  }
  while (lines.length > 0 && lines[lines.length - 1].trim().length === 0) {
    lines.pop();
  }
  if (lines.length === 0) {
    return {
      runnable: false,
      cleanedCommand: "",
      isMultiline: false,
      isPotentiallyDestructive: false,
      safeToAutoExecute: false,
    };
  }

  // Strip decorative prompt prefixes (`$ cmd` or `% cmd`) only when every non-comment line uses it
  const strippedLines =
    lines.length === 1
      ? [lines[0].replace(/^\s*(?:\$|%)\s+(?=\S)/, "").trim()]
      : lines;

  const cleanedCommand = strippedLines.join("\n").trim();
  if (!cleanedCommand || cleanedCommand.startsWith("#")) {
    return {
      runnable: false,
      cleanedCommand: "",
      isMultiline: false,
      isPotentiallyDestructive: false,
      safeToAutoExecute: false,
    };
  }

  const isMultiline = cleanedCommand.includes("\n");
  const isPotentiallyDestructive = DESTRUCTIVE_SHELL_RE.test(cleanedCommand);
  return {
    runnable: true,
    cleanedCommand,
    isMultiline,
    isPotentiallyDestructive,
    safeToAutoExecute: !isMultiline && !isPotentiallyDestructive,
  };
}

export type ListItemBlock = Extract<RichBlockView, { kind: "list_item" }>;
export type ListGroupKind = "ordered" | "unordered" | "checklist";

export type GroupedRenderNode =
  | { kind: "block"; block: RichBlockView; keyIndex: number }
  | { kind: "list_group"; listKind: ListGroupKind; items: ListItemBlock[]; keyIndex: number };

export interface NestedListItemNode {
  item: ListItemBlock;
  children: NestedListGroupNode[];
}

export interface NestedListGroupNode {
  listKind: ListGroupKind;
  depth: number;
  startIndex: number;
  items: NestedListItemNode[];
}

export function classifyListItem(item: ListItemBlock): ListGroupKind {
  if (item.checked !== null) return "checklist";
  return item.ordered ? "ordered" : "unordered";
}

/**
 * Group consecutive Markdown `RichBlockView` items into render nodes.
 * Nested list items (`depth > 0`) stay attached to the preceding `list_group` even when their
 * list kind differs from the parent (e.g. unordered bullets inside a numbered list), preserving
 * true hierarchical `<ul>`/`<ol>` nesting.
 */
export function groupMarkdownBlocks(blocks: RichBlockView[]): GroupedRenderNode[] {
  const nodes: GroupedRenderNode[] = [];
  for (let i = 0; i < blocks.length; i += 1) {
    const block = blocks[i];
    if (block.kind === "list_item") {
      const listKind = classifyListItem(block);
      const depth = Math.max(0, block.depth ?? 0);
      const prev = nodes[nodes.length - 1];
      if (prev && prev.kind === "list_group" && (depth > 0 || prev.listKind === listKind)) {
        prev.items.push(block);
      } else {
        nodes.push({ kind: "list_group", listKind, items: [block], keyIndex: i });
      }
    } else {
      nodes.push({ kind: "block", block, keyIndex: i });
    }
  }
  return nodes;
}

/**
 * Convert a flat sequence of depth-annotated `ListItemBlock`s into a nested forest of
 * `NestedListGroupNode`s ready for semantic `<ul>`/`<ol>` rendering.
 */
export function buildNestedListForest(
  items: readonly ListItemBlock[],
): NestedListGroupNode[] {
  const roots: NestedListGroupNode[] = [];
  const stack: NestedListGroupNode[] = [];

  for (const item of items) {
    const depth = Math.min(8, Math.max(0, item.depth ?? 0));
    const listKind = classifyListItem(item);

    while (stack.length > 0 && stack[stack.length - 1].depth > depth) {
      stack.pop();
    }

    const top = stack[stack.length - 1];
    if (top && top.depth === depth && top.listKind === listKind) {
      top.items.push({ item, children: [] });
      continue;
    }

    if (top && top.depth === depth && top.listKind !== listKind) {
      stack.pop();
    }

    const parentGroup = stack[stack.length - 1];
    const newGroup: NestedListGroupNode = {
      listKind,
      depth,
      startIndex: item.ordered ? Math.max(1, item.index || 1) : 1,
      items: [{ item, children: [] }],
    };

    if (parentGroup && parentGroup.items.length > 0) {
      parentGroup.items[parentGroup.items.length - 1].children.push(newGroup);
    } else {
      roots.push(newGroup);
    }
    stack.push(newGroup);
  }

  return roots;
}
