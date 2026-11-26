import type {
  ConversationSearchHit,
  StageTurnView,
  TimelineItemView,
  ToolCallDetail,
} from "../../lib/orbit-ipc.ts";
import {
  extractDirectAgentMessagePresentation,
} from "../../lib/agent-message-clean.ts";
import {
  isDirectAgentPromptBubble,
  isTimelineAgentMessageHidden,
} from "../../lib/turn-delegation-split.ts";
import { findTurnIndexForSeq } from "./stage-helpers.ts";

export const STAGE_FIND_REMOTE_DEBOUNCE_MS = 180;
export const STAGE_FIND_REMOTE_LIMIT = 100;
const MAX_OCCURRENCES_PER_EVENT = 25;

export interface StageFindMatch {
  /** Stable identity across pagination and remote hit merges (`${seq}:${occurrenceInEvent}`). */
  key: string;
  seq: number;
  occurrenceInEvent: number;
  /** 0-based visible prose occurrence index inside the turn DOM, or `null` when folded/unloaded. */
  visibleOccurrenceInTurn: number | null;
  turnIndex: number | null;
  turnKey: string | null;
  role: string;
  snippet?: string;
  unloaded: boolean;
  source: "loaded" | "remote";
}

interface SearchableEventSlice {
  seq: number;
  role: string;
  text: string;
  visibleInDom: boolean;
}

function extractToolCallSearchText(detail: ToolCallDetail, summaryTitle: string): string {
  const parts: string[] = [];
  if (summaryTitle) parts.push(summaryTitle);
  switch (detail.kind) {
    case "shell":
      if (detail.command) parts.push(detail.command);
      if (detail.output_preview) parts.push(detail.output_preview);
      break;
    case "file_edit":
      if (detail.path) parts.push(detail.path);
      break;
    case "file_read":
      if (detail.path) parts.push(detail.path);
      break;
    case "search":
      if (detail.query) parts.push(detail.query);
      if (detail.scope) parts.push(detail.scope);
      break;
    case "subagent":
      if (detail.role) parts.push(detail.role);
      if (detail.prompt_preview) parts.push(detail.prompt_preview);
      break;
    case "generic":
      if (detail.tool_name) parts.push(detail.tool_name);
      if (detail.arguments_json) parts.push(detail.arguments_json);
      break;
  }
  return parts.join(" ");
}

function extractItemSearchSlice(
  item: TimelineItemView,
  visibleInDom: boolean,
): SearchableEventSlice | null {
  switch (item.item_type) {
    case "user_prompt":
      return item.text
        ? { seq: item.seq, role: "user", text: item.text, visibleInDom }
        : null;
    case "assistant_message":
      return item.text
        ? { seq: item.seq, role: "assistant", text: item.text, visibleInDom }
        : null;
    case "agent_message": {
      if (isDirectAgentPromptBubble(item)) {
        const cleaned = extractDirectAgentMessagePresentation(item.text, item).cleanText;
        return cleaned
          ? { seq: item.seq, role: "agent", text: cleaned, visibleInDom }
          : null;
      }
      const combined = [item.source_title, item.event, item.text]
        .filter((s): s is string => Boolean(s && s.trim()))
        .join(" ");
      return combined
        ? { seq: item.seq, role: "agent", text: combined, visibleInDom }
        : null;
    }
    case "notice":
      return item.message
        ? { seq: item.seq, role: "notice", text: item.message, visibleInDom }
        : null;
    case "tool_call": {
      const text = extractToolCallSearchText(item.detail, item.summary_title);
      return text
        ? { seq: item.seq, role: "tool", text, visibleInDom: false }
        : null;
    }
    case "question_request": {
      const parts: string[] = [];
      for (const q of item.questions) {
        if (q.header) parts.push(q.header);
        if (q.question) parts.push(q.question);
      }
      if (item.resolved_answers) {
        for (const group of item.resolved_answers) {
          for (const ans of group) parts.push(ans);
        }
      }
      const text = parts.join(" ");
      return text
        ? { seq: item.seq, role: "assistant", text, visibleInDom: false }
        : null;
    }
    case "assistant_thinking":
      return item.text
        ? { seq: item.seq, role: "thinking", text: item.text, visibleInDom: false }
        : null;
    case "permission_request":
      return item.title
        ? { seq: item.seq, role: "tool", text: item.title, visibleInDom: false }
        : null;
  }
}

function collectTurnSearchSlices(turn: StageTurnView): SearchableEventSlice[] {
  const slices: SearchableEventSlice[] = [];
  const seenSeqs = new Set<number>();

  const pushSlice = (slice: SearchableEventSlice | null) => {
    if (!slice || seenSeqs.has(slice.seq)) return;
    seenSeqs.add(slice.seq);
    slices.push(slice);
  };

  if (turn.user_prompt) {
    pushSlice(extractItemSearchSlice(turn.user_prompt, true));
  }
  if (
    turn.agent_message?.item_type === "agent_message" &&
    !isTimelineAgentMessageHidden(turn.agent_message)
  ) {
    pushSlice(extractItemSearchSlice(turn.agent_message, true));
  }
  for (const seg of turn.live_segments) {
    if (seg.kind === "assistant_message" || seg.kind === "notice") {
      pushSlice(extractItemSearchSlice(seg.item, true));
    } else if (seg.kind === "agent_message") {
      if (seg.item.item_type === "agent_message" && !isTimelineAgentMessageHidden(seg.item)) {
        pushSlice(extractItemSearchSlice(seg.item, true));
      }
    } else if (seg.kind === "step_group") {
      for (const item of seg.items) {
        pushSlice(extractItemSearchSlice(item, false));
      }
    }
  }
  if (turn.terminal_reply) {
    pushSlice(extractItemSearchSlice(turn.terminal_reply, true));
  }
  for (const notice of turn.notices) {
    pushSlice(extractItemSearchSlice(notice, true));
  }
  for (const item of turn.folded_steps) {
    pushSlice(extractItemSearchSlice(item, false));
  }

  return slices;
}

function countLiteralOccurrences(haystackLower: string, needleLower: string): number {
  if (!needleLower) return 0;
  let count = 0;
  let offset = 0;
  while (count < MAX_OCCURRENCES_PER_EVENT) {
    const idx = haystackLower.indexOf(needleLower, offset);
    if (idx < 0) break;
    count += 1;
    offset = idx + needleLower.length;
  }
  return count;
}

/**
 * Instant client-side case-insensitive substring matching across all currently loaded stage turns.
 * Preserves chronological turn and event order.
 */
export function findMatchesInLoadedTurns(
  turns: readonly StageTurnView[],
  rawQuery: string,
): StageFindMatch[] {
  const needle = rawQuery.trim().toLowerCase();
  if (!needle || turns.length === 0) return [];

  const matches: StageFindMatch[] = [];

  for (let turnIndex = 0; turnIndex < turns.length; turnIndex += 1) {
    const turn = turns[turnIndex];
    const slices = collectTurnSearchSlices(turn);
    let visibleOccurrenceInTurn = 0;

    for (const slice of slices) {
      const occurrences = countLiteralOccurrences(slice.text.toLowerCase(), needle);
      for (let occ = 0; occ < occurrences; occ += 1) {
        const visibleIdx = slice.visibleInDom ? visibleOccurrenceInTurn++ : null;
        matches.push({
          key: `${slice.seq}:${occ}`,
          seq: slice.seq,
          occurrenceInEvent: occ,
          visibleOccurrenceInTurn: visibleIdx,
          turnIndex,
          turnKey: turn.key,
          role: slice.role,
          unloaded: false,
          source: "loaded",
        });
      }
    }
  }

  return matches;
}

/**
 * Unify instant client-side loaded matches with debounced remote daemon FTS5 hits in strict
 * chronological `seq` order, deduplicating any `seq` already covered by the loaded scan while
 * surfacing unloaded historical matches (`seq < oldestLoadedSeq`) for auto-paginated jumping.
 */
export function mergeLoadedAndRemoteStageMatches(
  turns: readonly StageTurnView[],
  rawQuery: string,
  remoteHits: readonly ConversationSearchHit[],
  oldestLoadedSeq = 0,
): StageFindMatch[] {
  const trimmed = rawQuery.trim();
  if (!trimmed) return [];

  const loadedMatches = findMatchesInLoadedTurns(turns, trimmed);
  if (remoteHits.length === 0) {
    return loadedMatches;
  }

  const coveredSeqs = new Set<number>();
  for (const m of loadedMatches) {
    coveredSeqs.add(m.seq);
  }

  const effectiveOldestSeq =
    oldestLoadedSeq > 0 ? oldestLoadedSeq : (turns[0]?.anchor_seq ?? 0);
  const combined: StageFindMatch[] = [...loadedMatches];

  for (const hit of remoteHits) {
    if (hit.seq <= 0 || coveredSeqs.has(hit.seq)) {
      continue;
    }
    coveredSeqs.add(hit.seq);

    const turnLookup = findTurnIndexForSeq(turns as StageTurnView[], hit.seq);
    const isLoadedInStage =
      turnLookup.exact &&
      turnLookup.index >= 0 &&
      turnLookup.index < turns.length;

    if (isLoadedInStage) {
      combined.push({
        key: `${hit.seq}:0`,
        seq: hit.seq,
        occurrenceInEvent: 0,
        visibleOccurrenceInTurn: null,
        turnIndex: turnLookup.index,
        turnKey: turns[turnLookup.index].key,
        role: hit.role,
        snippet: hit.snippet,
        unloaded: false,
        source: "remote",
      });
    } else {
      const unloaded =
        Boolean(hit.is_unloaded) ||
        effectiveOldestSeq === 0 ||
        hit.seq < effectiveOldestSeq ||
        !turnLookup.exact;
      combined.push({
        key: `${hit.seq}:0`,
        seq: hit.seq,
        occurrenceInEvent: 0,
        visibleOccurrenceInTurn: null,
        turnIndex: null,
        turnKey: null,
        role: hit.role,
        snippet: hit.snippet,
        unloaded,
        source: "remote",
      });
    }
  }

  combined.sort((a, b) => a.seq - b.seq || a.occurrenceInEvent - b.occurrenceInEvent);
  return combined;
}

/**
 * Resolve the active match key after matches recompute (e.g. when remote history hits arrive or
 * older turns finish paginating), keeping the user's current position anchored without jarring jumps.
 */
export function resolveNextActiveFindKey(
  matches: readonly StageFindMatch[],
  previousKey: string | null,
): string | null {
  if (matches.length === 0) return null;

  if (previousKey) {
    const exact = matches.find((m) => m.key === previousKey);
    if (exact) return exact.key;

    const seqPart = Number.parseInt(previousKey.split(":")[0] ?? "", 10);
    if (!Number.isNaN(seqPart)) {
      const sameSeq = matches.find((m) => m.seq === seqPart);
      if (sameSeq) return sameSeq.key;
    }
  }

  // On a fresh query, prefer the first loaded match so typing never triggers unexpected pagination
  // when older remote hits arrive; if all matches are in unloaded history, pick the newest
  // historical match (closest to the current tail for fastest pagination).
  const firstLoaded = matches.find((m) => !m.unloaded);
  if (firstLoaded) return firstLoaded.key;
  return matches[matches.length - 1].key;
}

/**
 * Wrap-around match navigation (`+1` for next, `-1` for previous).
 */
export function stepStageFindIndex(
  currentIndex: number,
  total: number,
  delta: 1 | -1,
): number {
  if (total <= 0) return -1;
  const safeCurrent = currentIndex >= 0 && currentIndex < total ? currentIndex : 0;
  return (safeCurrent + delta + total) % total;
}

/**
 * Decide whether navigating to `match` must trigger `loadEarlierStageTurns` pagination loop.
 */
export function doesMatchRequireOlderHistoryPagination(
  match: StageFindMatch | null,
  turns: readonly StageTurnView[],
  windowStartTurnIndex: number,
  hasOlderHistory: boolean,
): boolean {
  if (!match) return false;
  if (!match.unloaded && match.turnIndex !== null && match.turnIndex >= 0 && match.turnIndex < turns.length) {
    return false;
  }
  const canLoadOlder = windowStartTurnIndex > 0 || hasOlderHistory;
  if (!canLoadOlder) return false;
  const firstLoadedAnchorSeq = turns[0]?.anchor_seq ?? Number.MAX_SAFE_INTEGER;
  return match.unloaded || match.seq < firstLoadedAnchorSeq;
}
