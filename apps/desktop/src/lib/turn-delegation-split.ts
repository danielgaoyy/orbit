import { DEFAULT_LOCALE, translate, type OrbitLocale } from "./i18n/index.ts";
import { formatDurationCompact, formatThoughtSummaryLabel } from "./timeline-coalesce.ts";
import {
  extractCleanAgentMessageBody,
  extractDirectAgentMessagePresentation,
} from "./agent-message-clean.ts";
import type { StageTurnView, TimelineItemView, TurnLiveSegmentView, WorkSummaryResultView } from "@/lib/orbit-ipc";
import { classifyGenericSubagentTool, resolveToolCallSubagent } from "./subagents.ts";

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

export interface StateChangeMilestoneInfo {
  callId: string;
  kind: "timer" | "handoff";
  label: string;
  preview: string;
  status: "running" | "completed" | "failed" | "denied";
}

type AgentMessage = Extract<TimelineItemView, { item_type: "agent_message" }>;
export type CoalescedFinishStatus = "completed" | "canceled";

export function peerFinishStatus(item: AgentMessage, cleanBody?: string): CoalescedFinishStatus | "failed" | null {
  if (item.kind !== "finish_notification" || item.permission_context) return null;
  const event = item.event?.trim().toLowerCase() ?? "";
  if (
    ["failed", "error", "errored"].includes(event) ||
    /^Agent\s+\S+(?:\s+\(.*\))?\s+(?:failed|errored)\./im.test(item.text) ||
    /^\s*(?:error|failed):/im.test(cleanBody ?? extractCleanAgentMessageBody(item.text))
  ) return "failed";
  if (["canceled", "cancelled", "stopped", "closed"].includes(event)) return "canceled";
  if (["", "finished", "completed", "succeeded"].includes(event)) return "completed";
  return null;
}

/**
 * Returns true when an incoming `agent_message` is a real dispatched prompt/instruction
 * (`create_agent`, `spawn_workspace_peer`, `send_agent_prompt`, `message_peer`) that
 * should render inside the recipient timeline using the shared `UserPromptBubble`.
 *
 * Strictly excludes notifications, heartbeats, permission checkpoints, and pure
 * protocol/system noise (`<orbit-fleet-state>`, `<orbit-fleet-delta>`, control signals).
 */
export function isDirectAgentPromptBubble(item: AgentMessage): boolean {
  if (item.kind !== "direct_message") return false;
  if (item.permission_context) return false;
  const parsed = extractDirectAgentMessagePresentation(item.text, item);
  return !parsed.isSystemNoise && parsed.cleanText.length > 0;
}

/**
 * Returns true when an incoming `agent_message` is routine system noise that should stay
 * silent across both main stage and subagent/peer conversation streams.
 *
 * Preserves:
 * - visible `direct_message` dispatch prompts (rendered via shared `UserPromptBubble`)
 * - `heartbeat` wakeup dividers (rendered via `AgentMessageCard`)
 * - `permission_context` approval checkpoints (rendered via `AgentMessageCard` while pending)
 * - failed `finish_notification` alerts (rendered via `AgentMessageCard`)
 */
export function isTimelineAgentMessageHidden(item: AgentMessage): boolean {
  if (item.kind === "heartbeat") return false;
  if (item.permission_context) return false;
  if (item.kind === "finish_notification") {
    return peerFinishStatus(item) !== "failed";
  }
  if (item.kind === "direct_message") {
    return !isDirectAgentPromptBubble(item);
  }
  return true;
}

/**
 * Suppress hidden incoming `agent_message` segments (routine/queued finish callbacks and
 * inter-agent direct messages) and rejoin adjacent `step_group` segments split across
 * those now-invisible boundaries.
 */
export function coalescePeerFinishes(
  _openingMessage: StageTurnView["agent_message"],
  segments: readonly TurnLiveSegmentView[],
  _hostId: string,
  options?: { preserveFinishReports?: boolean },
) {
  const preserveFinishReports = Boolean(options?.preserveFinishReports);
  const finishStatuses = new Map<AgentMessage, CoalescedFinishStatus>();
  const visible: TurnLiveSegmentView[] = [];
  for (const seg of segments) {
    if (
      seg.kind === "agent_message" &&
      seg.item.item_type === "agent_message" &&
      !(
        preserveFinishReports &&
        seg.item.kind === "finish_notification" &&
        !seg.item.permission_context
      ) &&
      isTimelineAgentMessageHidden(seg.item)
    ) {
      continue;
    }
    // The core flushed its work group at a hidden segment (agent message, hidden wakeup,
    // or plan notice). Rejoin any now-adjacent step groups so collapsed summaries stay contiguous.
    const prev = visible.at(-1);
    if (prev?.kind === "step_group" && seg.kind === "step_group") {
      const items = [...prev.items, ...seg.items];
      const thoughtMs = (prev.thought_duration_ms ?? 0) + (seg.thought_duration_ms ?? 0) || null;
      visible[visible.length - 1] = {
        ...prev,
        items,
        thought_duration_ms: thoughtMs,
        summary: summarizeWorkChunk(items, thoughtMs),
        work_duration_ms: (prev.work_duration_ms ?? 0) + (seg.work_duration_ms ?? 0) || null,
        latest_activity_ms: seg.latest_activity_ms ?? prev.latest_activity_ms,
        has_running_step: prev.has_running_step || seg.has_running_step,
        has_following_content: seg.has_following_content,
      };
    } else {
      visible.push(seg);
    }
  }
  return { segments: visible, finishStatuses };
}

export interface CompletedTurnWorkFoldPlan {
  shouldWrap: boolean;
  items: TimelineItemView[];
  fallbackLabel: string;
  unrecoveredFailureCount: number;
  hasAnyFailureOrDenial: boolean;
  sumWorkDurationMs: number;
}

const NO_TURN_WORK_FOLD: CompletedTurnWorkFoldPlan = {
  shouldWrap: false,
  items: [],
  fallbackLabel: "",
  unrecoveredFailureCount: 0,
  hasAnyFailureOrDenial: false,
  sumWorkDurationMs: 0,
};

/**
 * Decides whether a settled turn's pre-final segments should collapse behind the
 * top-level `TurnWorkedDisclosure` (`Worked for Xs`) above the final reply.
 *
 * Strictly wraps ONLY pure pre-final tool-bearing `step_group` segments (with no
 * promoted timer/handoff milestones). Any earlier `assistant_message`, `notice`,
 * `compaction`, `wakeup`, visible `agent_message`, or promoted milestone prevents
 * wrapping so important intermediate replies, runtime warnings, and post-turn wakeup
 * reports remain visible at top level while their surrounding tool groups collapse
 * quietly on their own.
 *
 * Pure thinking-only pre-final groups (`action_steps === 0`) also skip wrapping so
 * they keep their dedicated `[Brain] Thought about it` / `已思考` disclosure.
 */
export function resolveCompletedTurnWorkFold(params: {
  segments: readonly TurnLiveSegmentView[];
  isTurnActive: boolean;
  turnHasUnrecoveredError: boolean;
  locale?: OrbitLocale;
  stepGroupSplits?: ReadonlyMap<string, StepGroupSplitResult>;
}): CompletedTurnWorkFoldPlan {
  const {
    segments,
    isTurnActive,
    turnHasUnrecoveredError,
    locale = DEFAULT_LOCALE,
    stepGroupSplits,
  } = params;
  if (isTurnActive || segments.length < 2) return NO_TURN_WORK_FOLD;
  const lastSeg = segments[segments.length - 1];
  if (
    !lastSeg ||
    lastSeg.kind !== "assistant_message" ||
    lastSeg.item.item_type !== "assistant_message" ||
    lastSeg.item.streaming ||
    !lastSeg.item.text.trim()
  ) {
    return NO_TURN_WORK_FOLD;
  }

  const preFinal = segments.slice(0, -1);
  const items: TimelineItemView[] = [];
  let totalActionSteps = 0;
  let totalThoughtMs = 0;
  let sumWorkDurationMs = 0;
  let unrecoveredFailureCount = 0;
  let hasAnyFailureOrDenial = false;

  for (const seg of preFinal) {
    if (seg.kind !== "step_group") {
      return NO_TURN_WORK_FOLD;
    }
    const split = stepGroupSplits?.get(seg.key) ?? splitStepGroupChronologically(seg.items);
    if (split.hasDelegations) {
      return NO_TURN_WORK_FOLD;
    }
    items.push(...seg.items);
    totalActionSteps += seg.summary.counts.action_steps ?? 0;
    if (seg.thought_duration_ms) totalThoughtMs += seg.thought_duration_ms;
    if (seg.work_duration_ms) sumWorkDurationMs += seg.work_duration_ms;
    const unrecovered = seg.summary.counts.unrecovered_failure_count ?? 0;
    if (turnHasUnrecoveredError) {
      unrecoveredFailureCount += unrecovered;
    }
    if (
      (seg.summary.counts.failed_count ?? 0) > 0 ||
      (seg.summary.counts.denied_count ?? 0) > 0 ||
      unrecovered > 0
    ) {
      hasAnyFailureOrDenial = true;
    }
  }

  if (items.length === 0 || totalActionSteps <= 0) {
    return NO_TURN_WORK_FOLD;
  }

  const singleGroup =
    preFinal.length === 1 && preFinal[0]?.kind === "step_group" ? preFinal[0] : null;
  const fallbackLabel =
    singleGroup && locale === "en" && singleGroup.summary.label
      ? singleGroup.summary.label
      : summarizeWorkChunk(items, totalThoughtMs > 0 ? totalThoughtMs : null, locale).label;

  return {
    shouldWrap: true,
    items,
    fallbackLabel,
    unrecoveredFailureCount,
    hasAnyFailureOrDenial,
    sumWorkDurationMs,
  };
}

function milestoneArgs(raw: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(raw);
    return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
  } catch {
    return null; // Tool arguments may still be streaming.
  }
}

export function classifyStateChangeMilestoneTool(toolName: string, rawArgs = ""): "timer" | "handoff" | null {
  const leaf = toolName.trim().toLowerCase().replace(/^run\s+/, "")
    .split(/[:/.]|__/).at(-1)?.trim().replace(/\s+/g, "_");
  if (leaf === "handoff_worktree" || leaf === "mcp_orbit_handoff_worktree") return "handoff";
  if (leaf !== "schedule" && leaf !== "orbit_timer" && leaf !== "mcp_orbit_orbit_timer") return null;
  const args = milestoneArgs(rawArgs);
  if (!args) return null;
  const action = args.action ?? args.Action;
  if (action != null) {
    return typeof action === "string" && action.trim().toLowerCase() === "create" ? "timer" : null;
  }
  // Older provider schedule calls omit the action and carry the schedule itself.
  const seconds = args.duration_seconds ?? args.DurationSeconds;
  const cron = args.cron_expression ?? args.CronExpression;
  const condition = args.timer_condition ?? args.TimerCondition ?? args.condition;
  const timed = typeof seconds === "number" && seconds > 0;
  const scheduled = typeof cron === "string" && cron.trim().length > 0;
  const conditional = typeof condition === "string" && /\S/.test(condition) && condition.trim().toLowerCase() !== "never";
  return timed || scheduled || conditional ? "timer" : null;
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
 * Returns true when a timeline item is an outgoing subagent dispatch (`invoke_subagent`,
 * `create_agent`, `spawn_workspace_peer`, or `detail.kind === "subagent"`) or inter-agent
 * message (`send_message`, `send_agent_prompt`, `message_peer`), used by `useDelegationDispatches`
 * to resolve target subagents across turns while keeping them folded inside step groups.
 */
export function isSubagentOrMessageToolCall(
  item: TimelineItemView,
): item is ToolCallTimelineItem {
  if (item.item_type !== "tool_call") return false;
  if (item.detail.kind === "subagent") {
    const isLegacyPlaceholder =
      !item.detail.child_agent_id &&
      !item.detail.prompt_preview.trim() &&
      item.detail.role.trim().toLowerCase() === "subagent";
    return !isLegacyPlaceholder || item.status === "running";
  }
  if (item.detail.kind === "generic") {
    return classifyGenericSubagentTool(item.detail.tool_name) !== null;
  }
  return false;
}

/**
 * Returns true when a timeline item is a session-level state-change milestone (`orbit_timer`
 * create, `schedule`, or `handoff_worktree`) promoted out of folded work groups.
 *
 * Outgoing subagent dispatches (`invoke_subagent`, `create_agent`, `spawn_workspace_peer`,
 * `detail.kind === "subagent"`) and outgoing inter-agent messages (`send_message`,
 * `send_agent_prompt`, `message_peer`) stay folded inside regular step groups (aligned with
 * Codex / Antigravity) so work blocks are never fragmented by standalone rows.
 */
export function isDelegationMilestoneToolCall(
  item: TimelineItemView,
): item is ToolCallTimelineItem {
  if (item.item_type !== "tool_call") return false;
  if (item.detail.kind === "generic") {
    return (
      classifyStateChangeMilestoneTool(item.detail.tool_name, item.detail.arguments_json) !== null
    );
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
  const resolved = resolveToolCallSubagent(item, []);
  if (resolved?.action !== "dispatch") return null;
  return {
    callId: resolved.subagent.callId,
    label: resolved.subagent.title,
    preview: resolved.subagent.promptPreview,
    status: item.status,
  };
}

/**
 * Extracts concise milestone metadata for session-level state-changing tools
 * (`schedule`, `orbit_timer`, `handoff_worktree`) surfaced outside folded step groups.
 */
export function extractStateChangeMilestoneCall(
  item: ToolCallTimelineItem,
  locale: OrbitLocale = DEFAULT_LOCALE,
): StateChangeMilestoneInfo | null {
  if (item.detail.kind !== "generic") return null;
  const kind = classifyStateChangeMilestoneTool(item.detail.tool_name, item.detail.arguments_json);
  if (!kind) return null;

  const t = translate.bind(null, locale);
  let label = t(kind === "timer" ? "stage.timerArmed" : "stage.handoffWorktree");
  let preview = "";
  const args = milestoneArgs(item.detail.arguments_json);
  if (args && kind === "timer") {
    const seconds = args.duration_seconds ?? args.DurationSeconds;
    const cron = args.cron_expression ?? args.CronExpression;
    if (typeof cron === "string" && cron.trim()) {
      label = t("stage.timerScheduled", { cron: cron.trim() });
    } else if (typeof seconds === "number" && seconds > 0) {
      label = t("stage.timerDuration", { duration: formatDurationCompact(seconds * 1000, locale) });
    }
    const prompt = args.prompt ?? args.Prompt;
    preview = typeof prompt === "string" ? prompt.trim() : "";
  } else if (args) {
    const direction = t(args.direction === "local_to_worktree" ? "stage.localToWorktree" : "stage.worktreeToLocal");
    label = t(args.dry_run === true ? "stage.handoffPreview" : "stage.handoff", { direction });
    preview = typeof args.branch === "string" ? args.branch.trim() : "";
  }
  if (!preview && args && Object.keys(args).length) preview = JSON.stringify(args, null, 2);

  return {
    callId: item.call_id || `milestone-${item.seq}`,
    kind,
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
// Formats split chunks and localized display text for groups projected by core.
// ---------------------------------------------------------------------------

function basenameOfPath(filePath: string): string {
  if (!filePath) return "";
  const trimmed = filePath.replace(/[/\\]+$/, "");
  const parts = trimmed.split(/[/\\]/);
  return parts[parts.length - 1] ?? trimmed;
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

function summarizeSingleToolCall(item: ToolCallTimelineItem, locale: OrbitLocale): [string, boolean] {
  const t = translate.bind(null, locale);
  if (item.status === "denied") {
    return [
      item.detail.kind === "shell" ? t("stage.commandStopped") : t("stage.declined"),
      true,
    ];
  }

  const failed = item.status === "failed";
  const withFailed = (base: string): [string, boolean] =>
    failed ? [t("stage.failedSummary", { action: base }), true] : [base, false];

  switch (item.detail.kind) {
    case "file_read": {
      const name = basenameOfPath(item.detail.path);
      return withFailed(name ? t("stage.readPath", { path: name }) : t("stage.readFile"));
    }
    case "file_edit": {
      const isCreate = isFileCreateToolCall(item);
      const name = basenameOfPath(item.detail.path);
      return withFailed(name
        ? t(isCreate ? "stage.createdPath" : "stage.editedPath", { path: name })
        : t(isCreate ? "stage.createdFile" : "stage.editedFile"));
    }
    case "search": {
      const sub = classifySearchTool(item.detail.query, item.summary_title);
      if (sub === "web") {
        return failed ? [t("stage.webSearchFailed"), true] : [t("stage.searchedWeb"), false];
      }
      if (sub === "glob") {
        return withFailed(t("stage.foundFiles"));
      }
      return withFailed(t("stage.searchedFiles"));
    }
    case "shell":
      return failed ? [t("stage.commandFailed"), true] : [t("stage.ranCommand"), false];
    case "subagent":
      return withFailed(t("stage.ranAgent"));
    case "generic": {
      const parsed = parseGenericToolName(item.detail.tool_name);
      if (parsed.serverName) {
        return withFailed(t("stage.usedIntegration", { name: parsed.serverName }));
      }
      if (parsed.displayLabel && parsed.displayLabel !== "Tool") {
        return withFailed(t("stage.usedNamedTool", { name: parsed.displayLabel }));
      }
      return failed ? [t("sidePanel.scriptFailed"), true] : [t("stage.usedTool"), false];
    }
  }
}

function buildLiveStepStatus(item: TimelineItemView | undefined, locale: OrbitLocale): [string, string | null] {
  const t = translate.bind(null, locale);
  if (!item) return [t("stage.working"), null];
  if (item.item_type === "assistant_thinking") {
    return [t("stage.thinkingPending"), null];
  }
  if (item.item_type === "tool_call") {
    switch (item.detail.kind) {
      case "file_read": {
        const name = basenameOfPath(item.detail.path);
        return [name ? t("stage.readingPath", { path: name }) : t("stage.readingFile"), null];
      }
      case "file_edit": {
        const isCreate = isFileCreateToolCall(item);
        const name = basenameOfPath(item.detail.path);
        const label = name
          ? t(isCreate ? "stage.creatingPath" : "stage.editingPath", { path: name })
          : t(isCreate ? "stage.creatingFile" : "stage.editingFile");
        return [label, null];
      }
      case "search": {
        const sub = classifySearchTool(item.detail.query, item.summary_title);
        if (sub === "web") return [`${t("stage.searchingWeb")}…`, null];
        if (sub === "glob") return [`${t("stage.findingFiles")}…`, null];
        const q = item.detail.query.trim().replace(/\s+/g, " ");
        return [q.length >= 1 && q.length <= 28 ? t("stage.searchingFor", { query: q }) : t("stage.searchingFiles"), null];
      }
      case "shell": {
        const firstLine =
          item.detail.command
            .split(/\r?\n/)
            .map((ln) => ln.trim().replace(/\s+/g, " "))
            .find(Boolean) ?? "";
        if (!firstLine) return [t("stage.runningCommand"), null];
        const shortCmd = Array.from(firstLine).slice(0, 36).join("").trimEnd();
        return [t("stage.runningTarget", { target: shortCmd.replace(/…$/, "") }), null];
      }
      case "subagent": {
        const cleanRole = item.detail.role.trim().replace(/\s+/g, " ");
        return [
          cleanRole.length >= 1 && cleanRole.length <= 28
            ? t("stage.runningTarget", { target: cleanRole })
            : `${t("stage.subagentRunning")}…`,
          null,
        ];
      }
      case "generic": {
        const parsed = parseGenericToolName(item.detail.tool_name);
        const name = parsed.serverName || (item.detail.tool_name.trim() && parsed.displayLabel);
        return [name ? t("stage.usingNamedTool", { name }) : t("stage.usingTool"), null];
      }
    }
  }
  return [t("stage.working"), null];
}

/**
 * Recomputes a `WorkSummaryResultView` for a split `work` chunk whose promoted delegation/message
 * tool calls have been extracted into sibling chronological milestone rows.
 */
export function summarizeWorkChunk(
  items: readonly TimelineItemView[],
  thoughtDurationMs?: number | null,
  locale: OrbitLocale = DEFAULT_LOCALE,
): WorkSummaryResultView {
  const t = translate.bind(null, locale);
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
        markSeen("explore");
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
          markSeen("explore");
          globCount += 1;
        } else {
          markSeen("explore");
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
  const [liveLabel, liveDetail] = buildLiveStepStatus(activeTail, locale);

  let labelIncludesFailure = false;
  let label = "";

  if (toolCalls.length === 0) {
    if (thoughtCount > 0) {
      label = formatThoughtSummaryLabel(thoughtDurationMs, thoughtCount, locale);
    } else if (questionCount > 0) {
      label = questionCount === 1 ? t("stage.askedQuestion") : t("stage.askedQuestions", { count: questionCount });
    } else if (permissionCount > 0) {
      label = t("stage.checkedPermissions");
    } else if (narrationCount > 0) {
      label = t(narrationCount === 1 ? "stage.noteCount" : "stage.notesCount", { count: narrationCount });
    } else {
      label = t("stage.completedSteps");
    }
  } else if (toolCalls.length === 1) {
    const [singleLabel, incFail] = summarizeSingleToolCall(toolCalls[0], locale);
    label = singleLabel;
    labelIncludesFailure = incFail;
  } else if (deniedCount === totalTc) {
    label = t("stage.declined");
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
          ? t("stage.editedPath", { path: soleName })
          : editedFileCount === 1
            ? t("stage.editedFile")
            : t("stage.editedFiles"),
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
          ? t("stage.createdPath", { path: soleName })
          : createdFileCount === 1
            ? t("stage.createdFile")
            : t("stage.createdFiles"),
        tier: 1,
        rawCallCount: createCalls,
        firstOrder: firstSeen.get("create") ?? 0,
      });
    }

    if (subagentCount > 0) {
      buckets.push({
        label: subagentCount === 1 ? t("stage.ranAgent") : t("stage.ranAgents"),
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
            ? t("stage.usedIntegration", { name: firstServer })
            : t("stage.usedIntegrations"),
        tier: 2,
        rawCallCount: mcpCallCount,
        firstOrder: firstSeen.get("mcp") ?? 0,
      });
    }

    if (readFileCount > 0 || grepCount > 0 || globCount > 0) {
      const exploreCalls = readCalls + grepCount + globCount;
      let exploreLabel: string;
      if (readFileCount > 0) {
        const soleName =
          isSoleCategory && readPaths.length === 1 && grepCount === 0 && globCount === 0
            ? basenameOfPath(readPaths[0])
            : "";
        exploreLabel = soleName
          ? t("stage.readPath", { path: soleName })
          : readFileCount === 1 && grepCount === 0 && globCount === 0
            ? t("stage.readFile")
            : t("stage.readFiles");
      } else if (grepCount > 0) {
        exploreLabel = t("stage.searchedFiles");
      } else {
        exploreLabel = t("stage.foundFiles");
      }
      buckets.push({
        label: exploreLabel,
        tier: 3,
        rawCallCount: exploreCalls,
        firstOrder: firstSeen.get("explore") ?? 0,
      });
    }

    if (webSearchCount > 0) {
      buckets.push({
        label: t("stage.searchedWeb"),
        tier: 3,
        rawCallCount: webSearchCount,
        firstOrder: firstSeen.get("search_web") ?? 0,
      });
    }

    if (commandCount > 0) {
      buckets.push({
        label: commandCount === 1 ? t("stage.ranCommand") : t("stage.ranCommands"),
        tier: 4,
        rawCallCount: commandCount,
        firstOrder: firstSeen.get("shell") ?? 0,
      });
    }

    if (otherToolCount > 0) {
      buckets.push({
        label: otherToolCount === 1 ? t("stage.usedTool") : t("stage.usedTools"),
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

    const formatted = buckets
      .map((b, idx) => (idx === 0 || locale !== "en" ? b.label : lowercaseClauseLead(b.label)))
      .join(locale === "en" ? ", " : "、");

    label = formatted || t("stage.usedTools");
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
