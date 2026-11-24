import { useMemo, useRef } from "react";
import type { StageTurnView } from "@/lib/orbit-ipc";
import { resolveDelegationDispatches, type ResolvedSubagentItem } from "@/lib/subagents";
import { isDelegationMilestoneToolCall, type ToolCallTimelineItem } from "@/lib/turn-delegation-split";

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

export function useDelegationDispatches(
  stageTurns: readonly StageTurnView[],
  sessionSubagents: readonly ResolvedSubagentItem[],
  hostId: string,
  messagePeers = sessionSubagents,
) {
  // Reserve each child once across chunks, including turns outside the virtual viewport.
  // Keep the batch stable across streamed updates that only change assistant text.
  const previousDelegations = useRef<readonly ToolCallTimelineItem[]>([]);
  const delegationItems = useMemo(() => {
    const next = stageTurns.flatMap((turn) => turn.live_segments.flatMap((segment) =>
      segment.kind === "step_group" ? segment.items.filter(isDelegationMilestoneToolCall) : [],
    ));
    const previous = previousDelegations.current;
    if (next.length === previous.length && next.every((item, i) => item === previous[i])) return previous;
    previousDelegations.current = next;
    return next;
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
