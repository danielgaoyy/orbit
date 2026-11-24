import { useMemo, useRef } from "react";
import type { StageTurnView } from "@/lib/orbit-ipc";
import { resolveDelegationDispatches, type ResolvedSubagentItem } from "@/lib/subagents";
import { isDelegationMilestoneToolCall, type ToolCallTimelineItem } from "@/lib/turn-delegation-split";

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
  return useMemo(
    () => resolveDelegationDispatches(delegationItems, sessionSubagents, hostId, messagePeers),
    [delegationItems, sessionSubagents, hostId, messagePeers],
  );
}
