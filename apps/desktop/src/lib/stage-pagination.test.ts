import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { StageTurnView } from "./orbit-ipc.ts";
import {
  INITIAL_STAGE_TURN_WINDOW,
  LOG_INITIAL_LINE_CAP,
  LOG_PAGE_INCREMENT,
  LOG_TOP_PAGINATE_THRESHOLD_PX,
  OLDER_STAGE_TURN_PAGE_SIZE,
  computeEarlierTurnSliceRequest,
  computeLogWindowSlice,
  computeNextLogLineCap,
  computePrependRestoredScrollTop,
  mergePrependedStageTurns,
  mergeStageDeltaWithWindow,
  shouldTriggerTopLogPagination,
} from "./stage-pagination.ts";

function makeTurn(index: number, label = ""): StageTurnView {
  return {
    key: `turn-${index}`,
    turn_index: index,
    anchor_seq: (index + 1) * 10,
    user_prompt: null,
    agent_message: null,
    is_compaction_command: false,
    folded_steps: [],
    work_counts: {
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
    },
    work_summary_label: label,
    work_summary_live_label: label,
    work_summary_live_detail: null,
    work_summary_includes_failure: false,
    thought_duration_ms: null,
    work_duration_ms: null,
    turn_duration_ms: null,
    terminal_reply: null,
    last_assistant_reply: null,
    notices: [],
    compactions: [],
    live_segments: [],
    turn_diffs: [],
    has_error: false,
    has_unrecovered_error: false,
    has_active_item: false,
    started_at_ms: null,
    completed_at_ms: null,
  };
}

function makeTurnRange(start: number, count: number): StageTurnView[] {
  return Array.from({ length: count }, (_, i) => makeTurn(start + i));
}

describe("stage turn tail-window pagination (F-06)", () => {
  it("hydrates an initial tail window on full reset (sinceRevision === 0)", () => {
    const tailTurns = makeTurnRange(76, INITIAL_STAGE_TURN_WINDOW);
    const merged = mergeStageDeltaWithWindow(
      undefined,
      {
        reset: false,
        replace_from_turn_index: 76,
        changed_turns: tailTurns,
        window_start_turn_index: 76,
        total_turn_count: 100,
      },
      0,
    );

    assert.equal(merged.turns.length, 24);
    assert.equal(merged.turns[0].key, "turn-76");
    assert.equal(merged.turns[23].key, "turn-99");
    assert.equal(merged.window_start_turn_index, 76);
    assert.equal(merged.total_turn_count, 100);
  });

  it("maps absolute replace_from_turn_index into local window offsets on incremental deltas", () => {
    const initial = {
      turns: makeTurnRange(76, 24),
      window_start_turn_index: 76,
      total_turn_count: 100,
    };

    // Streaming update on the latest turn (absolute index 99 -> local index 23),
    // even if backend serializes default window_start_turn_index: 0 on non-reset delta.
    const updatedLast = makeTurn(99, "streaming-updated");
    const afterUpdate = mergeStageDeltaWithWindow(
      initial,
      {
        reset: false,
        replace_from_turn_index: 99,
        changed_turns: [updatedLast],
        window_start_turn_index: 0,
        total_turn_count: 100,
      },
      12,
    );

    assert.equal(afterUpdate.window_start_turn_index, 76);
    assert.equal(afterUpdate.total_turn_count, 100);
    assert.equal(afterUpdate.turns.length, 24);
    assert.equal(afterUpdate.turns[0].key, "turn-76");
    assert.equal(afterUpdate.turns[23].work_summary_label, "streaming-updated");

    // Appending a brand-new turn (absolute index 100 -> local index 24)
    const afterAppend = mergeStageDeltaWithWindow(
      afterUpdate,
      {
        reset: false,
        replace_from_turn_index: 100,
        changed_turns: [makeTurn(100, "new-turn")],
        total_turn_count: 101,
      },
      13,
    );

    assert.equal(afterAppend.window_start_turn_index, 76);
    assert.equal(afterAppend.total_turn_count, 101);
    assert.equal(afterAppend.turns.length, 25);
    assert.equal(afterAppend.turns[24].key, "turn-100");
  });

  it("preserves prev.turns reference on no-op incremental deltas and trims on in-window rewind", () => {
    const initial = {
      turns: makeTurnRange(50, 20),
      window_start_turn_index: 50,
      total_turn_count: 70,
    };

    const noOp = mergeStageDeltaWithWindow(
      initial,
      {
        reset: false,
        replace_from_turn_index: 70,
        changed_turns: [],
        total_turn_count: 70,
      },
      5,
    );
    assert.strictEqual(noOp.turns, initial.turns);
    assert.equal(noOp.window_start_turn_index, 50);

    // Rewind to absolute turn 60 with no replacement turns trims local turns[10..]
    const rewound = mergeStageDeltaWithWindow(
      initial,
      {
        reset: false,
        replace_from_turn_index: 60,
        changed_turns: [],
        total_turn_count: 60,
      },
      6,
    );
    assert.equal(rewound.window_start_turn_index, 50);
    assert.equal(rewound.total_turn_count, 60);
    assert.equal(rewound.turns.length, 10);
    assert.equal(rewound.turns[9].key, "turn-59");
  });

  it("shifts window_start_turn_index backward when incremental delta replaces before current window", () => {
    const initial = {
      turns: makeTurnRange(76, 24),
      window_start_turn_index: 76,
      total_turn_count: 100,
    };

    const replacement = makeTurnRange(50, 12);
    const merged = mergeStageDeltaWithWindow(
      initial,
      {
        reset: false,
        replace_from_turn_index: 50,
        changed_turns: replacement,
        total_turn_count: 62,
      },
      15,
    );

    assert.equal(merged.window_start_turn_index, 50);
    assert.equal(merged.total_turn_count, 62);
    assert.equal(merged.turns.length, 12);
    assert.equal(merged.turns[0].key, "turn-50");
    assert.equal(merged.turns[11].key, "turn-61");
  });

  it("computes earlier turn page slices cleanly including partial final page and jump targets", () => {
    assert.deepEqual(computeEarlierTurnSliceRequest(76, OLDER_STAGE_TURN_PAGE_SIZE), {
      startTurnIndex: 52,
      count: 24,
    });
    assert.deepEqual(computeEarlierTurnSliceRequest(10, OLDER_STAGE_TURN_PAGE_SIZE), {
      startTurnIndex: 0,
      count: 10,
    });
    assert.equal(computeEarlierTurnSliceRequest(0, OLDER_STAGE_TURN_PAGE_SIZE), null);

    // Jump target earlier than one page widens the fetch all the way to targetTurnIndex
    assert.deepEqual(computeEarlierTurnSliceRequest(76, OLDER_STAGE_TURN_PAGE_SIZE, 0), {
      startTurnIndex: 0,
      count: 76,
    });
    // Jump target inside the next page still fetches a full page
    assert.deepEqual(computeEarlierTurnSliceRequest(76, OLDER_STAGE_TURN_PAGE_SIZE, 60), {
      startTurnIndex: 52,
      count: 24,
    });
  });

  it("prepends earlier turns safely, deduplicates boundary keys, and drops stale async responses", () => {
    const current = {
      turns: makeTurnRange(76, 24),
      window_start_turn_index: 76,
      total_turn_count: 100,
    };

    const olderPage = makeTurnRange(52, 24);
    const prepended = mergePrependedStageTurns(current, olderPage, 52, 76, "turn-76");
    assert.equal(prepended.applied, true);
    assert.equal(prepended.prependedCount, 24);
    assert.equal(prepended.window_start_turn_index, 52);
    assert.equal(prepended.total_turn_count, 100);
    assert.equal(prepended.turns.length, 48);
    assert.equal(prepended.turns[0].key, "turn-52");
    assert.equal(prepended.turns[24].key, "turn-76");

    // Deduplicates an overlapping turn key cleanly
    const overlapping = [...makeTurnRange(28, 24), makeTurn(52)];
    const deduped = mergePrependedStageTurns(prepended, overlapping, 28, 52, "turn-52");
    assert.equal(deduped.applied, true);
    assert.equal(deduped.prependedCount, 24);
    assert.equal(deduped.window_start_turn_index, 28);
    assert.equal(deduped.turns.length, 72);

    // Rejects stale response if window_start_turn_index already moved
    const staleByWindow = mergePrependedStageTurns(deduped, makeTurnRange(52, 24), 52, 76, "turn-76");
    assert.equal(staleByWindow.applied, false);
    assert.equal(staleByWindow.prependedCount, 0);
    assert.strictEqual(staleByWindow.turns, deduped.turns);

    // Rejects stale response if stage reset replaced the first turn key
    const resetStage = {
      turns: [makeTurn(999)],
      window_start_turn_index: 76,
      total_turn_count: 77,
    };
    const staleByKey = mergePrependedStageTurns(resetStage, olderPage, 52, 76, "turn-76");
    assert.equal(staleByKey.applied, false);
    assert.equal(staleByKey.prependedCount, 0);
    assert.strictEqual(staleByKey.turns, resetStage.turns);
  });
});

describe("log viewer automatic top-scroll pagination (F-08)", () => {
  it("computes tail-window slice and expands automatically for earlier search targets", () => {
    const tail = computeLogWindowSlice(3000, LOG_INITIAL_LINE_CAP, null);
    assert.equal(tail.effectiveCap, 800);
    assert.equal(tail.startIdx, 2200);

    // Short log fits entirely
    const shortLog = computeLogWindowSlice(120, LOG_INITIAL_LINE_CAP, null);
    assert.equal(shortLog.startIdx, 0);

    // Find match on line 100 expands cap to cover line 100 with headroom
    const jumped = computeLogWindowSlice(3000, LOG_INITIAL_LINE_CAP, 100);
    assert.ok(jumped.startIdx < 100);
    assert.equal(jumped.effectiveCap, 2940);
  });

  it("bumps rendered line cap in bounded increments and restores scrollTop without jump", () => {
    assert.equal(computeNextLogLineCap(2500, 800, LOG_PAGE_INCREMENT), 1600);
    assert.equal(computeNextLogLineCap(2500, 2000, LOG_PAGE_INCREMENT), 2500);

    assert.equal(shouldTriggerTopLogPagination(1200, 180, false, LOG_TOP_PAGINATE_THRESHOLD_PX), true);
    assert.equal(shouldTriggerTopLogPagination(1200, 300, false, LOG_TOP_PAGINATE_THRESHOLD_PX), false);
    assert.equal(shouldTriggerTopLogPagination(1200, 120, true, LOG_TOP_PAGINATE_THRESHOLD_PX), false);
    assert.equal(shouldTriggerTopLogPagination(0, 10, false, LOG_TOP_PAGINATE_THRESHOLD_PX), false);

    // Prepending 800 lines adding 15,200px of scrollHeight keeps exact reading position
    assert.equal(computePrependRestoredScrollTop(140, 14_000, 29_200), 15_340);
    // Non-positive height delta never moves scrollTop backward
    assert.equal(computePrependRestoredScrollTop(140, 14_000, 13_800), 140);
  });
});
