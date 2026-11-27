import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { coalescePeerFinishes } from "./turn-delegation-split.ts";
import { extractCleanAgentMessageBody } from "./agent-message-clean.ts";
import type { TimelineItemView, TurnLiveSegmentView } from "./orbit-ipc.ts";

function makeFinishItem(
  seq: number,
  id: string,
  title: string,
  text: string,
  event: string | null = "finished",
): Extract<TimelineItemView, { item_type: "agent_message" }> {
  return {
    item_type: "agent_message",
    seq,
    message_id: id,
    kind: "finish_notification",
    source_agent_id: `ag_${id}`,
    source_host_id: "host_1",
    source_title: title,
    event,
    text,
    permission_context: null,
    delivery_status: "settled",
    delivery_turn_id: "turn_2",
    blocking_turn_id: null,
    queued_at_ms: 1000,
    dispatched_at_ms: 1010,
    settled_at_ms: 1500,
    blocks: [],
    timestamp_ms: 1000,
  };
}

describe("Personal Agent report presentation & coalescing", () => {
  it("preserves coalesced 2nd worker finish notifications on Personal Agent while suppressing routine finishes on coding sessions", () => {
    const firstFinish = makeFinishItem(
      10,
      "w1",
      "Fix Login Bug",
      "Agent ag_w1 (Fix Login Bug) finished.\n\n<agent-response>\nFixed JWT refresh.\n</agent-response>",
    );
    const secondFinish = makeFinishItem(
      11,
      "w2",
      "Run CI Checks",
      "Agent ag_w2 (Run CI Checks) finished.\n\n<agent-response>\nAll 503 tests green.\n</agent-response>",
    );
    const segments: TurnLiveSegmentView[] = [
      { kind: "agent_message", key: "am-11", item: secondFinish },
      {
        kind: "assistant_message",
        key: "msg-12",
        item: {
          item_type: "assistant_message",
          seq: 12,
          item_id: "a_12",
          text: "Both tasks finished: JWT refresh is fixed and all 503 tests pass.",
          streaming: false,
          blocks: [],
          timestamp_ms: 1200,
        },
      },
    ];

    const codingResult = coalescePeerFinishes(firstFinish, segments, "host_1");
    assert.equal(
      codingResult.segments.some((s) => s.kind === "agent_message"),
      false,
      "Regular coding sessions should continue hiding routine inline finish notifications",
    );

    const paResult = coalescePeerFinishes(firstFinish, segments, "host_1", {
      preserveFinishReports: true,
    });
    const preserved = paResult.segments.filter((s) => s.kind === "agent_message");
    assert.equal(
      preserved.length,
      1,
      "Personal Agent sessions must preserve coalesced worker finish notifications for source attribution & detail cards",
    );
    assert.equal(
      preserved[0]?.kind === "agent_message" && preserved[0].item.item_type === "agent_message"
        ? preserved[0].item.source_title
        : null,
      "Run CI Checks",
    );
  });

  it("extracts clean detail body without raw XML envelopes or machine finish boilerplate", () => {
    const raw =
      "<orbit-system kind=\"agent-message\">\nAgent ag_w1 (Fix Login) finished.\nHost: host_remote_1\n\n<agent-response>\n**Summary**: Updated token rotation.\n</agent-response>\n</orbit-system>";
    assert.equal(extractCleanAgentMessageBody(raw), "**Summary**: Updated token rotation.");
  });
});
