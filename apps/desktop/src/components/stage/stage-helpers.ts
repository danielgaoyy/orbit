import { useSyncExternalStore } from "react";
import { extractInlineArtifactDirectives } from "@/components/artifacts/mcp-app-bridge";
import type {
  ArtifactSummary,
  DiffFileView,
  DiffLineView,
  InlineSpanView,
  PromptBadge,
  RichBlockView,
  SessionInsightsView,
  StageTurnView,
  TimelineItemView,
  WorkSummaryResultView,
} from "@/lib/orbit-ipc";
import type { ResolvedSubagentItem } from "@/lib/subagents";

export const USER_PROMPT_PREVIEW_MAX_PX = 124;

export interface TurnTimingRecord {
  startedAtMs: number;
  completedAtMs: number | null;
  durationMs: number | null;
  workDurationMs: number | null;
}

export type ViewportAnchorHoldFn = (
  triggerEl: HTMLElement | null,
  apply: () => void,
  edge?: "top" | "bottom",
) => void;

export type ThinkingDurationLookupFn = (thinkKey: string) => number | null;

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

// Singleton Intl formatters & tooltip cache so N turns allocate zero formatter instances per render
const SHORT_DATE_SAME_YEAR_FMT =
  typeof Intl !== "undefined"
    ? new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" })
    : null;
const SHORT_DATE_WITH_YEAR_FMT =
  typeof Intl !== "undefined"
    ? new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric" })
    : null;
const FULL_TIMESTAMP_FMT =
  typeof Intl !== "undefined"
    ? new Intl.DateTimeFormat("en-US", {
        year: "numeric",
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      })
    : null;
const fullTimestampTooltipCache = new Map<number, string>();

// Shared singleton relative-time clocks (15s for recent < 1h, 60s for older >= 1h)
const RECENT_CLOCK_THRESHOLD_MS = 3_600_000;
let sharedRelativeNowMs = Date.now();
let sharedSlowRelativeNowMs = Date.now();
let sharedRelativeTimerId: number | null = null;
let sharedSlowRelativeTimerId: number | null = null;
const relativeClockListeners = new Set<() => void>();
const slowRelativeClockListeners = new Set<() => void>();

function subscribeRelativeClock(listener: () => void): () => void {
  relativeClockListeners.add(listener);
  if (sharedRelativeTimerId === null && typeof window !== "undefined") {
    sharedRelativeNowMs = Date.now();
    sharedRelativeTimerId = window.setInterval(() => {
      sharedRelativeNowMs = Date.now();
      for (const fn of relativeClockListeners) {
        fn();
      }
    }, 15_000);
  }
  return () => {
    relativeClockListeners.delete(listener);
    if (relativeClockListeners.size === 0 && sharedRelativeTimerId !== null) {
      window.clearInterval(sharedRelativeTimerId);
      sharedRelativeTimerId = null;
    }
  };
}

function subscribeSlowRelativeClock(listener: () => void): () => void {
  slowRelativeClockListeners.add(listener);
  if (sharedSlowRelativeTimerId === null && typeof window !== "undefined") {
    sharedSlowRelativeNowMs = Date.now();
    sharedSlowRelativeTimerId = window.setInterval(() => {
      sharedSlowRelativeNowMs = Date.now();
      for (const fn of slowRelativeClockListeners) {
        fn();
      }
    }, 60_000);
  }
  return () => {
    slowRelativeClockListeners.delete(listener);
    if (slowRelativeClockListeners.size === 0 && sharedSlowRelativeTimerId !== null) {
      window.clearInterval(sharedSlowRelativeTimerId);
      sharedSlowRelativeTimerId = null;
    }
  };
}

export function useSharedRelativeNow(timestampMs: number): number {
  const isRecent = Date.now() - timestampMs < RECENT_CLOCK_THRESHOLD_MS;
  return useSyncExternalStore(
    isRecent ? subscribeRelativeClock : subscribeSlowRelativeClock,
    () => (isRecent ? sharedRelativeNowMs : sharedSlowRelativeNowMs),
    () => (isRecent ? sharedRelativeNowMs : sharedSlowRelativeNowMs),
  );
}

export function formatRelativeTimeLabel(timestampMs: number, nowMs: number): string {
  const diffSec = Math.max(0, Math.floor((nowMs - timestampMs) / 1000));
  if (diffSec < 60) {
    return "just now";
  }
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) {
    return `${mins}m ago`;
  }
  const hours = Math.floor(mins / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  const days = Math.floor(hours / 24);
  if (days < 30) {
    return `${days}d ago`;
  }
  try {
    const d = new Date(timestampMs);
    const sameYear = d.getFullYear() === new Date(nowMs).getFullYear();
    const fmt = sameYear ? SHORT_DATE_SAME_YEAR_FMT : SHORT_DATE_WITH_YEAR_FMT;
    return fmt ? fmt.format(d) : `${days}d ago`;
  } catch {
    return `${days}d ago`;
  }
}

export function formatFullTimestampTooltip(timestampMs: number): string {
  const cached = fullTimestampTooltipCache.get(timestampMs);
  if (cached !== undefined) return cached;
  try {
    const formatted = FULL_TIMESTAMP_FMT ? FULL_TIMESTAMP_FMT.format(new Date(timestampMs)) : "";
    if (fullTimestampTooltipCache.size > 1000) {
      fullTimestampTooltipCache.clear();
    }
    fullTimestampTooltipCache.set(timestampMs, formatted);
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
    } else if (seg.kind === "assistant_message" || seg.kind === "notice") {
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

const historicalThinkingSigCache = new WeakMap<
  StageTurnView,
  { agentId: string; timingsVersion: number; sig: string }
>();

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
  const cached = historicalThinkingSigCache.get(turn);
  if (cached && cached.agentId === agentId && cached.timingsVersion === timingsVersion) {
    return cached.sig;
  }
  const sig = buildTurnThinkingSignature(turn, agentId, thinkingTimings);
  historicalThinkingSigCache.set(turn, { agentId, timingsVersion, sig });
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
      title: matched?.title ?? rawTitle ?? "Artifact",
      version: matched?.version,
      kindLabel: matched?.kind,
    });
  }
  return refs;
}

export function parseArtifactRefsFromTurnTools(
  turn: StageTurnView,
  knownArtifacts: readonly ArtifactSummary[],
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
          title: matched?.title ?? rawTitle ?? "Artifact",
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
  if (height <= 0 || !Number.isFinite(height)) return;
  if (sessionTurnHeightCache.has(key)) {
    sessionTurnHeightCache.delete(key);
  } else if (sessionTurnHeightCache.size >= SESSION_TURN_HEIGHT_CACHE_LIMIT) {
    const oldest = sessionTurnHeightCache.keys().next().value;
    if (oldest !== undefined) sessionTurnHeightCache.delete(oldest);
  }
  sessionTurnHeightCache.set(key, height);
}

export const TURN_LIST_GAP_PX = 32;
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
        (seg.kind === "assistant_message" || seg.kind === "notice") &&
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
