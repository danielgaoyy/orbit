import { useSyncExternalStore } from "react";
import { DEFAULT_LOCALE, translate, type OrbitLocale } from "../../lib/i18n/index.ts";
import { extractInlineArtifactDirectives } from "../artifacts/mcp-app-bridge.ts";
import type {
  ArtifactSummary,
  DiffFileView,
  DiffLineView,
  GuardianAssessment,
  InlineSpanView,
  PromptBadge,
  RichBlockView,
  SessionInsightsView,
  StageTurnView,
  TimelineItemView,
  WorkSummaryResultView,
} from "@/lib/orbit-ipc";
import {
  areResolvedSubagentsEqual,
  classifyGenericSubagentTool,
  type ResolvedSubagentItem,
  type SubagentNameMatcher,
} from "../../lib/subagents.ts";
import {
  selectPendingDecisions,
  selectSessionSubagents,
  type SessionSubagentWithAvatar,
  type UnifiedPendingDecisions,
} from "../../state/selectors/session-selectors.ts";
import type { OrbitUiState } from "../../state/store-types.ts";

export const USER_PROMPT_PREVIEW_MAX_PX = 124;

export interface TurnTimingRecord {
  startedAtMs: number;
  completedAtMs: number | null;
  durationMs: number | null;
  workDurationMs: number | null;
}

export interface ViewportAnchorHoldOptions {
  intent?: "expand" | "collapse";
}

export type ViewportAnchorHoldFn = (
  triggerEl: HTMLElement | null,
  apply: () => void,
  edge?: "top" | "bottom",
  options?: ViewportAnchorHoldOptions,
) => void;

export const STAGE_OCCLUSION_THRESHOLD_PX = 22;
export const STAGE_JUMP_LATEST_THRESHOLD_PX = 72;
export const STAGE_STICKY_REENGAGE_THRESHOLD_PX = 20;

export function computeStageScrollState({
  currentTop, previousTop, distanceFromBottom, stickToBottom,
  holdActive = false, isShellLayoutMoving = false, hasTurns = true,
}: {
  currentTop: number;
  previousTop: number;
  distanceFromBottom: number;
  stickToBottom: boolean;
  holdActive?: boolean;
  isShellLayoutMoving?: boolean;
  hasTurns?: boolean;
}) {
  // Chrome clamps scrollTop when content shrinks. Only movement within the new
  // scroll range can be a user scrolling up from the previously observed top.
  const reachablePreviousTop = Math.min(previousTop, currentTop + Math.max(0, distanceFromBottom));
  if (!holdActive) {
    if (currentTop < reachablePreviousTop - 1.5 && !isShellLayoutMoving) {
      stickToBottom = false;
    } else if (currentTop > previousTop && distanceFromBottom <= STAGE_STICKY_REENGAGE_THRESHOLD_PX) {
      stickToBottom = true;
    }
  }
  return {
    stickToBottom,
    isOccludingText: hasTurns && !stickToBottom && distanceFromBottom > STAGE_OCCLUSION_THRESHOLD_PX,
    showJumpLatest: hasTurns && !stickToBottom && distanceFromBottom > STAGE_JUMP_LATEST_THRESHOLD_PX,
  };
}

export function computeDisclosureRevealDelta({
  intent, beforeContainerHeight, containerRect, scrollerRect, triggerRect,
}: {
  intent?: ViewportAnchorHoldOptions["intent"];
  beforeContainerHeight: number;
  containerRect: Pick<DOMRect, "bottom" | "height">;
  scrollerRect: Pick<DOMRect, "top" | "bottom" | "height">;
  triggerRect: Pick<DOMRect, "top" | "bottom">;
}): number {
  if (intent === "collapse" || (!intent && containerRect.height < beforeContainerHeight - 2)) return 0;
  const maxNudge = Math.max(0, triggerRect.top - scrollerRect.top - 24);
  const overflowBelow = containerRect.bottom - scrollerRect.bottom;
  if (overflowBelow <= 8 || maxNudge <= 4) return 0;
  const fullReveal = Math.min(overflowBelow + 16, maxNudge);
  if (containerRect.height <= scrollerRect.height * 0.55) return fullReveal;
  const visibleBelowTrigger = Math.max(0, scrollerRect.bottom - triggerRect.bottom);
  const targetReveal = Math.min(240, Math.round(scrollerRect.height * 0.35));
  return Math.min(fullReveal, Math.max(0, targetReveal - visibleBelowTrigger));
}

export type ThinkingDurationLookupFn = (thinkKey: string) => number | null;

interface StageTurnTimings {
  timing?: TurnTimingRecord;
  activeThinkingKey: string | null;
  thinking: Record<string, TurnTimingRecord>;
  thinkingVersion: number;
  getThinkingDurationMs: ThinkingDurationLookupFn;
}

export const EMPTY_TURN_TIMINGS: StageTurnTimings = {
  activeThinkingKey: null,
  thinking: {},
  thinkingVersion: 0,
  getThinkingDurationMs: () => null,
};

// Evict a turn's version and durations together. A single long-running turn is bounded too.
const MAX_TIMED_TURNS = 200;
const MAX_THINKING_BLOCKS_PER_TURN = 200;
export type StageTimingHistory = ReadonlyMap<string, StageTurnTimings>;

export function updateStageTimingHistory(
  history: StageTimingHistory,
  hostId: string,
  agentId: string,
  turn: StageTurnView,
  active: boolean,
  now: number,
): StageTimingHistory {
  const key = `${hostId}::${agentId}::${turn.key}`;
  const previous = history.get(key) ?? EMPTY_TURN_TIMINGS;
  let { timing } = previous;
  if (active && (!timing || timing.completedAtMs !== null)) {
    const startedAtMs = Math.min(turn.started_at_ms ?? now, now);
    timing = { startedAtMs, completedAtMs: null, durationMs: null, workDurationMs: null };
  } else if (timing && timing.completedAtMs === null) {
    const elapsed = Math.max(0, now - timing.startedAtMs);
    if (!active) {
      timing = {
        ...timing,
        completedAtMs: now,
        durationMs: elapsed,
        workDurationMs: timing.workDurationMs ?? elapsed,
      };
    } else if (turn.terminal_reply && timing.workDurationMs === null) {
      timing = { ...timing, workDurationMs: elapsed };
    } else if (!turn.terminal_reply && timing.workDurationMs !== null) {
      timing = { ...timing, workDurationMs: null };
    }
  }

  const lastSegment = turn.live_segments.at(-1);
  const tail = active && lastSegment?.kind === "step_group" ? lastSegment.items.at(-1) : null;
  const tailKey = tail?.item_type === "assistant_thinking"
    ? `${agentId}::think::${tail.item_id || tail.seq}`
    : null;
  const activeThinkingKey = tailKey && previous.thinking[tailKey]?.completedAtMs == null ? tailKey : null;
  let entry = previous;
  if (activeThinkingKey !== previous.activeThinkingKey) {
    const thinking = { ...previous.thinking };
    if (previous.activeThinkingKey !== null) {
      const block = thinking[previous.activeThinkingKey];
      thinking[previous.activeThinkingKey] = {
        ...block, completedAtMs: now, durationMs: Math.max(0, now - block.startedAtMs),
      };
    }
    if (activeThinkingKey !== null) {
      thinking[activeThinkingKey] = {
        startedAtMs: now, completedAtMs: null, durationMs: null, workDurationMs: null,
      };
      const keys = Object.keys(thinking);
      if (keys.length > MAX_THINKING_BLOCKS_PER_TURN) delete thinking[keys[0]];
    }
    entry = {
      activeThinkingKey,
      thinking,
      thinkingVersion: previous.thinkingVersion + 1,
      getThinkingDurationMs: (thinkKey) => thinking[thinkKey]?.durationMs ?? null,
    };
  }

  if (timing === previous.timing && entry === previous) return history;
  const next = new Map(history);
  next.delete(key);
  next.set(key, { ...entry, timing });
  if (next.size > MAX_TIMED_TURNS) next.delete(next.keys().next().value!);
  return next;
}

export interface UserPromptPresentation {
  cleanText: string;
  badges: PromptBadge[];
  sanitizedLegacy: boolean;
}

export interface PromptImagePreviewTarget {
  url: string;
  label: string;
  path: string | null;
  hostId: string;
  cwd: string;
}

export interface RewindFileEntry {
  displayPath: string;
  basename: string;
  additions: number;
  deletions: number;
}

export interface RewindRequestTarget {
  seq: number;
  promptText: string;
  promptBadges: readonly PromptBadge[];
  affectedTurnsCount: number;
  editedFiles: RewindFileEntry[];
}

export interface InlineArtifactRef {
  key: string;
  artifactId?: string;
  title: string;
  version?: number;
  kindLabel?: string;
}

export interface StageScrollUiSnapshot {
  isOccludingText: boolean;
  showJumpLatest: boolean;
  activePromptSeq: number | null;
}

export interface StageScrollUiStore {
  getSnapshot: () => StageScrollUiSnapshot;
  subscribe: (listener: () => void) => () => void;
  update: (patch: Partial<StageScrollUiSnapshot>) => void;
}

export interface OutlinePromptWithIndex {
  seq: number;
  turnIndex: number;
  preview: string;
}

export interface ScrollVelocityState {
  offset: number;
  ts: number;
  dir: "up" | "down" | null;
  fast: boolean;
  wasScrolling: boolean;
}

export const EMPTY_SUBAGENTS: readonly ResolvedSubagentItem[] = [];
export const EMPTY_TIMELINE_ITEMS: TimelineItemView[] = [];
export const EMPTY_TURNS: StageTurnView[] = [];
export const EMPTY_DIFF_FILES: DiffFileView[] = [];
export const EMPTY_ARTIFACTS: readonly ArtifactSummary[] = [];
export const EMPTY_INSIGHTS: SessionInsightsView = {
  touched_files: [],
  background_tasks: [],
  subagents: [],
  detected_local_urls: [],
};
export const EMPTY_WORK_COUNTS: WorkSummaryResultView["counts"] = {
  edited_file_count: 0,
  created_file_count: 0,
  command_count: 0,
  read_file_count: 0,
  search_count: 0,
  subagent_count: 0,
  mcp_server_count: 0,
  other_tool_count: 0,
  thought_count: 0,
  narration_count: 0,
  action_steps: 0,
  total_steps: 0,
  failed_count: 0,
  denied_count: 0,
  unrecovered_failure_count: 0,
  additions: 0,
  deletions: 0,
};

function sameItems(a: readonly unknown[], b: readonly unknown[]): boolean {
  return a === b || (a.length === b.length && a.every((item, i) => item === b[i]));
}

function sameStageSubagents(a: readonly SessionSubagentWithAvatar[], b: readonly SessionSubagentWithAvatar[]) {
  return areResolvedSubagentsEqual(a, b) && a.every((sa, i) =>
    sa.character === b[i].character && sa.remoteHostName === b[i].remoteHostName &&
    sa.isManaged === b[i].isManaged && sa.childProvider === b[i].childProvider,
  );
}

/** Keep React's session snapshot stable across token deltas and unrelated host updates. */
export function createStageSessionSelector(hostId: string | null, agentId: string | null) {
  let inputs: readonly unknown[] = [];
  let snapshot: {
    subagents: readonly SessionSubagentWithAvatar[];
    decisions: UnifiedPendingDecisions;
  } | undefined;
  return (state: Pick<OrbitUiState, "hostDetails" | "fleet" | "stages" | "archivedSubagents">) => {
    const stage = hostId && agentId ? state.stages[`${hostId}::${agentId}`] : undefined;
    // Archive filters only affect the visible roster. Stage and decisions both use the full roster.
    const nextInputs = [
      state.hostDetails, state.fleet.hosts, stage?.insights?.subagents,
      stage?.pending_permissions, stage?.pending_questions, stage?.pending_plan_approval,
    ];
    if (snapshot && sameItems(inputs, nextInputs)) return snapshot;

    const all = selectSessionSubagents(state, hostId, agentId).all;
    const subagents = snapshot && sameStageSubagents(snapshot.subagents, all) ? snapshot.subagents : all;
    const decisions = selectPendingDecisions(state, hostId, agentId);
    inputs = nextInputs;
    if (snapshot && subagents === snapshot.subagents) {
      const prev = snapshot.decisions;
      if (
        prev.pendingPlanApproval === decisions.pendingPlanApproval &&
        sameItems(prev.parentPermissions, decisions.parentPermissions) &&
        sameItems(prev.parentQuestions, decisions.parentQuestions) &&
        prev.subagentPermissions.length === decisions.subagentPermissions.length &&
        decisions.subagentPermissions.every((item, i) =>
          item.key === prev.subagentPermissions[i].key && item.permission === prev.subagentPermissions[i].permission,
        )
      ) return snapshot;
    }
    snapshot = { subagents, decisions };
    return snapshot;
  };
}

const subagentReferences = new WeakMap<StageTurnView, boolean>();

export function turnHasSubagentReferences(turn: StageTurnView): boolean {
  const cached = subagentReferences.get(turn);
  if (cached !== undefined) return cached;
  const hasReferences = Boolean(turn.agent_message) || turn.live_segments.some((seg) => {
    if (seg.kind !== "step_group") return seg.kind === "agent_message";
    return seg.items.some((item) => {
      if (item.item_type !== "tool_call") return item.item_type === "agent_message";
      return item.detail.kind === "subagent" ||
        (item.detail.kind === "generic" && classifyGenericSubagentTool(item.detail.tool_name) !== null);
    });
  });
  subagentReferences.set(turn, hasReferences);
  return hasReferences;
}

/** Historical prose only depends on the roster when one of its rendered spans links a name. */
export function createTurnProseMatcherLookup(matcher: SubagentNameMatcher | null) {
  if (!matcher) return () => null;
  const cache = new WeakMap<StageTurnView, boolean>();
  const linksName = (span: InlineSpanView) => {
    if (span.link_url || span.math_ml) return false;
    return span.inline_code
      ? Boolean(matcher.matchExact(span.text))
      : matcher.splitText(span.text).some((part) => part.kind === "subagent");
  };
  return (turn: StageTurnView) => {
    let found = cache.get(turn);
    if (found === undefined) {
      found = turn.live_segments.some((seg) =>
        seg.kind === "assistant_message" && seg.item.item_type === "assistant_message" &&
        seg.item.blocks.some((block) => {
          if ("spans" in block) return block.spans.some(linksName);
          if (block.kind !== "table") return false;
          return block.headers.some((cell) => cell.spans.some(linksName)) ||
            block.rows.some((row) => row.cells.some((cell) => cell.spans.some(linksName)));
        }),
      );
      cache.set(turn, found);
    }
    return found ? matcher : null;
  };
}

// Keep formatter construction off the render path after the first use of each locale.
const dateStyles = {
  sameYear: { month: "short", day: "numeric" },
  withYear: { month: "short", day: "numeric", year: "numeric" },
  full: { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false },
} satisfies Record<string, Intl.DateTimeFormatOptions>;
const dateFormatters = new Map<string, Intl.DateTimeFormat>();
const fullTimestampTooltipCache = new Map<string, string>();

function stageDateFormatter(locale: OrbitLocale, style: keyof typeof dateStyles): Intl.DateTimeFormat {
  const key = `${locale}:${style}`;
  let formatter = dateFormatters.get(key);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(locale, dateStyles[style]);
    dateFormatters.set(key, formatter);
  }
  return formatter;
}

// Shared singleton relative-time clocks (15s for recent < 1h, 60s for older >= 1h)
const RECENT_CLOCK_THRESHOLD_MS = 3_600_000;
export function createRelativeClock(intervalMs: number) {
  const listeners = new Set<() => void>();
  let now = Date.now();
  let interval: number | null = null;

  function tick() {
    if (typeof document !== "undefined" && document.hidden) return;
    now = Date.now();
    for (const listener of listeners) listener();
  }

  function stop() {
    if (interval !== null) window.clearInterval(interval);
    interval = null;
  }

  function syncVisibility() {
    if (typeof document !== "undefined" && document.hidden) {
      stop();
    } else if (interval === null) {
      interval = window.setInterval(tick, intervalMs);
      tick();
    }
  }

  function subscribe(listener: () => void) {
    listeners.add(listener);
    if (listeners.size === 1 && typeof window !== "undefined") {
      if (typeof document !== "undefined") document.addEventListener("visibilitychange", syncVisibility);
      syncVisibility();
    }
    return () => {
      listeners.delete(listener);
      if (listeners.size === 0) {
        if (typeof document !== "undefined") document.removeEventListener("visibilitychange", syncVisibility);
        stop();
      }
    };
  }

  return { subscribe, getSnapshot: () => now };
}

const relativeClock = createRelativeClock(15_000);
const slowRelativeClock = createRelativeClock(60_000);

export function useSharedRelativeNow(timestampMs: number): number {
  const clock = Date.now() - timestampMs < RECENT_CLOCK_THRESHOLD_MS ? relativeClock : slowRelativeClock;
  return useSyncExternalStore(clock.subscribe, clock.getSnapshot, clock.getSnapshot);
}

export function formatRelativeTimeLabel(
  timestampMs: number,
  nowMs: number,
  locale: OrbitLocale = DEFAULT_LOCALE,
): string {
  const diffSec = Math.max(0, Math.floor((nowMs - timestampMs) / 1000));
  if (diffSec < 60) {
    return translate(locale, "palette.justNow");
  }
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) {
    return translate(locale, "palette.minutesAgo", { n: mins });
  }
  const hours = Math.floor(mins / 60);
  if (hours < 24) {
    return translate(locale, "palette.hoursAgo", { n: hours });
  }
  const days = Math.floor(hours / 24);
  if (days < 30) {
    return translate(locale, "palette.daysAgo", { n: days });
  }
  try {
    const d = new Date(timestampMs);
    const sameYear = d.getFullYear() === new Date(nowMs).getFullYear();
    return stageDateFormatter(locale, sameYear ? "sameYear" : "withYear").format(d);
  } catch {
    return translate(locale, "palette.daysAgo", { n: days });
  }
}

export function formatFullTimestampTooltip(timestampMs: number, locale: OrbitLocale = DEFAULT_LOCALE): string {
  const key = `${locale}:${timestampMs}`;
  const cached = fullTimestampTooltipCache.get(key);
  if (cached !== undefined) return cached;
  try {
    const formatted = stageDateFormatter(locale, "full").format(timestampMs);
    if (fullTimestampTooltipCache.size > 1000) {
      fullTimestampTooltipCache.clear();
    }
    fullTimestampTooltipCache.set(key, formatted);
    return formatted;
  } catch {
    return "";
  }
}

export function formatJsonPreview(rawJson: string): string {
  const trimmed = rawJson.trim();
  if (!trimmed) return "";
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return trimmed;
  }
}

export function parseUnifiedDiffFallback(unifiedDiff: string): DiffLineView[] {
  const out: DiffLineView[] = [];
  for (const raw of unifiedDiff.split(/\r?\n/)) {
    if (
      raw.startsWith("diff --git") ||
      raw.startsWith("index ") ||
      raw.startsWith("--- ") ||
      raw.startsWith("+++ ")
    ) {
      continue;
    }
    if (raw.startsWith("@@")) {
      out.push({ kind: "hunk_header", old_line: null, new_line: null, content: raw });
    } else if (raw.startsWith("+")) {
      out.push({ kind: "added", old_line: null, new_line: null, content: raw.slice(1) });
    } else if (raw.startsWith("-")) {
      out.push({ kind: "removed", old_line: null, new_line: null, content: raw.slice(1) });
    } else {
      out.push({
        kind: "context",
        old_line: null,
        new_line: null,
        content: raw.startsWith(" ") ? raw.slice(1) : raw,
      });
    }
  }
  return out;
}

export function stableStepItemKey(item: TimelineItemView, idx: number): string {
  switch (item.item_type) {
    case "assistant_thinking":
      return item.item_id ? `think-${item.item_id}` : `think-seq-${item.seq}-${idx}`;
    case "assistant_message":
      return item.item_id ? `msg-${item.item_id}` : `msg-seq-${item.seq}-${idx}`;
    case "tool_call":
      return item.call_id ? `tool-${item.call_id}` : `tool-seq-${item.seq}-${idx}`;
    case "permission_request":
      return item.request_id ? `perm-${item.request_id}` : `perm-seq-${item.seq}-${idx}`;
    case "question_request":
      return item.request_id ? `q-${item.request_id}` : `q-seq-${item.seq}-${idx}`;
    case "user_prompt":
      return item.message_id ? `prompt-${item.message_id}` : `prompt-seq-${item.seq}-${idx}`;
    case "agent_message":
      return item.message_id ? `am-${item.message_id}` : `am-seq-${item.seq}-${idx}`;
    case "notice":
      return `notice-${item.seq}-${idx}`;
  }
}

const LEGACY_PLAN_PREFIX =
  "Analyze the relevant codebase and propose a concise step-by-step plan before making edits:";
const LEGACY_REVIEW_PROMPT =
  "Review the uncommitted working tree changes for bugs, edge cases, and architectural consistency.";

const promptPresentationCache = new WeakMap<
  Extract<TimelineItemView, { item_type: "user_prompt" }>,
  UserPromptPresentation
>();

/**
 * Extracts clean user-visible text and semantic chips from `user_prompt`,
 * including backwards-compatible cleanup for older turns stored before structured `badges`.
 * Memoized by `WeakMap` so `UserPromptBubble` and `outlinePrompts` run regexes at most once per item.
 */
export function extractUserPromptPresentation(
  item: Extract<TimelineItemView, { item_type: "user_prompt" }>,
): UserPromptPresentation {
  const cached = promptPresentationCache.get(item);
  if (cached) return cached;

  if (item.badges && item.badges.length > 0) {
    const res: UserPromptPresentation = {
      cleanText: item.text.trim(),
      badges: item.badges,
      sanitizedLegacy: false,
    };
    promptPresentationCache.set(item, res);
    return res;
  }

  let working = item.text;
  const inferredBadges: PromptBadge[] = [];
  let sanitized = false;

  working = working.replace(/^Attached image file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "image", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Attached PDF file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "pdf", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Attached audio file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "audio", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Attached video file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "video", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Attached file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "file", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Referenced workspace files:\s*(.+)$/gm, (_, listStr: string) => {
    sanitized = true;
    const matches = Array.from(listStr.matchAll(/`@?([^`]+)`/g));
    for (const m of matches) {
      const rawPath = m[1];
      const name = rawPath.split("/").pop() || rawPath;
      inferredBadges.push({ kind: "file", label: name, path: rawPath });
    }
    return "";
  });

  working = working.replace(
    /^>\s*(Working Tree Diff|Staged Diff):\n```diff\n[\s\S]*?\n```\s*$/gm,
    (_, label: string) => {
      sanitized = true;
      inferredBadges.push({ kind: "git_diff", label });
      return "";
    },
  );
  working = working.replace(
    /^>\s*Context:\s*Inspect (staged|working tree) git changes\.\s*$/gim,
    (_, kind: string) => {
      sanitized = true;
      inferredBadges.push({
        kind: "git_diff",
        label: kind.toLowerCase() === "staged" ? "Staged Diff" : "Working Tree Diff",
      });
      return "";
    },
  );
  working = working.replace(
    /^>\s*([^:\n]*lines):\n```text\n[\s\S]*?\n```\s*$/gm,
    (_, label: string) => {
      sanitized = true;
      inferredBadges.push({ kind: "terminal", label: label.trim() });
      return "";
    },
  );
  working = working.replace(
    /^>\s*Reference\s*`([^`]+)`:\n```[\s\S]*?\n```\s*$/gm,
    (_, loc: string) => {
      sanitized = true;
      const shortLoc = loc.split("/").pop() || loc;
      inferredBadges.push({ kind: "quote", label: shortLoc, path: loc });
      return "";
    },
  );

  working = working.trim();

  if (working.startsWith(LEGACY_PLAN_PREFIX)) {
    sanitized = true;
    inferredBadges.unshift({ kind: "slash", label: "/plan" });
    working = working.slice(LEGACY_PLAN_PREFIX.length).trim();
  } else if (working === LEGACY_REVIEW_PROMPT) {
    sanitized = true;
    inferredBadges.unshift({ kind: "slash", label: "/review" });
    working = "";
  }

  const res: UserPromptPresentation = {
    cleanText: working,
    badges: inferredBadges,
    sanitizedLegacy: sanitized,
  };
  promptPresentationCache.set(item, res);
  return res;
}

export function isRevertibleWorkspaceFile(rawPath: string, cwd: string): boolean {
  const trimmed = rawPath.trim();
  if (!trimmed || trimmed === "Working Tree" || trimmed === "Staged Diff") {
    return false;
  }
  const cleanCwd = cwd.replace(/\/+$/, "");
  if (trimmed.startsWith("/") && cleanCwd && cleanCwd !== "~") {
    return trimmed === cleanCwd || trimmed.startsWith(`${cleanCwd}/`);
  }
  return true;
}

export function resolveTurnForkSeq(turn: StageTurnView): number | null {
  let maxSeq = 0;
  const record = (seq: number | undefined | null) => {
    if (typeof seq === "number" && seq > maxSeq) maxSeq = seq;
  };
  record(turn.anchor_seq);
  record(turn.user_prompt?.seq);
  record(turn.agent_message?.seq);
  record(turn.terminal_reply?.seq);
  record(turn.last_assistant_reply?.seq);
  for (const item of turn.folded_steps) record(item.seq);
  for (const item of turn.notices) record(item.seq);
  for (const item of turn.compactions) record(item.seq);
  for (const seg of turn.live_segments) {
    if (seg.kind === "step_group") {
      for (const item of seg.items) record(item.seq);
    } else if (seg.kind === "assistant_message" || seg.kind === "agent_message" || seg.kind === "notice") {
      record(seg.item.seq);
    }
  }
  return maxSeq > 0 ? maxSeq : null;
}

export function isWaitingWakeupLabel(label: string | undefined | null): boolean {
  return Boolean(label && /^waiting for\b/i.test(label.trim()));
}

export function isHiddenWakeupLabel(label: string | undefined | null): boolean {
  if (!label) return true;
  const trimmed = label.trim();
  return (
    /^waiting for\b/i.test(trimmed) ||
    /^resumed from background task\b/i.test(trimmed)
  );
}

export function computeTotalThinkingDurationMs(
  items: TimelineItemView[],
  agentId: string,
  getThinkingDurationMs: ThinkingDurationLookupFn,
): number | null {
  let total = 0;
  let foundAny = false;
  for (const it of items) {
    if (it.item_type !== "assistant_thinking") continue;
    const dur = getThinkingDurationMs(`${agentId}::think::${it.item_id || it.seq}`);
    if (dur !== null) {
      total += dur;
      foundAny = true;
    }
  }
  return foundAny ? total : null;
}

export function buildTurnThinkingSignature(
  turn: StageTurnView,
  agentId: string,
  thinkingTimings: Record<string, TurnTimingRecord>,
): string {
  let sig = "";
  for (const seg of turn.live_segments) {
    if (seg.kind !== "step_group") continue;
    for (const it of seg.items) {
      if (it.item_type !== "assistant_thinking") continue;
      const key = `${agentId}::think::${it.item_id || it.seq}`;
      const dur = thinkingTimings[key]?.durationMs;
      if (dur !== null && dur !== undefined) {
        sig += `${it.seq}:${dur};`;
      }
    }
  }
  return sig;
}

type HistoricalSignatures = WeakMap<StageTurnView, { agentId: string; timingsVersion: number; sig: string }>;
const historicalThinkingSigCache = new WeakMap<Record<string, TurnTimingRecord>, HistoricalSignatures>();

/**
 * O(1) WeakMap-memoized thinking signature lookup for immutable historical turns so
 * live token deltas on the active tail turn never scan steps across all warm turns.
 */
export function getCachedHistoricalThinkingSignature(
  turn: StageTurnView,
  agentId: string,
  thinkingTimings: Record<string, TurnTimingRecord>,
  timingsVersion: number,
  isTurnActive: boolean,
): string {
  if (isTurnActive) {
    return buildTurnThinkingSignature(turn, agentId, thinkingTimings);
  }
  let signatures = historicalThinkingSigCache.get(thinkingTimings);
  if (!signatures) {
    signatures = new WeakMap();
    historicalThinkingSigCache.set(thinkingTimings, signatures);
  }
  const cached = signatures.get(turn);
  if (cached && cached.agentId === agentId && cached.timingsVersion === timingsVersion) {
    return cached.sig;
  }
  const sig = buildTurnThinkingSignature(turn, agentId, thinkingTimings);
  signatures.set(turn, { agentId, timingsVersion, sig });
  return sig;
}

export function areArtifactListsEquivalent(
  a: readonly ArtifactSummary[],
  b: readonly ArtifactSummary[],
): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const x = a[i];
    const y = b[i];
    if (
      x.artifact_id !== y.artifact_id ||
      x.version !== y.version ||
      x.title !== y.title ||
      x.kind !== y.kind ||
      x.resource_uri !== y.resource_uri
    ) {
      return false;
    }
  }
  return true;
}

export function parseArtifactRefsFromMessage(
  text: string,
  knownArtifacts: readonly ArtifactSummary[],
  locale: OrbitLocale = DEFAULT_LOCALE,
): InlineArtifactRef[] {
  const directives = extractInlineArtifactDirectives(text);
  if (directives.length === 0) {
    return [];
  }
  const refs: InlineArtifactRef[] = [];
  const seen = new Set<string>();
  for (const dir of directives) {
    const explicitId = dir.id;
    const rawTitle = dir.title;
    const rawUri = dir.resourceUri;

    const matched =
      (explicitId ? knownArtifacts.find((a) => a.artifact_id === explicitId) : undefined) ??
      (rawUri ? knownArtifacts.find((a) => a.resource_uri === rawUri) : undefined) ??
      (rawTitle
        ? knownArtifacts.find((a) => a.title.toLowerCase() === rawTitle.toLowerCase())
        : undefined);

    const key = matched?.artifact_id || explicitId || rawTitle || rawUri || `idx_${dir.index}`;
    if (seen.has(key)) continue;
    seen.add(key);

    refs.push({
      key,
      artifactId: matched?.artifact_id ?? explicitId,
      title: matched?.title ?? rawTitle ?? translate(locale, "stage.artifact"),
      version: matched?.version,
      kindLabel: matched?.kind,
    });
  }
  return refs;
}

export function parseArtifactRefsFromTurnTools(
  turn: StageTurnView,
  knownArtifacts: readonly ArtifactSummary[],
  locale: OrbitLocale = DEFAULT_LOCALE,
): InlineArtifactRef[] {
  const refs: InlineArtifactRef[] = [];
  const seen = new Set<string>();
  const inspectItems = (items: readonly TimelineItemView[]) => {
    for (const item of items) {
      if (item.item_type !== "tool_call") continue;
      if (item.detail.kind !== "generic") continue;
      const toolName = item.detail.tool_name.toLowerCase();
      if (!toolName.endsWith("publish_artifact")) continue;
      try {
        const parsed = JSON.parse(item.detail.arguments_json) as Record<string, unknown>;
        const explicitId =
          typeof parsed.artifact_id === "string" && parsed.artifact_id.trim()
            ? parsed.artifact_id.trim()
            : undefined;
        const rawTitle =
          typeof parsed.title === "string" && parsed.title.trim()
            ? parsed.title.trim()
            : undefined;
        const matched =
          (explicitId ? knownArtifacts.find((a) => a.artifact_id === explicitId) : undefined) ??
          (rawTitle
            ? knownArtifacts.find((a) => a.title.toLowerCase() === rawTitle.toLowerCase())
            : undefined);
        const key = matched?.artifact_id || explicitId || rawTitle || item.call_id;
        if (seen.has(key)) continue;
        seen.add(key);
        refs.push({
          key,
          artifactId: matched?.artifact_id ?? explicitId,
          title: matched?.title ?? rawTitle ?? translate(locale, "stage.artifact"),
          version: matched?.version,
          kindLabel: matched?.kind ?? (typeof parsed.kind === "string" ? parsed.kind : undefined),
        });
      } catch {
        // Ignore unparseable partial tool arguments while streaming
      }
    }
  };
  inspectItems(turn.folded_steps);
  for (const seg of turn.live_segments) {
    if (seg.kind === "step_group") {
      inspectItems(seg.items);
    }
  }
  return refs;
}

export function createStageScrollUiStore(): StageScrollUiStore {
  let snap: StageScrollUiSnapshot = {
    isOccludingText: false,
    showJumpLatest: false,
    activePromptSeq: null,
  };
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => snap,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    update: (patch) => {
      const nextOcclusion = patch.isOccludingText ?? snap.isOccludingText;
      const nextJump = patch.showJumpLatest ?? snap.showJumpLatest;
      const nextActive =
        patch.activePromptSeq !== undefined ? patch.activePromptSeq : snap.activePromptSeq;
      if (
        nextOcclusion === snap.isOccludingText &&
        nextJump === snap.showJumpLatest &&
        nextActive === snap.activePromptSeq
      ) {
        return;
      }
      snap = {
        isOccludingText: nextOcclusion,
        showJumpLatest: nextJump,
        activePromptSeq: nextActive,
      };
      for (const listener of listeners) {
        listener();
      }
    },
  };
}

const SESSION_TURN_HEIGHT_CACHE_LIMIT = 1000;
const sessionTurnHeightCache = new Map<string, number>();

export function getCachedTurnHeight(key: string): number | undefined {
  return sessionTurnHeightCache.get(key);
}

export function setCachedTurnHeight(key: string, height: number): void {
  if (height < 0 || !Number.isFinite(height)) return;
  if (sessionTurnHeightCache.has(key)) {
    sessionTurnHeightCache.delete(key);
  } else if (sessionTurnHeightCache.size >= SESSION_TURN_HEIGHT_CACHE_LIMIT) {
    const oldest = sessionTurnHeightCache.keys().next().value;
    if (oldest !== undefined) sessionTurnHeightCache.delete(oldest);
  }
  sessionTurnHeightCache.set(key, height);
}

export const TURN_LIST_GAP_PX = 28;
export const CONTINUATION_TURN_GAP_PX = 12;

export function isCompactionMarkerTurn(turn: StageTurnView): boolean {
  return turn.is_compaction_command && turn.folded_steps.length === 0 &&
    turn.terminal_reply === null && turn.notices.length === 0;
}

type FooterContent = { replyText: string | null; forkUpToSeq: number | null };
const footerContentCache = new WeakMap<StageTurnView, FooterContent | null>();

export function getTurnFooter(
  turn: StageTurnView,
  { isActive, isLastTurn, canFork, timing }: {
    isActive?: boolean;
    isLastTurn?: boolean;
    canFork: boolean;
    timing?: TurnTimingRecord;
  },
) {
  if (isActive) return null;
  let content = footerContentCache.get(turn);
  if (content === undefined) {
    // Coalescing a finish into its peer report still leaves a body. A turn made
    // up entirely of suppressed wakeups or an opening prompt has no footer slot.
    const hasBody = !isCompactionMarkerTurn(turn) && turn.live_segments.some(
      (seg) => seg.kind !== "wakeup" || !isHiddenWakeupLabel(seg.label),
    );
    content = hasBody ? {
      replyText: turn.last_assistant_reply?.item_type === "assistant_message"
        ? turn.last_assistant_reply.text.trim() || null : null,
      forkUpToSeq: resolveTurnForkSeq(turn),
    } : null;
    footerContentCache.set(turn, content);
  }
  if (!content) return null;
  const completedAtMs = timing?.completedAtMs ?? turn.completed_at_ms;
  const showFork = canFork && (isLastTurn || content.forkUpToSeq !== null);
  if (!content.replyText && !showFork && completedAtMs === null) return null;
  return { ...content, completedAtMs, showFork };
}

/** The preceding historical footer already supplies 24px of separation. */
export function getTurnTopGapPx(turn: StageTurnView, previousHasFooter: boolean | undefined): number {
  if (previousHasFooter === undefined) return 0;
  if (turn.user_prompt !== null) return previousHasFooter ? 16 : TURN_LIST_GAP_PX;
  return previousHasFooter ? 0 : CONTINUATION_TURN_GAP_PX;
}

export interface TurnListRow {
  key: string;
  topGapPx: number;
  /** Top padding plus the fixed footer slot, both excluded from cached content heights. */
  spacingPx: number;
}

export function getTurnItemHeight(turn: StageTurnView, row: TurnListRow, contentHeight?: number): number {
  return row.spacingPx + (contentHeight ?? estimateTurnSizeFallback(turn));
}

export function getTurnContentStart(row: TurnListRow, itemStart: number): number {
  return itemStart + row.topGapPx;
}

export function getTurnListRows(
  turns: readonly StageTurnView[],
  scope: string | null,
  timings: StageTimingHistory,
  isLatestTurnActive: boolean,
  previousRows: readonly TurnListRow[] = [],
): readonly TurnListRow[] {
  let changedRows: TurnListRow[] | undefined;
  let previousHasFooter: boolean | undefined;
  for (const [index, turn] of turns.entries()) {
    const isLastTurn = index === turns.length - 1;
    const hasFooter = getTurnFooter(turn, {
      isActive: isLastTurn && isLatestTurnActive,
      isLastTurn,
      canFork: true,
      timing: timings.get(`${scope}::${turn.key}`)?.timing,
    }) !== null;
    const topGapPx = getTurnTopGapPx(turn, previousHasFooter);
    const footerPx = hasFooter ? (isLastTurn ? 28 : 24) : 0;
    const spacingPx = topGapPx + footerPx;
    const row = previousRows[index];
    if (row?.key === turn.key && row.topGapPx === topGapPx && row.spacingPx === spacingPx) {
      changedRows?.push(row);
    } else {
      changedRows ??= previousRows.slice(0, index);
      changedRows.push({ key: turn.key, topGapPx, spacingPx });
    }
    previousHasFooter = hasFooter;
  }
  // Stable rows preserve TanStack's measurements across live token deltas.
  return changedRows ?? (turns.length === previousRows.length ? previousRows : previousRows.slice(0, turns.length));
}

export const IDLE_LOOKAHEAD_UP_PX = 900;
export const IDLE_LOOKAHEAD_DOWN_PX = 1400;
export const SCROLL_LEADING_PX = 3600;
export const SCROLL_FAST_LEADING_PX = 1800;
export const SCROLL_TRAILING_PX = 1200;
export const FAST_FLICK_PX_PER_SEC = 2200;
export const MAX_OVERSCAN_ITEMS_PER_SIDE = 6;
export const WARM_RESTING_CAP = 10;
export const WARM_SCROLLING_CAP = 14;
export const IDLE_PREWARM_BATCH = 2;

function spanCharLen(spans: InlineSpanView[]): number {
  let len = 0;
  for (const s of spans) {
    len += s.text.length;
  }
  return len;
}

function estimateRichBlockHeight(block: RichBlockView): number {
  switch (block.kind) {
    case "code_block":
      return Math.min(1400, Math.max(84, block.lines.length * 21 + 52));
    case "table":
      return Math.min(1200, Math.max(96, (block.rows.length + 1) * 38 + 24));
    case "heading":
      return 42;
    case "list_item":
      return Math.max(28, Math.ceil(spanCharLen(block.spans) / 68) * 24 + 8);
    case "paragraph":
      return Math.max(34, Math.ceil(spanCharLen(block.spans) / 72) * 25 + 14);
    case "block_quote":
      return Math.max(44, Math.ceil(spanCharLen(block.spans) / 66) * 24 + 16);
    case "math_block":
      return Math.max(68, block.tex.split("\n").length * 28 + 40);
    case "horizontal_rule":
      return 28;
  }
}

export function estimateTurnSizeFallback(turn: StageTurnView | undefined): number {
  if (!turn) return 180;
  let est = 24;
  if (turn.user_prompt?.item_type === "user_prompt") {
    const promptLen = turn.user_prompt.text?.length ?? 0;
    const badges = turn.user_prompt.badges ?? [];
    const imageCount = badges.filter((b) => b.kind === "image").length;
    const otherBadgeCount = badges.length - imageCount;
    est =
      56 +
      Math.min(124, Math.ceil(promptLen / 56) * 24) +
      (otherBadgeCount > 0 ? 28 : 0) +
      (imageCount > 0 ? Math.min(204, Math.ceil(imageCount / 5) * 102) : 0);
  }
  if (turn.agent_message?.item_type === "agent_message") {
    est += 110;
  }
  for (const seg of turn.live_segments) {
    if (seg.kind === "assistant_message" && seg.item.item_type === "assistant_message") {
      if (seg.item.blocks.length === 0) {
        est += 48;
      } else {
        let blockSum = 16;
        for (const block of seg.item.blocks) {
          blockSum += estimateRichBlockHeight(block);
        }
        est += blockSum;
      }
    } else if (seg.kind === "step_group") {
      est += 34;
    } else if (seg.kind === "wakeup") {
      if (!isHiddenWakeupLabel(seg.label)) {
        est += 28;
      }
    } else if (seg.kind === "compaction") {
      est += 32;
    } else {
      est += 44;
    }
  }
  if (turn.turn_diffs.length > 0) est += 38;
  return Math.max(72, Math.min(5600, est));
}

export function findTurnIndexForSeq(
  turns: StageTurnView[],
  targetSeq: number,
): { index: number; exact: boolean } {
  if (turns.length === 0) return { index: -1, exact: false };
  for (let i = 0; i < turns.length; i += 1) {
    const t = turns[i];
    if (
      t.anchor_seq === targetSeq ||
      t.user_prompt?.seq === targetSeq ||
      t.agent_message?.seq === targetSeq ||
      t.terminal_reply?.seq === targetSeq ||
      t.last_assistant_reply?.seq === targetSeq ||
      t.folded_steps.some((item) => item.seq === targetSeq) ||
      t.notices.some((item) => item.seq === targetSeq) ||
      t.compactions.some((item) => item.seq === targetSeq)
    ) {
      return { index: i, exact: true };
    }
    for (const seg of t.live_segments) {
      if (seg.kind === "step_group" && seg.items.some((item) => item.seq === targetSeq)) {
        return { index: i, exact: true };
      }
      if (
        (seg.kind === "assistant_message" || seg.kind === "agent_message" || seg.kind === "notice") &&
        seg.item.seq === targetSeq
      ) {
        return { index: i, exact: true };
      }
    }
  }
  let best = 0;
  for (let i = 0; i < turns.length; i += 1) {
    if (turns[i].anchor_seq <= targetSeq) {
      best = i;
    } else {
      break;
    }
  }
  return { index: best, exact: false };
}

export type SafeBatchDecisionCandidate =
  | {
      readonly kind: "permission";
      readonly targetHostId?: string;
      readonly targetAgentId?: string;
      readonly requestId?: string;
      readonly allowedDecisions?: readonly string[];
      readonly guardianAssessment?: GuardianAssessment | null;
      readonly isPlanApproval?: boolean;
      readonly planApproval?: unknown;
    }
  | {
      readonly kind: "question";
      readonly targetHostId?: string;
      readonly targetAgentId?: string;
      readonly requestId?: string;
    };

/**
 * Returns `true` strictly when a permission request explicitly permits a single-shot `"allow"`
 * decision (`allow`, `allow_once`, or `accept`).
 */
export function permitsAllowDecision(allowedDecisions: readonly string[] | undefined): boolean {
  if (!allowedDecisions || allowedDecisions.length === 0) return false;
  return allowedDecisions.some((dec) => {
    const norm = dec.trim().toLowerCase();
    return norm === "allow" || norm === "allow_once" || norm === "accept";
  });
}

/**
 * Pure selector returning strictly non-plan, non-question, non-`High`/non-`Critical` pending
 * permissions that explicitly permit `"allow"`. Mirrors daemon-side `ResolvePermissionsBatch`
 * safety invariants.
 */
export function selectSafeBatchPermissions<T extends SafeBatchDecisionCandidate>(
  queue: readonly T[],
): Extract<T, { kind: "permission" }>[] {
  const out: Extract<T, { kind: "permission" }>[] = [];
  for (const item of queue) {
    if (item.kind !== "permission") continue;
    if (item.isPlanApproval || item.planApproval != null) continue;
    const risk = item.guardianAssessment?.risk_level;
    if (risk === "high" || risk === "critical") continue;
    if (!permitsAllowDecision(item.allowedDecisions)) continue;
    out.push(item as Extract<T, { kind: "permission" }>);
  }
  return out;
}
