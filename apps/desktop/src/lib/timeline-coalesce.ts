import type { DiffFileView, TimelineItemView } from "@/lib/orbit-ipc";

export interface TurnFileDiffSummary {
  path: string;
  additions: number;
  deletions: number;
}

export interface TurnWorkCounts {
  editedFileCount: number;
  createdFileCount: number;
  commandCount: number;
  readFileCount: number;
  searchCount: number;
  subagentCount: number;
  mcpServerCount: number;
  otherToolCount: number;
  thoughtCount: number;
  narrationCount: number;
  actionSteps: number;
  totalSteps: number;
  failedCount: number;
  deniedCount: number;
  unrecoveredFailureCount: number;
  additions: number;
  deletions: number;
}

export interface WorkSummaryResult {
  counts: TurnWorkCounts;
  /** Settled collapsed summary label (e.g. `Edited 2 files, ran a command`, `Read omni-composer.tsx`, `Thought for 6s`). */
  label: string;
  /** Active streaming summary label while steps are still in flight (e.g. `Reading omni-composer.tsx…`). */
  liveLabel: string;
  /** Whether the primary `label` already embeds failure wording (e.g. `Command failed`). */
  labelIncludesFailure: boolean;
}

export type TurnLiveSegment =
  | {
      kind: "step_group";
      key: string;
      firstSeq: number;
      items: TimelineItemView[];
      summary: WorkSummaryResult;
      /** True when subsequent prose or another segment has already started after this step batch. */
      hasFollowingContent: boolean;
      /** True if any tool call inside this group is currently `running`. */
      hasRunningStep: boolean;
    }
  | {
      kind: "assistant_message";
      key: string;
      item: Extract<TimelineItemView, { item_type: "assistant_message" }>;
    }
  | {
      kind: "notice";
      key: string;
      item: Extract<TimelineItemView, { item_type: "notice" }>;
    }
  | {
      kind: "compaction";
      key: string;
      label: string;
    };

export interface StageTurn {
  key: string;
  turnIndex: number;
  anchorSeq: number;
  userPrompt: Extract<TimelineItemView, { item_type: "user_prompt" }> | null;
  isCompactionCommand: boolean;
  /** Intermediate steps prior to `terminalReply` (eligible for pre-reply auto-fold). */
  foldedSteps: TimelineItemView[];
  /** Aggregated counts of work performed inside `foldedSteps`. */
  workCounts: TurnWorkCounts;
  /** Settled summary sentence for `foldedSteps` (`Edited 2 files, ran a command`). */
  workSummaryLabel: string;
  /** Whether `workSummaryLabel` already states failure (`Command failed`). */
  workSummaryIncludesFailure: boolean;
  /** Final synthesis reply of the turn, kept visible when pre-reply work folds. */
  terminalReply: Extract<TimelineItemView, { item_type: "assistant_message" }> | null;
  /** Notices (warnings/errors) surfaced unconditionally so system alerts are never buried. */
  notices: Extract<TimelineItemView, { item_type: "notice" }>[];
  /** Chronological segments used while the turn has not yet reached a terminal reply (or when unfolded). */
  liveSegments: TurnLiveSegment[];
  /** Deduplicated per-file diff stats touched during this turn. */
  turnDiffs: TurnFileDiffSummary[];
  /** Whether any tool call in this turn failed or was denied. */
  hasError: boolean;
  /** Whether the turn ended on an unrecovered tool failure/denial without subsequent success. */
  hasUnrecoveredError: boolean;
  /** Whether any tool call or message in this turn is currently in flight. */
  hasActiveItem: boolean;
}

export interface ProcessedTimelineStage {
  turns: StageTurn[];
  pendingPermissions: Extract<TimelineItemView, { item_type: "permission_request" }>[];
  pendingQuestions: Extract<TimelineItemView, { item_type: "question_request" }>[];
  sessionTurnDiffs: DiffFileView[];
}

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

export function isFileCreateToolCall(
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

function summarizeSingleToolCall(
  tc: Extract<TimelineItemView, { item_type: "tool_call" }>,
): { label: string; includesFailure: boolean } {
  if (tc.status === "denied") {
    return {
      label: tc.detail.kind === "shell" ? "Stopped running the command" : "You declined",
      includesFailure: true,
    };
  }

  const failed = tc.status === "failed";
  const withFailedSuffix = (base: string) =>
    failed ? { label: `${base} (failed)`, includesFailure: true } : { label: base, includesFailure: false };

  switch (tc.detail.kind) {
    case "file_read": {
      const name = basenameOfPath(tc.detail.path);
      return withFailedSuffix(name ? `Read ${name}` : "Read a file");
    }
    case "file_edit": {
      const isCreate = isFileCreateToolCall(tc);
      const name = basenameOfPath(tc.detail.path);
      const prefix = isCreate ? "Created" : "Edited";
      return withFailedSuffix(name ? `${prefix} ${name}` : `${prefix} a file`);
    }
    case "search": {
      const subkind = classifySearchTool(tc.detail.query, tc.summary_title);
      if (subkind === "web") {
        return failed
          ? { label: "Failed to search the web", includesFailure: true }
          : { label: "Searched the web", includesFailure: false };
      }
      if (subkind === "glob") {
        return withFailedSuffix("Looked for files");
      }
      return withFailedSuffix("Searched files");
    }
    case "shell": {
      if (failed) {
        return { label: "Command failed", includesFailure: true };
      }
      return { label: "Ran a command", includesFailure: false };
    }
    case "subagent": {
      return withFailedSuffix("Ran an agent");
    }
    case "generic": {
      const parsed = parseGenericToolName(tc.detail.tool_name);
      if (parsed.serverName) {
        return withFailedSuffix(`Used ${parsed.serverName} integration`);
      }
      if (parsed.displayLabel && parsed.displayLabel !== "Tool") {
        return withFailedSuffix(`Used ${parsed.displayLabel}`);
      }
      return failed
        ? { label: "Failed", includesFailure: true }
        : { label: "Used a tool", includesFailure: false };
    }
  }
}

function buildLiveStepLabel(item: TimelineItemView | undefined): string {
  if (!item) return "Working…";
  if (item.item_type === "assistant_thinking") {
    const headline = extractThinkingHeadline(item.text, true);
    return headline || "Thinking…";
  }
  if (item.item_type !== "tool_call") {
    return "Working…";
  }

  switch (item.detail.kind) {
    case "file_read": {
      const name = basenameOfPath(item.detail.path);
      return name ? `Reading ${name}…` : "Reading a file…";
    }
    case "file_edit": {
      const isCreate = isFileCreateToolCall(item);
      const name = basenameOfPath(item.detail.path);
      const verb = isCreate ? "Creating" : "Editing";
      return name ? `${verb} ${name}…` : `${verb} a file…`;
    }
    case "search": {
      const subkind = classifySearchTool(item.detail.query, item.summary_title);
      if (subkind === "web") return "Searching the web…";
      if (subkind === "glob") return "Looking for files…";
      return "Searching files…";
    }
    case "shell":
      return "Running a command…";
    case "subagent":
      return "Running subagent…";
    case "generic": {
      const parsed = parseGenericToolName(item.detail.tool_name);
      if (parsed.serverName) return `Using ${parsed.serverName}…`;
      return parsed.displayLabel ? `Using ${parsed.displayLabel}…` : "Using a tool…";
    }
  }
}

interface SummaryCategoryBucket {
  key: string;
  label: string;
  tier: number;
  rawCallCount: number;
  firstOrder: number;
}

function lowercaseClauseLead(clause: string): string {
  if (!clause) return clause;
  return clause.charAt(0).toLowerCase() + clause.slice(1);
}

/**
 * Summarizes a contiguous stretch of agent steps into a concise 1-line summary header:
 * - Single tool call: specific target phrasing (`Read omni-composer.tsx`, `Edited stream-stage.tsx`, `Ran a command`, `Command failed`)
 * - Pure reasoning: `Thought for Xs` (or `Thought about it` / `Thought process`)
 * - Multi-tool batch: deduplicates paths per category, sorts by semantic priority tier
 *   (Edits/Creates -> Agents/Integrations -> Reads/Searches -> Commands/Generic),
 *   joins up to 3 clauses with `, `, and collapses 4+ categories into `{top2}, and N more steps`.
 */
export function summarizeWorkItems(
  items: TimelineItemView[],
  options?: {
    thoughtDurationMs?: number | null;
  },
): WorkSummaryResult {
  const toolCalls: Extract<TimelineItemView, { item_type: "tool_call" }>[] = [];
  let thoughtCount = 0;
  let narrationCount = 0;
  let permissionCount = 0;
  let questionCount = 0;
  let additions = 0;
  let deletions = 0;

  for (const it of items) {
    if (it.item_type === "assistant_thinking") {
      thoughtCount += 1;
    } else if (it.item_type === "assistant_message") {
      if (it.text.trim().length > 0) narrationCount += 1;
    } else if (it.item_type === "permission_request") {
      permissionCount += 1;
    } else if (it.item_type === "question_request") {
      questionCount += 1;
    } else if (it.item_type === "tool_call") {
      toolCalls.push(it);
      for (const df of it.diff_files) {
        additions += df.additions;
        deletions += df.deletions;
      }
    }
  }

  let failedCount = 0;
  let deniedCount = 0;
  let trailingFailureStreak = 0;
  for (const tc of toolCalls) {
    if (tc.status === "failed") {
      failedCount += 1;
      trailingFailureStreak += 1;
    } else if (tc.status === "denied") {
      deniedCount += 1;
      trailingFailureStreak += 1;
    } else if (tc.status === "completed") {
      // Subsequent successful tool execution means earlier failures in the batch were recovered
      trailingFailureStreak = 0;
    }
  }

  const allFailed =
    toolCalls.length > 0 && failedCount + deniedCount === toolCalls.length;
  const unrecoveredFailureCount = allFailed ? failedCount + deniedCount : trailingFailureStreak;

  const editedPaths = new Set<string>();
  let editPathless = 0;
  let editCalls = 0;

  const createdPaths = new Set<string>();
  let createPathless = 0;
  let createCalls = 0;

  const readPaths = new Set<string>();
  let readPathless = 0;
  let readCalls = 0;

  let commandCount = 0;
  let grepCount = 0;
  let globCount = 0;
  let webSearchCount = 0;
  let subagentCount = 0;
  const mcpServers = new Map<string, number>();
  let mcpCallCount = 0;
  let otherToolCount = 0;

  const firstSeen = new Map<string, number>();
  const markSeen = (key: string) => {
    if (!firstSeen.has(key)) firstSeen.set(key, firstSeen.size);
  };

  for (const tc of toolCalls) {
    switch (tc.detail.kind) {
      case "file_edit": {
        if (isFileCreateToolCall(tc)) {
          markSeen("create");
          createCalls += 1;
          if (tc.detail.path) createdPaths.add(tc.detail.path);
          else createPathless += 1;
        } else {
          markSeen("edit");
          editCalls += 1;
          if (tc.detail.path) editedPaths.add(tc.detail.path);
          else editPathless += 1;
        }
        break;
      }
      case "file_read": {
        markSeen("read");
        readCalls += 1;
        if (tc.detail.path) readPaths.add(tc.detail.path);
        else readPathless += 1;
        break;
      }
      case "search": {
        const subkind = classifySearchTool(tc.detail.query, tc.summary_title);
        if (subkind === "web") {
          markSeen("search_web");
          webSearchCount += 1;
        } else if (subkind === "glob") {
          markSeen("look_files");
          globCount += 1;
        } else {
          markSeen("search_files");
          grepCount += 1;
        }
        break;
      }
      case "shell":
        markSeen("shell");
        commandCount += 1;
        break;
      case "subagent":
        markSeen("subagent");
        subagentCount += 1;
        break;
      case "generic": {
        const parsed = parseGenericToolName(tc.detail.tool_name);
        if (parsed.serverName) {
          markSeen("mcp");
          mcpCallCount += 1;
          mcpServers.set(parsed.serverName, (mcpServers.get(parsed.serverName) ?? 0) + 1);
        } else {
          markSeen("other");
          otherToolCount += 1;
        }
        break;
      }
    }
  }

  const editedFileCount = editedPaths.size + editPathless;
  const createdFileCount = createdPaths.size + createPathless;
  const readFileCount = readPaths.size + readPathless;
  const searchCount = grepCount + globCount + webSearchCount;
  const mcpServerCount = mcpServers.size;
  const actionSteps = toolCalls.length;
  const totalSteps = actionSteps + thoughtCount + permissionCount + questionCount;

  const activeTail =
    [...items]
      .reverse()
      .find((it) => it.item_type === "tool_call" && it.status === "running") ??
    items[items.length - 1];
  const liveLabel = buildLiveStepLabel(activeTail);

  let label = "";
  let labelIncludesFailure = false;

  if (toolCalls.length === 0) {
    if (thoughtCount > 0) {
      label = formatThoughtSummaryLabel(options?.thoughtDurationMs, thoughtCount);
    } else if (questionCount > 0) {
      label = questionCount === 1 ? "Asked a question" : `Asked ${questionCount} questions`;
    } else if (permissionCount > 0) {
      label = "Checked permissions";
    } else if (narrationCount > 0) {
      label = `${narrationCount} ${narrationCount === 1 ? "note" : "notes"}`;
    } else {
      label = "Completed steps";
    }
  } else if (toolCalls.length === 1) {
    const single = summarizeSingleToolCall(toolCalls[0]);
    label = single.label;
    labelIncludesFailure = single.includesFailure;
  } else if (deniedCount === toolCalls.length) {
    label = "You declined";
    labelIncludesFailure = true;
  } else {
    const activeCategoryCount = firstSeen.size;
    const isSoleCategory = activeCategoryCount === 1;
    const buckets: SummaryCategoryBucket[] = [];

    if (editedFileCount > 0) {
      const soleName =
        isSoleCategory && editedPaths.size === 1
          ? basenameOfPath(Array.from(editedPaths)[0] ?? "")
          : "";
      buckets.push({
        key: "edit",
        label: soleName
          ? `Edited ${soleName}`
          : editedFileCount === 1
            ? "Edited a file"
            : `Edited ${editedFileCount} files`,
        tier: 1,
        rawCallCount: editCalls,
        firstOrder: firstSeen.get("edit") ?? 0,
      });
    }

    if (createdFileCount > 0) {
      const soleName =
        isSoleCategory && createdPaths.size === 1
          ? basenameOfPath(Array.from(createdPaths)[0] ?? "")
          : "";
      buckets.push({
        key: "create",
        label: soleName
          ? `Created ${soleName}`
          : createdFileCount === 1
            ? "Created a file"
            : `Created ${createdFileCount} files`,
        tier: 1,
        rawCallCount: createCalls,
        firstOrder: firstSeen.get("create") ?? 0,
      });
    }

    if (subagentCount > 0) {
      buckets.push({
        key: "subagent",
        label: subagentCount === 1 ? "Ran an agent" : `Ran ${subagentCount} agents`,
        tier: 2,
        rawCallCount: subagentCount,
        firstOrder: firstSeen.get("subagent") ?? 0,
      });
    }

    if (mcpServerCount > 0) {
      const firstServer = Array.from(mcpServers.keys())[0] ?? "connector";
      buckets.push({
        key: "mcp",
        label:
          mcpServerCount === 1
            ? `Used ${firstServer} integration`
            : `Used ${mcpServerCount} integrations`,
        tier: 2,
        rawCallCount: mcpCallCount,
        firstOrder: firstSeen.get("mcp") ?? 0,
      });
    }

    if (readFileCount > 0) {
      const soleName =
        isSoleCategory && readPaths.size === 1
          ? basenameOfPath(Array.from(readPaths)[0] ?? "")
          : "";
      buckets.push({
        key: "read",
        label: soleName
          ? `Read ${soleName}`
          : readFileCount === 1
            ? "Read a file"
            : `Read ${readFileCount} files`,
        tier: 3,
        rawCallCount: readCalls,
        firstOrder: firstSeen.get("read") ?? 0,
      });
    }

    if (grepCount > 0) {
      buckets.push({
        key: "search_files",
        label: "Searched files",
        tier: 3,
        rawCallCount: grepCount,
        firstOrder: firstSeen.get("search_files") ?? 0,
      });
    }

    if (globCount > 0) {
      buckets.push({
        key: "look_files",
        label: "Looked for files",
        tier: 3,
        rawCallCount: globCount,
        firstOrder: firstSeen.get("look_files") ?? 0,
      });
    }

    if (webSearchCount > 0) {
      buckets.push({
        key: "search_web",
        label: "Searched the web",
        tier: 3,
        rawCallCount: webSearchCount,
        firstOrder: firstSeen.get("search_web") ?? 0,
      });
    }

    if (commandCount > 0) {
      buckets.push({
        key: "shell",
        label: commandCount === 1 ? "Ran a command" : `Ran ${commandCount} commands`,
        tier: 4,
        rawCallCount: commandCount,
        firstOrder: firstSeen.get("shell") ?? 0,
      });
    }

    if (otherToolCount > 0) {
      buckets.push({
        key: "other",
        label: otherToolCount === 1 ? "Used a tool" : `Used ${otherToolCount} tools`,
        tier: 4,
        rawCallCount: otherToolCount,
        firstOrder: firstSeen.get("other") ?? 0,
      });
    }

    buckets.sort(
      (a, b) =>
        a.tier - b.tier ||
        b.rawCallCount - a.rawCallCount ||
        a.firstOrder - b.firstOrder,
    );

    const visibleLimit = buckets.length > 3 ? 2 : 3;
    const shown = buckets.slice(0, visibleLimit);
    const hiddenSteps = buckets
      .slice(visibleLimit)
      .reduce((sum, b) => sum + b.rawCallCount, 0);

    const formattedShown = shown
      .map((b, idx) => (idx === 0 ? b.label : lowercaseClauseLead(b.label)))
      .join(", ");

    if (shown.length > 0 && hiddenSteps > 0) {
      label = `${formattedShown}, and ${hiddenSteps} more ${hiddenSteps === 1 ? "step" : "steps"}`;
    } else {
      label = formattedShown || `Used ${toolCalls.length} tools`;
    }
  }

  return {
    counts: {
      editedFileCount,
      createdFileCount,
      commandCount,
      readFileCount,
      searchCount,
      subagentCount,
      mcpServerCount,
      otherToolCount,
      thoughtCount,
      narrationCount,
      actionSteps,
      totalSteps,
      failedCount,
      deniedCount,
      unrecoveredFailureCount,
      additions,
      deletions,
    },
    label,
    liveLabel,
    labelIncludesFailure,
  };
}

function isStepRowItem(item: TimelineItemView): boolean {
  return (
    item.item_type === "assistant_thinking" ||
    item.item_type === "tool_call" ||
    item.item_type === "permission_request" ||
    item.item_type === "question_request"
  );
}

function buildLiveSegments(bodyItems: TimelineItemView[]): TurnLiveSegment[] {
  const rawSegments: TurnLiveSegment[] = [];
  let stepBuf: TimelineItemView[] = [];

  const flushSteps = (hasFollowingContent: boolean) => {
    if (stepBuf.length === 0) return;
    const firstSeq = stepBuf[0].seq;
    const hasRunningStep = stepBuf.some(
      (it) => it.item_type === "tool_call" && it.status === "running",
    );
    rawSegments.push({
      kind: "step_group",
      key: `steps-${firstSeq}`,
      firstSeq,
      items: stepBuf,
      summary: summarizeWorkItems(stepBuf),
      hasFollowingContent,
      hasRunningStep,
    });
    stepBuf = [];
  };

  for (const it of bodyItems) {
    if (isStepRowItem(it)) {
      stepBuf.push(it);
      continue;
    }

    if (it.item_type === "assistant_message") {
      flushSteps(true);
      rawSegments.push({
        kind: "assistant_message",
        key: `msg-${it.item_id}-${it.seq}`,
        item: it,
      });
    } else if (it.item_type === "notice") {
      if (/compact/i.test(it.message)) {
        flushSteps(true);
        rawSegments.push({
          kind: "compaction",
          key: `compact-${it.seq}`,
          label: it.message,
        });
      } else {
        flushSteps(false);
        rawSegments.push({
          kind: "notice",
          key: `notice-${it.seq}`,
          item: it,
        });
      }
    }
  }

  flushSteps(false);
  return rawSegments;
}

function finalizeTurn(
  turnIndex: number,
  userPrompt: Extract<TimelineItemView, { item_type: "user_prompt" }> | null,
  bodyItems: TimelineItemView[],
): StageTurn {
  const anchorSeq = userPrompt?.seq ?? bodyItems[0]?.seq ?? turnIndex;
  const key = userPrompt ? `turn-${userPrompt.message_id}` : `turn-init-${anchorSeq}`;
  const isCompactionCommand = Boolean(
    userPrompt &&
      (/^\s*\/compact\b/i.test(userPrompt.text.trim()) ||
        (userPrompt.badges?.some((b) => b.kind === "slash" && b.label === "/compact") &&
          userPrompt.text.trim().length === 0)),
  );

  const notices: Extract<TimelineItemView, { item_type: "notice" }>[] = [];
  const nonNoticeBody: TimelineItemView[] = [];
  let hasError = false;
  let hasActiveItem = false;

  const diffByPath = new Map<string, TurnFileDiffSummary>();

  for (const it of bodyItems) {
    if (it.item_type === "tool_call") {
      if (it.status === "running") hasActiveItem = true;
      if (it.status === "failed" || it.status === "denied") hasError = true;
      for (const df of it.diff_files) {
        const prev = diffByPath.get(df.path);
        if (prev) {
          prev.additions += df.additions;
          prev.deletions += df.deletions;
        } else {
          diffByPath.set(df.path, {
            path: df.path,
            additions: df.additions,
            deletions: df.deletions,
          });
        }
      }
      if (it.detail.kind === "file_edit" && !diffByPath.has(it.detail.path)) {
        diffByPath.set(it.detail.path, {
          path: it.detail.path,
          additions: 0,
          deletions: 0,
        });
      }
    } else if (it.item_type === "assistant_message" && it.streaming) {
      hasActiveItem = true;
    }

    if (it.item_type === "notice") {
      if (!/compact/i.test(it.message)) {
        notices.push(it);
      }
      continue;
    }

    nonNoticeBody.push(it);
  }

  // Identify terminal assistant reply: the trailing `assistant_message` when no step follows it
  let terminalReplyIndex = -1;
  for (let i = nonNoticeBody.length - 1; i >= 0; i -= 1) {
    const candidate = nonNoticeBody[i];
    if (candidate.item_type === "assistant_message") {
      terminalReplyIndex = i;
      break;
    }
    // If a tool call or thinking step occurred *after* the last assistant message,
    // there is no final synthesis reply yet — keep steps in live segment view.
    if (isStepRowItem(candidate)) {
      break;
    }
  }

  const candidateFolded =
    terminalReplyIndex > 0
      ? nonNoticeBody.slice(0, terminalReplyIndex)
      : terminalReplyIndex === -1
        ? nonNoticeBody
        : [];

  const foldedHasRunningTool = candidateFolded.some(
    (it) => it.item_type === "tool_call" && it.status === "running",
  );

  const terminalReply =
    terminalReplyIndex >= 0 && !foldedHasRunningTool
      ? (nonNoticeBody[terminalReplyIndex] as Extract<
          TimelineItemView,
          { item_type: "assistant_message" }
        >)
      : null;

  const foldedSteps =
    terminalReply !== null
      ? nonNoticeBody.slice(0, terminalReplyIndex)
      : terminalReplyIndex === -1
        ? nonNoticeBody
        : [];

  const summary = summarizeWorkItems(foldedSteps);
  const fullTurnToolSummary = summarizeWorkItems(nonNoticeBody);

  return {
    key,
    turnIndex,
    anchorSeq,
    userPrompt,
    isCompactionCommand,
    foldedSteps,
    workCounts: summary.counts,
    workSummaryLabel: summary.label,
    workSummaryIncludesFailure: summary.labelIncludesFailure,
    terminalReply,
    notices,
    liveSegments: buildLiveSegments(bodyItems),
    turnDiffs: Array.from(diffByPath.values()),
    hasError,
    hasUnrecoveredError: fullTurnToolSummary.counts.unrecoveredFailureCount > 0,
    hasActiveItem,
  };
}

/**
 * Structures the flat timeline into semantic Conversation Turns with:
 * - Settled step-batch & pre-reply auto-folding (`Edited 2 files, ran a command`)
 * - Live streaming vertical step timelines with rolling overflow collapse
 * - Turn-end aggregated diff summaries and sticky HITL approval/question gates
 */
export function processTimelineForStage(
  items: TimelineItemView[],
  _enableCoalescing = true,
): ProcessedTimelineStage {
  const turns: StageTurn[] = [];
  const pendingPermissions: Extract<TimelineItemView, { item_type: "permission_request" }>[] = [];
  const pendingQuestions: Extract<TimelineItemView, { item_type: "question_request" }>[] = [];
  const sessionTurnDiffs: DiffFileView[] = [];

  let currentPrompt: Extract<TimelineItemView, { item_type: "user_prompt" }> | null = null;
  let currentBody: TimelineItemView[] = [];
  let hasOpenTurn = false;

  const flushTurn = () => {
    if (!hasOpenTurn) return;
    turns.push(finalizeTurn(turns.length, currentPrompt, currentBody));
    currentPrompt = null;
    currentBody = [];
    hasOpenTurn = false;
  };

  for (const item of items) {
    if (item.item_type === "tool_call" && item.diff_files.length > 0) {
      for (const df of item.diff_files) {
        sessionTurnDiffs.push(df);
      }
    }

    // Active unresolved human-in-the-loop items anchor to the Sticky Dock above the Composer
    if (item.item_type === "permission_request" && !item.resolved_decision) {
      pendingPermissions.push(item);
      continue;
    }

    if (item.item_type === "question_request" && !item.resolved_answers) {
      pendingQuestions.push(item);
      continue;
    }

    if (item.item_type === "user_prompt") {
      flushTurn();
      currentPrompt = item;
      currentBody = [];
      hasOpenTurn = true;
      continue;
    }

    if (!hasOpenTurn) {
      hasOpenTurn = true;
      currentPrompt = null;
    }
    currentBody.push(item);
  }

  flushTurn();

  return {
    turns,
    pendingPermissions,
    pendingQuestions,
    sessionTurnDiffs,
  };
}
