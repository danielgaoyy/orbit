import type { TimelineItemView } from "@/lib/orbit-ipc";

export function stripWorkspacePrefix(filePath: string, cwd?: string | null): string {
  if (!filePath) return filePath;
  const cleanCwd = (cwd ?? "").replace(/\/+$/, "");
  if (cleanCwd && cleanCwd !== "~" && filePath.startsWith(`${cleanCwd}/`)) {
    return filePath.slice(cleanCwd.length + 1);
  }
  return filePath;
}

export function basenameOfPath(filePath: string): string {
  if (!filePath) return "";
  const trimmed = filePath.replace(/[/\\]+$/, "");
  const parts = trimmed.split(/[/\\]/);
  return parts[parts.length - 1] ?? trimmed;
}

export function formatDurationCompact(durationMs: number): string {
  const totalSeconds = Math.max(1, Math.round(durationMs / 1000));
  if (totalSeconds < 60) {
    return `${totalSeconds}s`;
  }
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes >= 60) {
    const hours = Math.floor(minutes / 60);
    const remMin = minutes % 60;
    return remMin > 0 ? `${hours}h ${remMin}m` : `${hours}h`;
  }
  return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
}

/**
 * Formats thinking duration into exact collapsed labels:
 * - `Thought for 6s` / `Thought for 1m 12s` / `Thought for 2m` when duration >= 1s
 * - `Thought process` (multiple blocks) or `Thought about it` (single block) when duration is unknown
 */
export function formatThoughtSummaryLabel(
  durationMs?: number | null,
  thoughtCount = 1,
): string {
  if (typeof durationMs === "number" && Number.isFinite(durationMs)) {
    const roundedSec = Math.round(durationMs / 1000);
    if (roundedSec >= 1) {
      return `Thought for ${formatDurationCompact(durationMs)}`;
    }
  }
  return thoughtCount > 1 ? "Thought process" : "Thought about it";
}

/**
 * Extracts a clean one-line headline from raw reasoning markdown (skipping code fences
 * and stripping markdown headings, bullets, blockquotes, links, and inline formatting).
 */
export function extractThinkingHeadline(rawText: string, preferLatest = false): string {
  if (!rawText) return "";
  const lines = rawText.split(/\r?\n/);
  const cleanedLines: string[] = [];
  let inFence = false;

  for (const rawLine of lines) {
    const trimmed = rawLine.trim();
    if (/^```/.test(trimmed)) {
      inFence = !inFence;
      continue;
    }
    if (inFence || !trimmed) continue;

    const plain = trimmed
      .replace(/^#{1,6}\s+/, "")
      .replace(/^(?:[-*+]|\d{1,3}\.)\s+/, "")
      .replace(/^>\s*/, "")
      .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/[*_`~]+/g, "")
      .replace(/\s+/g, " ")
      .trim();

    if (plain.length > 0) {
      cleanedLines.push(plain);
      if (!preferLatest) break;
    }
  }

  const chosen = preferLatest
    ? (cleanedLines[cleanedLines.length - 1] ?? "")
    : (cleanedLines[0] ?? "");
  return chosen.length > 88 ? `${chosen.slice(0, 87)}…` : chosen;
}

function humanizeIdentifier(raw: string): string {
  const clean = raw
    .trim()
    .replace(/[._-]+/g, " ")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/\s+/g, " ")
    .trim();
  if (!clean) return raw;
  return clean.charAt(0).toUpperCase() + clean.slice(1).toLowerCase();
}

function formatServerTitle(rawServer: string): string {
  const cleaned = rawServer
    .replace(/^plugin_[^_]+_/i, "")
    .replace(/[._-]+/g, " ")
    .trim();
  if (!cleaned) return rawServer;
  return cleaned
    .split(" ")
    .filter(Boolean)
    .map((part) => {
      if (part.length <= 3 && /^(?:gh|pr|ci|db|ui|ai|os|ip)$/i.test(part)) {
        return part.toUpperCase();
      }
      return part.charAt(0).toUpperCase() + part.slice(1);
    })
    .join(" ");
}

export interface ParsedGenericToolName {
  serverName: string | null;
  actionLabel: string;
  displayLabel: string;
}

export function parseGenericToolName(rawName: string): ParsedGenericToolName {
  const trimmed = rawName.trim();
  if (!trimmed) {
    return { serverName: null, actionLabel: "tool", displayLabel: "Tool" };
  }

  // `mcp__<server>__<tool>` convention
  if (trimmed.startsWith("mcp__")) {
    const parts = trimmed.slice(5).split("__").filter(Boolean);
    if (parts.length >= 2) {
      const serverName = formatServerTitle(parts[0]);
      const actionRaw = parts.slice(1).join(" ");
      const actionLabel = humanizeIdentifier(actionRaw).toLowerCase();
      return {
        serverName,
        actionLabel,
        displayLabel: `${serverName}: ${actionLabel}`,
      };
    }
  }

  // `<server>:<tool>` convention
  const colonIdx = trimmed.indexOf(":");
  if (colonIdx > 0 && colonIdx < trimmed.length - 1) {
    const serverName = formatServerTitle(trimmed.slice(0, colonIdx));
    const actionLabel = humanizeIdentifier(trimmed.slice(colonIdx + 1)).toLowerCase();
    return {
      serverName,
      actionLabel,
      displayLabel: `${serverName}: ${actionLabel}`,
    };
  }

  const humanized = humanizeIdentifier(trimmed);
  return {
    serverName: null,
    actionLabel: humanized.toLowerCase(),
    displayLabel: humanized,
  };
}

export type SearchSubkind = "web" | "glob" | "grep";

export function classifySearchTool(
  query: string,
  summaryTitle: string,
): SearchSubkind {
  const titleLower = summaryTitle.trim().toLowerCase();
  if (/\b(?:web|internet|google|bing|url)\b/.test(titleLower) || /^https?:\/\//i.test(query)) {
    return "web";
  }
  if (
    /\b(?:glob|find files|file pattern|list files)\b/.test(titleLower) ||
    (/^\*\*?\//.test(query.trim()) && !/\s/.test(query.trim()))
  ) {
    return "glob";
  }
  return "grep";
}

function isFileCreateToolCall(
  item: Extract<TimelineItemView, { item_type: "tool_call" }>,
): boolean {
  if (item.detail.kind !== "file_edit") return false;
  if (/^(?:create|created|write|wrote)\b/i.test(item.summary_title.trim())) {
    return true;
  }
  if (item.detail.unified_diff && /---\s+\/dev\/null/.test(item.detail.unified_diff)) {
    return true;
  }
  return false;
}

export interface ToolStepDescriptor {
  kind:
    | "file_read"
    | "file_create"
    | "file_edit"
    | "search"
    | "shell"
    | "subagent"
    | "generic";
  verb: string;
  target: string;
  secondaryMeta: string | null;
  openPath: string | null;
  additions: number;
  deletions: number;
  exitCode: number | null;
  isRunning: boolean;
  isFailed: boolean;
  isDenied: boolean;
  hasExpandableDetail: boolean;
}

export function describeToolStep(
  item: Extract<TimelineItemView, { item_type: "tool_call" }>,
  cwd?: string | null,
): ToolStepDescriptor {
  const isRunning = item.status === "running";
  const isFailed = item.status === "failed";
  const isDenied = item.status === "denied";

  const additions = item.diff_files.reduce((acc, f) => acc + f.additions, 0);
  const deletions = item.diff_files.reduce((acc, f) => acc + f.deletions, 0);
  const firstDiffLines = item.diff_files[0]?.lines ?? [];

  switch (item.detail.kind) {
    case "file_read": {
      const rel = stripWorkspacePrefix(item.detail.path, cwd) || "file";
      const range =
        item.detail.start_line && item.detail.end_line
          ? `L${item.detail.start_line}–${item.detail.end_line}`
          : item.detail.start_line
            ? `from L${item.detail.start_line}`
            : null;
      const verb = isDenied
        ? "Declined read"
        : isFailed
          ? "Couldn’t read"
          : isRunning
            ? "Reading"
            : "Read";
      return {
        kind: "file_read",
        verb,
        target: rel,
        secondaryMeta: range,
        openPath: item.detail.path || null,
        additions,
        deletions,
        exitCode: null,
        isRunning,
        isFailed,
        isDenied,
        hasExpandableDetail: false,
      };
    }

    case "file_edit": {
      const isCreate = isFileCreateToolCall(item);
      const rel = stripWorkspacePrefix(item.detail.path, cwd) || "file";
      const verb = isDenied
        ? "Declined edit"
        : isFailed
          ? isCreate
            ? "Failed to create"
            : "Failed to edit"
          : isRunning
            ? isCreate
              ? "Creating"
              : "Editing"
            : isCreate
              ? "Created"
              : "Edited";
      return {
        kind: isCreate ? "file_create" : "file_edit",
        verb,
        target: rel,
        secondaryMeta: null,
        openPath: item.detail.path || null,
        additions,
        deletions,
        exitCode: null,
        isRunning,
        isFailed,
        isDenied,
        hasExpandableDetail: firstDiffLines.length > 0 || Boolean(item.detail.unified_diff),
      };
    }

    case "search": {
      const subkind = classifySearchTool(item.detail.query, item.summary_title);
      let verb: string;
      if (subkind === "web") {
        verb = isFailed
          ? "Failed to search the web"
          : isRunning
            ? "Searching the web"
            : "Searched the web";
      } else if (subkind === "glob") {
        verb = isFailed
          ? "Failed to find files"
          : isRunning
            ? "Looking for files"
            : "Looked for files";
      } else {
        verb = isFailed
          ? "Search failed"
          : isRunning
            ? "Searching"
            : "Searched";
      }
      const scopeRel = item.detail.scope
        ? stripWorkspacePrefix(item.detail.scope, cwd)
        : null;
      return {
        kind: "search",
        verb,
        target: item.detail.query,
        secondaryMeta: scopeRel ? `in ${scopeRel}` : null,
        openPath: null,
        additions,
        deletions,
        exitCode: null,
        isRunning,
        isFailed,
        isDenied,
        hasExpandableDetail: false,
      };
    }

    case "shell": {
      const verb = isDenied
        ? "Stopped running the command"
        : isFailed
          ? "Command failed"
          : isRunning
            ? "Running"
            : "Ran";
      return {
        kind: "shell",
        verb,
        target: item.detail.command || "command",
        secondaryMeta: null,
        openPath: null,
        additions,
        deletions,
        exitCode: item.detail.exit_code,
        isRunning,
        isFailed,
        isDenied,
        hasExpandableDetail: Boolean(item.detail.command || item.detail.output_preview),
      };
    }

    case "subagent": {
      const verb = isDenied
        ? "Stopped subagent"
        : isFailed
          ? "Subagent failed"
          : isRunning
            ? "Running subagent"
            : "Ran subagent";
      return {
        kind: "subagent",
        verb,
        target: item.detail.role || item.detail.prompt_preview,
        secondaryMeta: null,
        openPath: null,
        additions,
        deletions,
        exitCode: null,
        isRunning,
        isFailed,
        isDenied,
        hasExpandableDetail: Boolean(item.detail.prompt_preview),
      };
    }

    case "generic": {
      const parsed = parseGenericToolName(item.detail.tool_name);
      const rawTitle = item.summary_title.trim();
      const target =
        rawTitle &&
        rawTitle.toLowerCase() !== item.detail.tool_name.trim().toLowerCase() &&
        rawTitle.toLowerCase() !== parsed.displayLabel.toLowerCase()
          ? rawTitle
          : "";
      const verb = isDenied
        ? `Declined ${parsed.displayLabel}`
        : isFailed
          ? `${parsed.displayLabel} failed`
          : parsed.displayLabel;
      const cleanArgs = (item.detail.arguments_json ?? "").trim();
      const hasNonEmptyArgs =
        cleanArgs.length > 0 && cleanArgs !== "{}" && cleanArgs !== "null";
      return {
        kind: "generic",
        verb,
        target,
        secondaryMeta: null,
        openPath: null,
        additions,
        deletions,
        exitCode: null,
        isRunning,
        isFailed,
        isDenied,
        hasExpandableDetail: hasNonEmptyArgs,
      };
    }
  }
}

export type FocusedDecisionTarget =
  | { kind: "permission"; requestId: string }
  | { kind: "question"; requestId: string }
  | null;

let activeFocusedDecisionTarget: FocusedDecisionTarget = null;

/** Returns the decision card currently focused in `StickyApprovalDock`. */
export function getFocusedDecisionTarget(): FocusedDecisionTarget {
  return activeFocusedDecisionTarget;
}

/** Updates the currently visible decision target for scoped keyboard approval shortcuts. */
export function setFocusedDecisionTarget(target: FocusedDecisionTarget): void {
  activeFocusedDecisionTarget = target;
}

export function isDenyDecision(dec: string): boolean {
  return /deny|reject|decline|cancel/i.test(dec);
}

export function isAlwaysDecision(dec: string): boolean {
  return /always|session|forever/i.test(dec);
}
