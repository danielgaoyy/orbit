import { useMemo, useRef } from "react";
import type { StageTurnView } from "@/lib/orbit-ipc";
import { resolveDelegationDispatches, type ResolvedSubagentItem } from "@/lib/subagents";
import { isSubagentOrMessageToolCall, type ToolCallTimelineItem } from "@/lib/turn-delegation-split";

type DispatchMap = ReturnType<typeof resolveDelegationDispatches>;
const EMPTY_DISPATCHES: DispatchMap = new Map();

function sameDispatches(previous: DispatchMap, next: DispatchMap): boolean {
  if (previous.size !== next.size) return false;
  for (const [item, match] of next) {
    const prior = previous.get(item);
    if (!prior || prior.action !== match.action || prior.linked !== match.linked) return false;
    // The resolver clones launch records, including the session's assigned portrait.
    const a = prior.subagent;
    const b = match.subagent;
    if (a === b) continue;
    const keys = Object.keys(b) as (keyof typeof b)[];
    if (Object.keys(a).length !== keys.length || keys.some((key) => a[key] !== b[key])) return false;
  }
  return true;
}

const EMPTY_DELEGATION_ITEMS: readonly ToolCallTimelineItem[] = [];
const turnDelegationItemsCache = new WeakMap<StageTurnView, readonly ToolCallTimelineItem[]>();

function getTurnDelegationItems(turn: StageTurnView): readonly ToolCallTimelineItem[] {
  const cached = turnDelegationItemsCache.get(turn);
  if (cached !== undefined) return cached;
  let found: ToolCallTimelineItem[] | null = null;
  for (const segment of turn.live_segments) {
    if (segment.kind !== "step_group") continue;
    for (const item of segment.items) {
      if (isSubagentOrMessageToolCall(item)) {
        found ??= [];
        found.push(item);
      }
    }
  }
  const result = found ?? EMPTY_DELEGATION_ITEMS;
  turnDelegationItemsCache.set(turn, result);
  return result;
}

export function useDelegationDispatches(
  stageTurns: readonly StageTurnView[],
  sessionSubagents: readonly ResolvedSubagentItem[],
  hostId: string,
  messagePeers = sessionSubagents,
) {
  // Reserve each child once across chunks, including turns outside the virtual viewport.
  // Cache per-turn extractions by WeakMap so streaming updates on the active tail turn
  // never re-scan or allocate arrays for immutable historical turns.
  const previousDelegations = useRef<readonly ToolCallTimelineItem[]>(EMPTY_DELEGATION_ITEMS);
  const delegationItems = useMemo(() => {
    let next: ToolCallTimelineItem[] | null = null;
    for (let i = 0; i < stageTurns.length; i += 1) {
      const items = getTurnDelegationItems(stageTurns[i]);
      if (items.length === 0) continue;
      next ??= [];
      for (let j = 0; j < items.length; j += 1) {
        next.push(items[j]);
      }
    }
    const resolved = next ?? EMPTY_DELEGATION_ITEMS;
    const previous = previousDelegations.current;
    if (resolved.length === previous.length && resolved.every((item, i) => item === previous[i])) {
      return previous;
    }
    previousDelegations.current = resolved;
    return resolved;
  }, [stageTurns]);
  const previousDispatches = useRef(EMPTY_DISPATCHES);
  return useMemo(() => {
    const next = delegationItems.length
      ? resolveDelegationDispatches(delegationItems, sessionSubagents, hostId, messagePeers)
      : EMPTY_DISPATCHES;
    if (next.size === 0) previousDispatches.current = EMPTY_DISPATCHES;
    else if (!sameDispatches(previousDispatches.current, next)) previousDispatches.current = next;
    return previousDispatches.current;
  }, [delegationItems, sessionSubagents, hostId, messagePeers]);
}
