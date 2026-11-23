import type { TimelineItemView, WorkSummaryResultView } from "@/lib/orbit-ipc";
import { classifyGenericSubagentTool } from "./subagents.ts";

export type ToolCallTimelineItem = Extract<TimelineItemView, { item_type: "tool_call" }>;

export type SplitStepChunk =
  | {
      kind: "work";
      /** Stable key derived from the first item in this chunk (preserved across streaming appends). */
      anchorKey: string;
      items: TimelineItemView[];
    }
  | {
      kind: "delegation";
      /** Stable key derived from the first item in this chunk (preserved across streaming appends). */
      anchorKey: string;
      items: ToolCallTimelineItem[];
    };

export interface StepGroupSplitResult {
  /** True when at least one subagent dispatch / inter-agent message was promoted out of the fold. */
  hasDelegations: boolean;
  chunks: SplitStepChunk[];
}

export interface FallbackDispatchCallInfo {
  callId: string;
  label: string;
  preview: string;
  status: "running" | "completed" | "failed" | "denied";
}

function stableTimelineItemKey(item: TimelineItemView): string {
  switch (item.item_type) {
    case "user_prompt":
      return item.message_id ? `prompt-${item.message_id}` : `prompt-seq-${item.seq}`;
    case "agent_message":
      return item.message_id ? `am-${item.message_id}` : `am-seq-${item.seq}`;
    case "assistant_thinking":
      return item.item_id ? `think-${item.item_id}` : `think-seq-${item.seq}`;
    case "assistant_message":
      return item.item_id ? `msg-${item.item_id}` : `msg-seq-${item.seq}`;
    case "tool_call":
      return item.call_id ? `tool-${item.call_id}` : `tool-seq-${item.seq}`;
    case "permission_request":
      return item.request_id ? `perm-${item.request_id}` : `perm-seq-${item.seq}`;
    case "question_request":
      return item.request_id ? `q-${item.request_id}` : `q-seq-${item.seq}`;
    case "notice":
      return `notice-${item.seq}`;
  }
}

/**
 * Returns true when a timeline item is a subagent dispatch (`invoke_subagent`, `create_agent`,
 * `spawn_workspace_peer`, or `detail.kind === "subagent"`) or an inter-agent message
 * (`send_message`, `send_agent_prompt`, `message_peer`) that should be promoted out of folded
 * work groups into a first-class chronological milestone row (Codex-style segmentation).
 */
export function isDelegationMilestoneToolCall(
  item: TimelineItemView,
): item is ToolCallTimelineItem {
  if (item.item_type !== "tool_call") return false;
  if (item.detail.kind === "subagent") {
    const isLegacyPlaceholder =
      !item.detail.child_agent_id &&
      !item.detail.prompt_preview.trim() &&
      item.detail.role.trim().toLowerCase() === "subagent";
    return !isLegacyPlaceholder;
  }
  if (item.detail.kind === "generic") {
    return classifyGenericSubagentTool(item.detail.tool_name) !== null;
  }
  return false;
}

/**
 * Partitions a `step_group`'s items into strictly chronological `work` and `delegation` chunks:
 * - Consecutive non-delegation items stay folded in a `work` chunk.
 * - Consecutive delegation/message items coalesce into a compact horizontal `delegation` row
 *   placed at their exact chronological position (never duplicated inside a fold).
 * - Fast-path: when no delegation calls exist, returns `hasDelegations: false` with a single
 *   untouched `work` chunk so callers can reuse Rust's precomputed summary verbatim.
 */
export function splitStepGroupChronologically(
  items: readonly TimelineItemView[],
): StepGroupSplitResult {
  let hasDelegations = false;
  for (let i = 0; i < items.length; i += 1) {
    if (isDelegationMilestoneToolCall(items[i])) {
      hasDelegations = true;
      break;
    }
  }

  if (!hasDelegations) {
    if (items.length === 0) {
      return { hasDelegations: false, chunks: [] };
    }
    return {
      hasDelegations: false,
      chunks: [
        {
          kind: "work",
          anchorKey: stableTimelineItemKey(items[0]),
          items: items as TimelineItemView[],
        },
      ],
    };
  }

  const chunks: SplitStepChunk[] = [];
  for (const item of items) {
    if (isDelegationMilestoneToolCall(item)) {
      const prev = chunks[chunks.length - 1];
      if (prev && prev.kind === "delegation") {
        prev.items.push(item);
      } else {
        chunks.push({
          kind: "delegation",
          anchorKey: stableTimelineItemKey(item),
          items: [item],
        });
      }
    } else {
      const prev = chunks[chunks.length - 1];
      if (prev && prev.kind === "work") {
        prev.items.push(item);
      } else {
        chunks.push({
          kind: "work",
          anchorKey: stableTimelineItemKey(item),
          items: [item],
        });
      }
    }
  }

  return { hasDelegations: true, chunks };
}

/**
 * Extracts readable fallback metadata for an unmatched generic dispatch tool call
 * (e.g. a failed/denied `invoke_subagent` or in-flight `create_agent` before its child card arrives)
 * so promoted delegation items never silently vanish from the timeline.
 */
export function extractFallbackDispatchCall(
  item: ToolCallTimelineItem,
): FallbackDispatchCallInfo | null {
  if (item.detail.kind !== "generic") return null;
  if (classifyGenericSubagentTool(item.detail.tool_name) !== "dispatch") return null;

  let label = "";
  let preview = "";
  const rawArgs = item.detail.arguments_json.trim();
  if (rawArgs.startsWith("{")) {
    try {
      const parsed: unknown = JSON.parse(rawArgs);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        const obj = parsed as Record<string, unknown>;
        for (const k of ["Role", "role", "title", "handle", "TypeName", "type_name"]) {
          const v = obj[k];
          if (typeof v === "string" && v.trim()) {
            label = v.trim();
            break;
          }
        }
        for (const k of ["Prompt", "prompt", "initial_prompt", "brief", "task"]) {
          const v = obj[k];
          if (typeof v === "string" && v.trim()) {
            preview = v.trim();
            break;
          }
        }
      }
    } catch {
      // Ignore partial JSON while streaming
    }
  }

  if (!label) {
    const parsedName = parseGenericToolName(item.detail.tool_name);
    label = parsedName.displayLabel || "Run subagent";
  }
  if (!preview && rawArgs && rawArgs !== "{}" && rawArgs !== "null") {
    preview = rawArgs;
  }

  return {
    callId: item.call_id || `dispatch-${item.seq}`,
    label,
    preview,
    status: item.status,
  };
}

/**
 * Computes elapsed duration for a split `work` chunk from its item timestamps when available
 * (>= 1000ms). Returns `null` when timestamps are missing or span less than 1 second.
 */
export function computeWorkChunkDurationMs(items: readonly TimelineItemView[]): number | null {
  let minTs: number | null = null;
  let maxTs: number | null = null;
  for (const it of items) {
    const ts = it.timestamp_ms;
    if (typeof ts === "number" && Number.isFinite(ts) && ts > 0) {
      if (minTs === null || ts < minTs) minTs = ts;
      if (maxTs === null || ts > maxTs) maxTs = ts;
    }
  }
  if (minTs === null || maxTs === null) return null;
  const delta = maxTs - minTs;
  return delta >= 1000 ? delta : null;
}

// ---------------------------------------------------------------------------
// Work-chunk summarizer (1:1 mirror of `crates/orbit-client-core/src/stage.rs`)
// Used ONLY for `work` chunks inside split step groups (untouched step groups
// reuse Rust's precomputed `seg.summary` verbatim).
// ---------------------------------------------------------------------------

function basenameOfPath(filePath: string): string {
  if (!filePath) return "";
  const trimmed = filePath.replace(/[/\\]+$/, "");
  const parts = trimmed.split(/[/\\]/);
  return parts[parts.length - 1] ?? trimmed;
}

function formatDurationCompact(durationMs: number): string {
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

function formatThoughtSummaryLabel(durationMs?: number | null, thoughtCount = 1): string {
  if (typeof durationMs === "number" && Number.isFinite(durationMs)) {
    const roundedSec = Math.round(durationMs / 1000);
    if (roundedSec >= 1) {
      return `Thought for ${formatDurationCompact(durationMs)}`;
    }
  }
  return thoughtCount > 1 ? "Thought process" : "Thought about it";
}

function extractThinkingHeadline(rawText: string, preferLatest = false): string {
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

function parseGenericToolName(rawName: string): {
  serverName: string | null;
  actionLabel: string;
  displayLabel: string;
} {
  const trimmed = rawName.trim();
  if (!trimmed) {
    return { serverName: null, actionLabel: "tool", displayLabel: "Tool" };
  }
  if (trimmed.startsWith("mcp__")) {
    const parts = trimmed.slice(5).split("__").filter(Boolean);
    if (parts.length >= 2) {
      const serverName = formatServerTitle(parts[0]);
      const actionLabel = humanizeIdentifier(parts.slice(1).join(" ")).toLowerCase();
      return {
        serverName,
        actionLabel,
        displayLabel: `${serverName}: ${actionLabel}`,
      };
    }
  }
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

function classifySearchTool(query: string, summaryTitle: string): "web" | "glob" | "grep" {
  const titleLower = summaryTitle.trim().toLowerCase();
  const qTrim = query.trim();
  if (/\b(?:web|internet|google|bing|url)\b/.test(titleLower) || /^https?:\/\//i.test(qTrim)) {
    return "web";
  }
  if (
    /\b(?:glob|find files|file pattern|list files)\b/.test(titleLower) ||
    (/^\*\*?\//.test(qTrim) && !/\s/.test(qTrim))
  ) {
    return "glob";
  }
  return "grep";
}

function isFileCreateToolCall(item: ToolCallTimelineItem): boolean {
  if (item.detail.kind !== "file_edit") return false;
  if (/^(?:create|created|write|wrote)\b/i.test(item.summary_title.trim())) {
    return true;
  }
  if (item.detail.unified_diff && /---\s+\/dev\/null/.test(item.detail.unified_diff)) {
    return true;
  }
  return false;
}

function lowercaseClauseLead(clause: string): string {
  if (!clause) return "";
  return clause.charAt(0).toLowerCase() + clause.slice(1);
}

function pushUnique(list: string[], value: string): void {
  if (!list.includes(value)) {
    list.push(value);
  }
}

function summarizeSingleToolCall(item: ToolCallTimelineItem): [string, boolean] {
  if (item.status === "denied") {
    return [
      item.detail.kind === "shell" ? "Stopped running the command" : "You declined",
      true,
    ];
  }

  const failed = item.status === "failed";
  const withFailed = (base: string): [string, boolean] =>
    failed ? [`${base} (failed)`, true] : [base, false];

  switch (item.detail.kind) {
    case "file_read": {
      const name = basenameOfPath(item.detail.path);
      return withFailed(name ? `Read ${name}` : "Read a file");
    }
    case "file_edit": {
      const isCreate = isFileCreateToolCall(item);
      const name = basenameOfPath(item.detail.path);
      const prefix = isCreate ? "Created" : "Edited";
      return withFailed(name ? `${prefix} ${name}` : `${prefix} a file`);
    }
    case "search": {
      const sub = classifySearchTool(item.detail.query, item.summary_title);
      if (sub === "web") {
        return failed ? ["Failed to search the web", true] : ["Searched the web", false];
      }
      if (sub === "glob") {
        return withFailed("Looked for files");
      }
      return withFailed("Searched files");
    }
    case "shell":
      return failed ? ["Command failed", true] : ["Ran a command", false];
    case "subagent":
      return withFailed("Ran an agent");
    case "generic": {
      const parsed = parseGenericToolName(item.detail.tool_name);
      if (parsed.serverName) {
        return withFailed(`Used ${parsed.serverName} integration`);
      }
      if (parsed.displayLabel && parsed.displayLabel !== "Tool") {
        return withFailed(`Used ${parsed.displayLabel}`);
      }
      return failed ? ["Failed", true] : ["Used a tool", false];
    }
  }
}

function buildLiveStepStatus(item: TimelineItemView | undefined): [string, string | null] {
  if (!item) return ["Working…", null];
  if (item.item_type === "assistant_thinking") {
    const headline = extractThinkingHeadline(item.text, true);
    return headline ? ["Thinking", headline] : ["Thinking…", null];
  }
  if (item.item_type === "tool_call") {
    switch (item.detail.kind) {
      case "file_read": {
        const name = basenameOfPath(item.detail.path);
        return [name ? `Reading ${name}…` : "Reading a file…", null];
      }
      case "file_edit": {
        const isCreate = isFileCreateToolCall(item);
        const name = basenameOfPath(item.detail.path);
        const verb = isCreate ? "Creating" : "Editing";
        return [name ? `${verb} ${name}…` : `${verb} a file…`, null];
      }
      case "search": {
        const sub = classifySearchTool(item.detail.query, item.summary_title);
        if (sub === "web") return ["Searching the web…", null];
        if (sub === "glob") return ["Looking for files…", null];
        const q = item.detail.query.trim().replace(/\s+/g, " ");
        return [q.length >= 1 && q.length <= 28 ? `Searching "${q}"…` : "Searching files…", null];
      }
      case "shell": {
        const firstLine =
          item.detail.command
            .split(/\r?\n/)
            .map((ln) => ln.trim().replace(/\s+/g, " "))
            .find(Boolean) ?? "";
        if (!firstLine) return ["Running a command…", null];
        const shortCmd = Array.from(firstLine).slice(0, 36).join("").trimEnd();
        return [shortCmd.endsWith("…") ? `Running ${shortCmd}` : `Running ${shortCmd}…`, null];
      }
      case "subagent": {
        const cleanRole = item.detail.role.trim().replace(/\s+/g, " ");
        return [
          cleanRole.length >= 1 && cleanRole.length <= 28
            ? `Running ${cleanRole}…`
            : "Running subagent…",
          null,
        ];
      }
      case "generic": {
        const parsed = parseGenericToolName(item.detail.tool_name);
        if (parsed.serverName) return [`Using ${parsed.serverName}…`, null];
        if (parsed.displayLabel) return [`Using ${parsed.displayLabel}…`, null];
        return ["Using a tool…", null];
      }
    }
  }
  return ["Working…", null];
}

/**
 * Recomputes a `WorkSummaryResultView` for a split `work` chunk whose promoted delegation/message
 * tool calls have been extracted into sibling chronological milestone rows.
 */
export function summarizeWorkChunk(
  items: readonly TimelineItemView[],
  thoughtDurationMs?: number | null,
): WorkSummaryResultView {
  const toolCalls: ToolCallTimelineItem[] = [];
  let thoughtCount = 0;
  let narrationCount = 0;
  let permissionCount = 0;
  let questionCount = 0;
  let additions = 0;
  let deletions = 0;

  for (const it of items) {
    switch (it.item_type) {
      case "assistant_thinking":
        thoughtCount += 1;
        break;
      case "assistant_message":
        if (it.text.trim().length > 0) narrationCount += 1;
        break;
      case "permission_request":
        permissionCount += 1;
        break;
      case "question_request":
        questionCount += 1;
        break;
      case "tool_call":
        toolCalls.push(it);
        for (const df of it.diff_files) {
          additions += df.additions;
          deletions += df.deletions;
        }
        break;
      default:
        break;
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
      trailingFailureStreak = 0;
    }
  }

  const totalTc = toolCalls.length;
  const allFailed = totalTc > 0 && failedCount + deniedCount === totalTc;
  const unrecoveredFailureCount = allFailed ? failedCount + deniedCount : trailingFailureStreak;

  const editedPaths: string[] = [];
  let editPathless = 0;
  let editCalls = 0;

  const createdPaths: string[] = [];
  let createPathless = 0;
  let createCalls = 0;

  const readPaths: string[] = [];
  let readPathless = 0;
  let readCalls = 0;

  let commandCount = 0;
  let grepCount = 0;
  let globCount = 0;
  let webSearchCount = 0;
  let subagentCount = 0;
  const mcpServers: string[] = [];
  let mcpCallCount = 0;
  let otherToolCount = 0;

  const firstSeen = new Map<string, number>();
  const markSeen = (key: string) => {
    if (!firstSeen.has(key)) {
      firstSeen.set(key, firstSeen.size);
    }
  };

  for (const tc of toolCalls) {
    switch (tc.detail.kind) {
      case "file_edit":
        if (isFileCreateToolCall(tc)) {
          markSeen("create");
          createCalls += 1;
          if (tc.detail.path) pushUnique(createdPaths, tc.detail.path);
          else createPathless += 1;
        } else {
          markSeen("edit");
          editCalls += 1;
          if (tc.detail.path) pushUnique(editedPaths, tc.detail.path);
          else editPathless += 1;
        }
        break;
      case "file_read":
        markSeen("read");
        readCalls += 1;
        if (tc.detail.path) pushUnique(readPaths, tc.detail.path);
        else readPathless += 1;
        break;
      case "search": {
        const sub = classifySearchTool(tc.detail.query, tc.summary_title);
        if (sub === "web") {
          markSeen("search_web");
          webSearchCount += 1;
        } else if (sub === "glob") {
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
          pushUnique(mcpServers, parsed.serverName);
        } else {
          markSeen("other");
          otherToolCount += 1;
        }
        break;
      }
    }
  }

  const editedFileCount = editedPaths.length + editPathless;
  const createdFileCount = createdPaths.length + createPathless;
  const readFileCount = readPaths.length + readPathless;
  const searchCount = grepCount + globCount + webSearchCount;
  const mcpServerCount = mcpServers.length;
  const actionSteps = totalTc;
  const totalSteps = actionSteps + thoughtCount + permissionCount + questionCount;

  let activeTail: TimelineItemView | undefined;
  for (let i = items.length - 1; i >= 0; i -= 1) {
    const it = items[i];
    if (it.item_type === "tool_call" && it.status === "running") {
      activeTail = it;
      break;
    }
  }
  if (!activeTail) {
    activeTail = items[items.length - 1];
  }
  const [liveLabel, liveDetail] = buildLiveStepStatus(activeTail);

  let labelIncludesFailure = false;
  let label = "";

  if (toolCalls.length === 0) {
    if (thoughtCount > 0) {
      label = formatThoughtSummaryLabel(thoughtDurationMs, thoughtCount);
    } else if (questionCount > 0) {
      label = questionCount === 1 ? "Asked a question" : `Asked ${questionCount} questions`;
    } else if (permissionCount > 0) {
      label = "Checked permissions";
    } else if (narrationCount > 0) {
      label = narrationCount === 1 ? "1 note" : `${narrationCount} notes`;
    } else {
      label = "Completed steps";
    }
  } else if (toolCalls.length === 1) {
    const [singleLabel, incFail] = summarizeSingleToolCall(toolCalls[0]);
    label = singleLabel;
    labelIncludesFailure = incFail;
  } else if (deniedCount === totalTc) {
    label = "You declined";
    labelIncludesFailure = true;
  } else {
    interface Bucket {
      label: string;
      tier: number;
      rawCallCount: number;
      firstOrder: number;
    }
    const isSoleCategory = firstSeen.size === 1;
    const buckets: Bucket[] = [];

    if (editedFileCount > 0) {
      const soleName =
        isSoleCategory && editedPaths.length === 1 ? basenameOfPath(editedPaths[0]) : "";
      buckets.push({
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
        isSoleCategory && createdPaths.length === 1 ? basenameOfPath(createdPaths[0]) : "";
      buckets.push({
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
        label: subagentCount === 1 ? "Ran an agent" : `Ran ${subagentCount} agents`,
        tier: 2,
        rawCallCount: subagentCount,
        firstOrder: firstSeen.get("subagent") ?? 0,
      });
    }

    if (mcpServerCount > 0) {
      const firstServer = mcpServers[0] ?? "connector";
      buckets.push({
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
        isSoleCategory && readPaths.length === 1 ? basenameOfPath(readPaths[0]) : "";
      buckets.push({
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
        label: "Searched files",
        tier: 3,
        rawCallCount: grepCount,
        firstOrder: firstSeen.get("search_files") ?? 0,
      });
    }

    if (globCount > 0) {
      buckets.push({
        label: "Looked for files",
        tier: 3,
        rawCallCount: globCount,
        firstOrder: firstSeen.get("look_files") ?? 0,
      });
    }

    if (webSearchCount > 0) {
      buckets.push({
        label: "Searched the web",
        tier: 3,
        rawCallCount: webSearchCount,
        firstOrder: firstSeen.get("search_web") ?? 0,
      });
    }

    if (commandCount > 0) {
      buckets.push({
        label: commandCount === 1 ? "Ran a command" : `Ran ${commandCount} commands`,
        tier: 4,
        rawCallCount: commandCount,
        firstOrder: firstSeen.get("shell") ?? 0,
      });
    }

    if (otherToolCount > 0) {
      buckets.push({
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
    const shownLen = Math.min(buckets.length, visibleLimit);
    let hiddenSteps = 0;
    for (let i = shownLen; i < buckets.length; i += 1) {
      hiddenSteps += buckets[i].rawCallCount;
    }
    const formattedShown = buckets
      .slice(0, shownLen)
      .map((b, idx) => (idx === 0 ? b.label : lowercaseClauseLead(b.label)))
      .join(", ");

    if (shownLen > 0 && hiddenSteps > 0) {
      const noun = hiddenSteps === 1 ? "step" : "steps";
      label = `${formattedShown}, and ${hiddenSteps} more ${noun}`;
    } else if (formattedShown) {
      label = formattedShown;
    } else {
      label = `Used ${totalTc} tools`;
    }
  }

  return {
    counts: {
      edited_file_count: editedFileCount,
      created_file_count: createdFileCount,
      command_count: commandCount,
      read_file_count: readFileCount,
      search_count: searchCount,
      subagent_count: subagentCount,
      mcp_server_count: mcpServerCount,
      other_tool_count: otherToolCount,
      thought_count: thoughtCount,
      narration_count: narrationCount,
      action_steps: actionSteps,
      total_steps: totalSteps,
      failed_count: failedCount,
      denied_count: deniedCount,
      unrecovered_failure_count: unrecoveredFailureCount,
      additions,
      deletions,
    },
    label,
    live_label: liveLabel,
    live_detail: liveDetail,
    label_includes_failure: labelIncludesFailure,
  };
}
